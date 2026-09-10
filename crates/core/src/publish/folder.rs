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
use serde::{Deserialize, Serialize};
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

/// Publish a folder and then empty it.
///
/// The two halves of the operation, in the only order they can happen in, with
/// the deletion reading what the upload recorded rather than what it returned.
/// That indirection is deliberate: the publish outcome is a summary of a run,
/// and the deletion needs evidence about a *file*.
///
/// `Publisher::publish` refuses without a recorded dry run (§9.2 rule 3), so
/// rule 4 of the deletion comes free — there is no path to here that skipped
/// the review.
///
/// A failed upload does not stop the run. Three failures out of four hundred
/// leave three files in the folder and publish the rest; halting would leave a
/// half-published folder and no clear way to resume, and rule 2 already means a
/// file that failed is never deleted.
pub fn publish_folder(
    dir: &Path,
    config: &crate::config::Config,
    ledger: &Ledger,
    api: &dyn crate::publish::PhotosApi,
    tokens: &dyn crate::publish::AccessTokens,
    sleeper: &dyn crate::publish::Sleeper,
    progress: &dyn crate::jobs::Progress,
) -> Result<(crate::publish::PublishOutcome, EmptyReport), Error> {
    // Refused before the first upload, not after: publishing to Google and
    // then discovering the folder cannot be emptied would leave photographs
    // uploaded that nobody can clear.
    config.resolve_for_publishing(dir)?;

    let plan = plan_folder(dir, ledger)?;

    let publisher = crate::publish::Publisher {
        ledger,
        api,
        tokens,
        sleeper,
        // The folder *is* the staging directory: `Publisher` reads each file as
        // `staging_dir.join(file_name)`, and `file_name` is relative to here.
        staging_dir: dir.to_path_buf(),
        // The weaker claim, written at the moment the row is.
        key_kind: "file",
    };

    let outcome = publisher.publish(&plan, progress)?;
    let emptied = empty_published(dir, config, &plan, ledger)?;

    Ok((outcome, emptied))
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
    fn publishing_a_folder_refuses_without_a_dry_run() {
        // Rule 4 comes free, and this is the test that says so rather than
        // assuming it: §9.2 rule 3 is checked against the database by
        // `Publisher::publish`, and `publish_folder` cannot get past it.
        use crate::config::Config;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("Publishing");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"one").unwrap();
        let dir = dir.canonicalize().unwrap();

        let config = Config {
            publishing_dir: Some(dir.clone()),
            ..Config::default()
        };
        let ledger = ledger();

        // No dry run recorded for this set of bytes.
        let err = publish_folder(
            &dir,
            &config,
            &ledger,
            &crate::publish::HttpPhotosApi::new(),
            &NoTokens,
            &crate::publish::RealSleeper,
            &crate::jobs::InMemoryProgress::new(),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("dry run"),
            "the refusal should name what is missing: {err}"
        );
        assert!(dir.join("a.jpg").exists(), "and nothing was touched");
    }

    /// Never asked for a token: the refusal happens before any network work.
    struct NoTokens;
    impl crate::publish::AccessTokens for NoTokens {
        fn access_token(&self) -> Result<String, Error> {
            panic!("publishing must refuse before it asks for a token")
        }
        fn invalidate(&self) {}
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

// ---------------------------------------------------------------------------
// Emptying the folder
// ---------------------------------------------------------------------------

/// A file that was uploaded and then removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub rel_path: String,
    pub bytes: u64,
    /// What Google called it. The evidence the deletion rests on.
    pub media_item_id: String,
}

/// A file that stayed, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub rel_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyReport {
    pub removed: Vec<Removed>,
    pub kept: Vec<Kept>,
    /// Subdirectories left with nothing in them, and tidied away.
    pub directories_removed: usize,
}

impl EmptyReport {
    pub fn describe(&self) -> String {
        if self.removed.is_empty() {
            return format!(
                "Nothing removed from the publishing folder; {} file(s) stayed",
                self.kept.len()
            );
        }
        let bytes: u64 = self.removed.iter().map(|r| r.bytes).sum();
        let mut line = format!(
            "{} file(s) removed from the publishing folder ({:.1} MB)",
            self.removed.len(),
            bytes as f64 / 1_048_576.0
        );
        if !self.kept.is_empty() {
            line.push_str(&format!(", {} kept", self.kept.len()));
        }
        line
    }
}

