//! Publishing a folder rather than a card session.
//!
//! `docs/publish-folder-plan.md`. The session road assumes a manifest, an
//! arrival report and a plan produced by the desktop handoff; this one assumes
//! a folder somebody put files in — by copying them there, or by pointing a
//! tool's output at it.
//!
//! **The state machine underneath is the same one.** `Publisher` reads each
//! file as `staging_dir.join(file_name)`, so a folder publish is that publisher
//! with `staging_dir` set to the publishing folder and `file_name` set to a
//! path relative to it. Retries, resumption and the mandatory dry run all come
//! along unchanged, which is the point of building it this way rather than
//! writing a second publisher that would drift from the first.

use crate::error::Error;
use crate::ingest::scanner::hash_file;
use crate::ledger::Ledger;
use crate::publish::publisher::{PublishItem, PublishPlan, ResumeCounts, Skipped};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// What Google Photos is asked to take.
///
/// Deliberately a list rather than "anything in the folder": a stray `.txt`,
/// a sidecar or a `.DS_Store` is not a photograph, and uploading it — or
/// failing on it — is worse than saying plainly that it was not one.
pub const ACCEPTED: [&str; 8] = ["jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp"];

/// §6.1's batch limit.
const BATCH_LIMIT: usize = 50;

/// One file found in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFile {
    /// Absolute, for reading.
    pub path: PathBuf,
    /// Relative to the publishing folder — what `Publisher` joins back on, and
    /// what keeps a file in a subfolder distinguishable from one beside it.
    pub rel_path: String,
    pub sha256: String,
    pub bytes: u64,
}

/// Everything publishable in the folder, in a stable order.
///
/// Sorted by relative path so two runs over the same folder produce the same
/// plan — which is what lets the session id below be a fact about the contents
/// rather than about when somebody looked.
pub fn walk(dir: &Path) -> Result<Vec<FolderFile>, Error> {
    let mut found = Vec::new();

    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| Error::Internal(format!("reading the folder: {e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_accepted(path) {
            continue;
        }

        let rel_path = path
            .strip_prefix(dir)
            .map_err(|_| Error::Internal(format!("{} is not inside the folder", path.display())))?
            .to_string_lossy()
            .to_string();

        found.push(FolderFile {
            sha256: hash_file(path)?,
            bytes: std::fs::metadata(path)?.len(),
            path: path.to_path_buf(),
            rel_path,
        });
    }

    found.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(found)
}