/// Empty the publishing folder of everything Google confirmed it received.
///
/// The one irreversible step in the application, and irreversible in both
/// directions — Google cannot un-publish, and a deleted file is gone. Four
/// rules govern it, and each exists because of a specific way this could go
/// wrong (`docs/publish-folder-plan.md`).
///
/// **1. Only the configured publishing folder is deleted from.** Every path
/// goes through `Config::resolve_for_publishing`, which compares canonicalised
/// paths — so a symlink inside the folder pointing at an archive resolves to
/// the archive and is refused, and a `..` cannot climb out.
///
/// **2. A file is removed only when Google returned a media item id for it.**
/// Per file, from the publish row, not from the run's outcome: "the job
/// finished" is not evidence about any particular photograph. A file whose
/// upload failed, or whose answer never arrived, stays.
///
/// **3. Everything here is a copy** — enforced by `resolve_for_create`
/// refusing tool output into this folder — so removing it destroys nothing.
///
/// **4. A dry run came first**, which `Publisher::publish` already refuses
/// without.
///
/// Anything in the folder that was not part of what was published stays, and
/// is listed, because a file that quietly survives an "empty" is a file
/// somebody will wonder about.
pub fn empty_published(
    dir: &Path,
    config: &crate::config::Config,
    plan: &PublishPlan,
    ledger: &Ledger,
) -> Result<EmptyReport, Error> {
    // Rule 1, before anything else: is this the folder at all?
    config.resolve_for_publishing(dir)?;

    let mut report = EmptyReport::default();
    let mut accounted: std::collections::HashSet<String> = std::collections::HashSet::new();

    for item in &plan.items {
        accounted.insert(item.file_name.clone());
        let path = dir.join(&item.file_name);

        // Rule 2. The publish row is per photograph and says what Google said.
        let row = ledger
            .publish_row(&item.shot_id)
            .map_err(|e| Error::Internal(e.to_string()))?;
        let confirmed = row.as_ref().and_then(|r| {
            (r.state == "created")
                .then(|| r.media_item_id.clone())
                .flatten()
        });

        let Some(media_item_id) = confirmed else {
            report.kept.push(Kept {
                rel_path: item.file_name.clone(),
                reason: match row.as_ref().map(|r| r.state.as_str()) {
                    Some("uploaded") => {
                        "uploaded, but Google never confirmed it as a media item".into()
                    }
                    Some(state) => format!("not published: {state}"),
                    None => "no record of it being published".into(),
                },
            });
            continue;
        };

        // Rule 1 again, per file: the plan's names came from a walk of this
        // folder, but a name is not a location until it is resolved.
        let resolved = match config.resolve_for_publishing(&path) {
            Ok(resolved) => resolved,
            Err(e) => {
                report.kept.push(Kept {
                    rel_path: item.file_name.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        };

        let bytes = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
        match std::fs::remove_file(&resolved) {
            Ok(()) => report.removed.push(Removed {
                rel_path: item.file_name.clone(),
                bytes,
                media_item_id,
            }),
            Err(e) => report.kept.push(Kept {
                rel_path: item.file_name.clone(),
                reason: format!("could not be removed: {e}"),
            }),
        }
    }

    // Everything else in the folder. Not an error — a `.txt` was never going to
    // be published, and a file that arrived after the plan was made was never
    // reviewed — but it is going to still be there, so it is said out loud.
    for file in walk(dir)? {
        if !accounted.contains(&file.rel_path) {
            report.kept.push(Kept {
                rel_path: file.rel_path,
                reason: "not part of what was published".into(),
            });
        }
    }
    for name in unpublishable(dir)? {
        report.kept.push(Kept {
            rel_path: name,
            reason: "not a photograph this uploads".into(),
        });
    }

    report.directories_removed = remove_empty_subdirectories(dir);
    Ok(report)
}

/// Tidy away subdirectories that are now empty, deepest first.
///
/// The publishing folder itself is never removed — it is configuration, and
/// the next publish expects it to be there. A directory with anything still in
/// it is left alone, because a file still in it is a file that did not publish.
fn remove_empty_subdirectories(dir: &Path) -> usize {
    let mut directories: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != dir)
        .map(|e| e.path().to_path_buf())
        .collect();

    // Deepest first, so a directory holding only empty directories also goes.
    directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    directories
        .iter()
        .filter(|path| std::fs::remove_dir(path).is_ok())
        .count()
}

#[cfg(test)]
mod emptying_tests {
    use super::*;
    use crate::config::Config;

    /// A publishing folder, and a configuration that admits it.
    fn publishing(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Config, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("Publishing");
        std::fs::create_dir(&dir).unwrap();
        for (name, contents) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
        }
        let dir = dir.canonicalize().unwrap();
        let config = Config {
            publishing_dir: Some(dir.clone()),
            ..Config::default()
        };
        (temp, config, dir)
    }

    /// Mark a planned item the way a confirmed upload would.
    fn confirm(ledger: &Ledger, plan: &PublishPlan, file_name: &str, media_item_id: &str) {
        let item = plan
            .items
            .iter()
            .find(|i| i.file_name == file_name)
            .expect("planned");
        ledger
            .queue_publish(
                &item.shot_id,
                &plan.session_id,
                &item.stem,
                &item.source_sha256,
                &item.file_name,
            )
            .unwrap();
        ledger.record_upload(&item.shot_id, "token").unwrap();
        ledger
            .record_created_and_published(
                &item.shot_id,
                media_item_id,
                &item.source_sha256,
                &item.stem,
                &item.file_name,
                "file",
            )
            .unwrap();
    }

    /// Queue an item and leave it uploaded but unconfirmed.
    fn leave_unconfirmed(ledger: &Ledger, plan: &PublishPlan, file_name: &str) {
        let item = plan
            .items
            .iter()
            .find(|i| i.file_name == file_name)
            .expect("planned");
        ledger
            .queue_publish(
                &item.shot_id,
                &plan.session_id,
                &item.stem,
                &item.source_sha256,
                &item.file_name,
            )
            .unwrap();
        ledger.record_upload(&item.shot_id, "token").unwrap();
    }

    #[test]
    fn a_photograph_google_confirmed_is_removed() {
        let (_t, config, dir) = publishing(&[("a.jpg", b"one")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        confirm(&ledger, &plan, "a.jpg", "media-1");

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert_eq!(report.removed.len(), 1);
        assert_eq!(report.removed[0].media_item_id, "media-1");
        assert!(!dir.join("a.jpg").exists());
    }

    #[test]
    fn a_photograph_whose_upload_failed_is_still_there_afterwards() {
        // Rule 2, and the single most important test in this module: a failure
        // must never be mistaken for a success by the thing that deletes.
        let (_t, config, dir) = publishing(&[("a.jpg", b"one"), ("b.jpg", b"two")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        confirm(&ledger, &plan, "a.jpg", "media-1");
        // b.jpg never even got queued: its upload failed outright.

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(!dir.join("a.jpg").exists());
        assert!(
            dir.join("b.jpg").exists(),
            "a failed upload must not be deleted"
        );
        assert_eq!(report.removed.len(), 1);
        assert!(report.kept.iter().any(|k| k.rel_path == "b.jpg"));
    }

    #[test]
    fn a_photograph_uploaded_but_never_confirmed_is_kept() {
        // §9.2 invariant 6 at its most literal: what cannot be verified is not
        // claimed, and here the claim would delete the only sensible copy to
        // re-try from.
        let (_t, config, dir) = publishing(&[("a.jpg", b"one")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        leave_unconfirmed(&ledger, &plan, "a.jpg");

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(dir.join("a.jpg").exists());
        assert_eq!(report.removed.len(), 0);
        assert!(report.kept[0].reason.contains("never confirmed"));
    }

    #[test]
    fn a_run_where_everything_failed_deletes_nothing() {
        let (_t, config, dir) = publishing(&[("a.jpg", b"one"), ("b.jpg", b"two")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert_eq!(report.removed.len(), 0);
        assert!(dir.join("a.jpg").exists());
        assert!(dir.join("b.jpg").exists());
        assert!(
            report.describe().contains("Nothing removed"),
            "{}",
            report.describe()
        );
    }

    #[test]
    fn a_file_that_arrived_after_the_plan_was_made_is_not_deleted() {
        // It was never reviewed and never uploaded. Deleting it would destroy
        // something nobody had looked at.
        let (_t, config, dir) = publishing(&[("a.jpg", b"one")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        confirm(&ledger, &plan, "a.jpg", "media-1");

        std::fs::write(dir.join("late.jpg"), b"arrived after the plan").unwrap();
        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(dir.join("late.jpg").exists());
        assert!(report
            .kept
            .iter()
            .any(|k| k.rel_path == "late.jpg" && k.reason.contains("not part of")));
    }

    #[test]
    fn something_that_is_not_a_photograph_stays_and_is_said_out_loud() {
        let (_t, config, dir) = publishing(&[("a.jpg", b"one"), ("notes.txt", b"keep me")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        confirm(&ledger, &plan, "a.jpg", "media-1");

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(dir.join("notes.txt").exists());
        assert!(report.kept.iter().any(|k| k.rel_path == "notes.txt"));
    }

    #[test]
    fn the_publishing_folder_itself_survives_being_emptied() {
        let (_t, config, dir) = publishing(&[("a.jpg", b"one")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        confirm(&ledger, &plan, "a.jpg", "media-1");

        empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(dir.exists(), "the folder is configuration, not content");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn an_emptied_subfolder_is_tidied_away_but_one_still_holding_a_file_is_not() {
        let (_t, config, dir) = publishing(&[
            ("borders/a.jpg", b"one"),
            ("keep/b.jpg", b"two"),
            ("keep/notes.txt", b"x"),
        ]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();
        confirm(&ledger, &plan, "borders/a.jpg", "media-1");
        confirm(&ledger, &plan, "keep/b.jpg", "media-2");

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(!dir.join("borders").exists(), "empty, so tidied away");
        assert!(dir.join("keep").exists(), "still holds notes.txt");
        assert_eq!(report.directories_removed, 1);
    }

    #[test]
    fn a_folder_that_is_not_the_publishing_folder_is_refused_outright() {
        // Rule 1. Nothing is examined, let alone deleted.
        let (temp, config, dir) = publishing(&[("a.jpg", b"one")]);
        let ledger = Ledger::open_in_memory().unwrap();
        let plan = plan_folder(&dir, &ledger).unwrap();

        let elsewhere = temp.path().join("Archive");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("precious.jpg"), b"do not touch").unwrap();

        let err = empty_published(&elsewhere, &config, &plan, &ledger).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
        assert!(elsewhere.join("precious.jpg").exists());
    }

    #[test]
    fn a_symlink_out_of_the_folder_is_not_followed() {
        // Rule 1 per file. Without canonicalising, this path is textually
        // inside the publishing folder and deleting it deletes an archive.
        let (temp, config, dir) = publishing(&[]);
        let archive = temp.path().join("archive");
        std::fs::create_dir(&archive).unwrap();
        std::fs::write(archive.join("precious.jpg"), b"do not touch").unwrap();
        std::os::unix::fs::symlink(archive.join("precious.jpg"), dir.join("precious.jpg")).unwrap();

        let ledger = Ledger::open_in_memory().unwrap();
        // Plan by hand: `walk` does not follow links, so the planner would not
        // have offered this file — the check is what happens if anything ever
        // put it in front of the deleter.
        let plan = PublishPlan {
            session_id: "folder-test".into(),
            items: vec![PublishItem {
                shot_id: "folder-test:x".into(),
                stem: "precious.jpg".into(),
                source_sha256: "0".repeat(64),
                file_name: "precious.jpg".into(),
                bytes: 0,
            }],
            skipped: vec![],
            total_bytes: 0,
            upload_requests: 1,
            batch_create_requests: 1,
            resuming: ResumeCounts::default(),
        };
        confirm(&ledger, &plan, "precious.jpg", "media-1");

        let report = empty_published(&dir, &config, &plan, &ledger).unwrap();

        assert!(
            archive.join("precious.jpg").exists(),
            "the archive is untouched"
        );
        assert_eq!(report.removed.len(), 0);
        assert!(report.kept[0].reason.contains("publishing folder"));
    }
}