/// Everything in the folder that is *not* publishable, so it can be reported.
///
/// A file this tool will not upload is a file that will still be there after
/// the folder is emptied, and somebody should know which before they wonder
/// why it stayed.
pub fn unpublishable(dir: &Path) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| Error::Internal(format!("reading the folder: {e}")))?;
        if entry.file_type().is_file() && !is_accepted(entry.path()) {
            found.push(
                entry
                    .path()
                    .strip_prefix(dir)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    found.sort();
    Ok(found)
}

fn is_accepted(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ACCEPTED.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// The session id for publishing this exact set of bytes.
///
/// **Derived from the contents, not from the folder or the clock**, and that is
/// what binds §9.2 rule 3's mandatory dry run to what was actually reviewed.
/// The dry run is recorded against this id; add a file to the folder afterwards
/// and the id changes, so the recorded dry run no longer matches and publishing
/// refuses until somebody looks again. A session id that was a fact about the
/// *folder* would let a file slip in between the review and the upload — and
/// the Google Photos API cannot delete what it receives.
///
/// Stable across a resumed run: a partial failure leaves the same files in
/// place, so the same id, so the same rows are resumed rather than doubled.
pub fn session_id(dir: &Path, files: &[FolderFile]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dir.to_string_lossy().as_bytes());
    for file in files {
        hasher.update(file.rel_path.as_bytes());
        hasher.update(file.sha256.as_bytes());
    }
    format!("folder-{}", crate::ingest::scanner::hex(&hasher.finalize()))
}

/// What publishing this folder would do. Writes nothing.
///
/// Deduplication asks a weaker question here than F16 asks of a card: *have I
/// uploaded exactly these bytes?* rather than *have I published this
/// photograph?*. The tools rewrite files — geotagging changes a photograph's
/// hash — so the same frame processed two ways is two different files to this
/// check. What actually keeps a folder from being published twice is that a
/// successful publish empties it.
pub fn plan_folder(dir: &Path, ledger: &Ledger) -> Result<PublishPlan, Error> {
    let files = walk(dir)?;
    let session = session_id(dir, &files);

    let hashes: Vec<String> = files.iter().map(|f| f.sha256.clone()).collect();
    let already = ledger
        .published_among(&hashes)
        .map_err(|e| Error::Internal(e.to_string()))?;

    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut total_bytes = 0u64;

    for file in &files {
        if already.contains(&file.sha256) {
            skipped.push(Skipped {
                stem: file.rel_path.clone(),
                reason: "these exact bytes have been uploaded before".into(),
            });
            continue;
        }

        total_bytes += file.bytes;
        items.push(PublishItem {
            // Deterministic, so a resumed publish continues the same rows
            // rather than writing a second set of them.
            shot_id: format!("{session}:{}", file.sha256),
            stem: file.rel_path.clone(),
            source_sha256: file.sha256.clone(),
            file_name: file.rel_path.clone(),
            bytes: file.bytes,
        });
    }

    for name in unpublishable(dir)? {
        skipped.push(Skipped {
            stem: name,
            reason: format!(
                "not a photograph this uploads. Accepted: {}",
                ACCEPTED
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        });
    }

    let upload_requests = items.len();
    let batch_create_requests = items.len().div_ceil(BATCH_LIMIT);

    Ok(PublishPlan {
        session_id: session,
        items,
        skipped,
        total_bytes,
        upload_requests,
        batch_create_requests,
        resuming: ResumeCounts::default(),
    })
}

/// Compute the plan **and record that somebody looked** (§9.2 rule 3).
///
/// The same split the session road makes, and for the same reason: if
/// publishing could build its own plan it would satisfy its own precondition,
/// and the safeguard would be a formality. `plan_folder` computes; this one
/// computes and leaves a mark in the database — not in memory, because the API
/// cannot delete and a safeguard a restart forgets is not a safeguard.
pub fn dry_run_folder(dir: &Path, ledger: &Ledger) -> Result<PublishPlan, Error> {
    let plan = plan_folder(dir, ledger)?;
    ledger
        .open_folder_session(&plan.session_id, &dir.to_string_lossy())
        .map_err(|e| Error::Internal(e.to_string()))?;
    ledger
        .record_dry_run(&plan.session_id)
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder holding the named files.
    fn folder(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
        }
        dir
    }

    fn ledger() -> Ledger {
        Ledger::open_in_memory().unwrap()
    }

    #[test]
    fn every_photograph_in_the_folder_is_planned() {
        let dir = folder(&[("a.jpg", b"one"), ("b.jpeg", b"two"), ("c.PNG", b"three")]);
        let plan = plan_folder(dir.path(), &ledger()).unwrap();

        assert_eq!(plan.items.len(), 3, "skipped: {:?}", plan.skipped);
        assert_eq!(plan.upload_requests, 3);
        assert_eq!(plan.batch_create_requests, 1);
    }

    #[test]
    fn a_photograph_in_a_subfolder_is_planned_with_its_path() {
        // Subfolders are included — a tool writing borders into a folder of its
        // own is a normal thing to do — and the relative path is what the
        // publisher joins back on, so two files called a.jpg stay distinct.
        let dir = folder(&[("a.jpg", b"one"), ("borders/a.jpg", b"bordered")]);
        let plan = plan_folder(dir.path(), &ledger()).unwrap();

        let names: Vec<&str> = plan.items.iter().map(|i| i.file_name.as_str()).collect();
        assert_eq!(names, vec!["a.jpg", "borders/a.jpg"]);
    }

    #[test]
    fn anything_that_is_not_a_photograph_is_reported_rather_than_uploaded() {
        // It will still be in the folder after publishing empties it, and
        // somebody should know which file stayed and why.
        let dir = folder(&[("a.jpg", b"one"), ("notes.txt", b"x"), (".DS_Store", b"y")]);
        let plan = plan_folder(dir.path(), &ledger()).unwrap();

        assert_eq!(plan.items.len(), 1);
        let reasons: Vec<&str> = plan.skipped.iter().map(|s| s.stem.as_str()).collect();
        assert!(reasons.contains(&"notes.txt"), "got {reasons:?}");
        assert!(plan.skipped.iter().all(|s| s.reason.contains(".jpg")));
    }

    #[test]
    fn an_empty_folder_says_so_rather_than_reporting_zero_of_zero() {
        let dir = folder(&[]);
        let plan = plan_folder(dir.path(), &ledger()).unwrap();

        assert_eq!(plan.items.len(), 0);
        assert!(
            plan.describe().contains("nothing to publish"),
            "{}",
            plan.describe()
        );
    }

    #[test]
    fn bytes_already_uploaded_are_not_uploaded_again() {
        let dir = folder(&[("a.jpg", b"one"), ("b.jpg", b"two")]);
        let ledger = ledger();
        let files = walk(dir.path()).unwrap();

        ledger
            .record_published_file(&files[0].sha256, "a.jpg", "earlier", Some("media-1"))
            .unwrap();

        let plan = plan_folder(dir.path(), &ledger).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].file_name, "b.jpg");
        assert!(plan
            .skipped
            .iter()
            .any(|s| s.reason.contains("exact bytes")));
    }

    #[test]
    fn the_two_kinds_of_deduplication_key_stay_distinguishable() {
        // F16 answers "have I published this photograph?" from the source hash;
        // a folder publish answers "have I uploaded these bytes?". Both live in
        // one table, and a later reader must not mistake one for the other.
        let ledger = ledger();
        ledger
            .record_published(
                &"a".repeat(64),
                "IMG_0001",
                &"b".repeat(64),
                "session-1",
                Some("media-1"),
            )
            .unwrap();
        ledger
            .record_published_file(&"c".repeat(64), "a.jpg", "folder-1", Some("media-2"))
            .unwrap();

        assert_eq!(
            ledger
                .published_key_kind(&"a".repeat(64))
                .unwrap()
                .as_deref(),
            Some("source")
        );
        assert_eq!(
            ledger
                .published_key_kind(&"c".repeat(64))
                .unwrap()
                .as_deref(),
            Some("file")
        );
    }

    #[test]
    fn the_same_folder_planned_twice_gets_the_same_session() {
        // A resumed publish has to continue the rows it started, not double
        // them, and the dry run recorded before the failure has to still count.
        let dir = folder(&[("a.jpg", b"one")]);
        let first = plan_folder(dir.path(), &ledger()).unwrap();
        let second = plan_folder(dir.path(), &ledger()).unwrap();

        assert_eq!(first.session_id, second.session_id);
    }

    #[test]
    fn adding_a_file_changes_the_session_and_so_invalidates_the_dry_run() {
        // The safeguard §9.2 rule 3 is actually asking for. A dry run says
        // somebody looked at *these* photographs; a file that arrived after
        // they looked has not been reviewed, and the API cannot delete what it
        // receives.
        let dir = folder(&[("a.jpg", b"one")]);
        let ledger = ledger();

        let reviewed = dry_run_folder(dir.path(), &ledger).unwrap();
        assert!(ledger.dry_run_at(&reviewed.session_id).unwrap().is_some());

        std::fs::write(dir.path().join("b.jpg"), b"snuck in").unwrap();
        let now = plan_folder(dir.path(), &ledger).unwrap();

        assert_ne!(now.session_id, reviewed.session_id);
        assert!(
            ledger.dry_run_at(&now.session_id).unwrap().is_none(),
            "the new set of files has not been reviewed"
        );
    }

    #[test]
    fn changing_a_files_contents_also_invalidates_the_dry_run() {
        // Geotagging between the review and the upload rewrites the file. What
        // was reviewed is not what would be published.
        let dir = folder(&[("a.jpg", b"one")]);
        let ledger = ledger();
        let reviewed = dry_run_folder(dir.path(), &ledger).unwrap();

        std::fs::write(dir.path().join("a.jpg"), b"now geotagged").unwrap();
        let now = plan_folder(dir.path(), &ledger).unwrap();

        assert_ne!(now.session_id, reviewed.session_id);
    }

    #[test]
    fn a_dry_run_writes_no_files_and_leaves_the_folder_as_it_was() {
        let dir = folder(&[("a.jpg", b"one"), ("notes.txt", b"x")]);
        let before: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();

        dry_run_folder(dir.path(), &ledger()).unwrap();

        let after: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(before.len(), after.len());
    }

    #[test]
    fn a_second_dry_run_does_not_clear_the_first() {
        // `INSERT OR IGNORE` rather than `OR REPLACE`: replacing the session
        // row would wipe the stamp saying somebody looked.
        let dir = folder(&[("a.jpg", b"one")]);
        let ledger = ledger();

        let first = dry_run_folder(dir.path(), &ledger).unwrap();
        let stamped = ledger.dry_run_at(&first.session_id).unwrap();
        dry_run_folder(dir.path(), &ledger).unwrap();

        assert!(stamped.is_some());
        assert!(ledger.dry_run_at(&first.session_id).unwrap().is_some());
    }

    #[test]
    fn fifty_one_photographs_need_two_batch_calls() {
        // §6.1's limit, which the session road already respects.
        let files: Vec<(String, Vec<u8>)> = (0..51)
            .map(|i| (format!("{i:03}.jpg"), format!("photo {i}").into_bytes()))
            .collect();
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, c)| (n.as_str(), c.as_slice()))
            .collect();
        let dir = folder(&refs);

        let plan = plan_folder(dir.path(), &ledger()).unwrap();
        assert_eq!(plan.items.len(), 51);
        assert_eq!(plan.batch_create_requests, 2);
    }
}
