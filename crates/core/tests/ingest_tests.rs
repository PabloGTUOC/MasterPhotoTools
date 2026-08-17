//! Phase 8 acceptance: card scan and shot pairing (F10, F11).

mod fixtures;

use fixtures::{Fixtures, ShotKind};
use phototools_core::ingest::{
    group_into_shots, record_scan, scan_card, scan_files, stage_all, AssetKind, Card, Fingerprint,
    Origin,
};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::ledger::Ledger;
use std::path::Path;

fn progress() -> InMemoryProgress {
    InMemoryProgress::new()
}

/// The acceptance card: 60 RAW+JPEG pairs, 30 JPEG-only, 10 RAW-only.
fn hundred_shots() -> Vec<ShotKind> {
    let mut kinds = vec![ShotKind::RawPlusJpeg; 60];
    kinds.extend(vec![ShotKind::JpegOnly; 30]);
    kinds.extend(vec![ShotKind::RawOnly; 10]);
    kinds
}

// ---------------------------------------------------------------------------
// Acceptance 1 — 100 shots, correct candidates
// ---------------------------------------------------------------------------

#[test]
fn a_hundred_shot_card_groups_into_exactly_a_hundred_shots() {
    let f = Fixtures::new();
    let root = f.card_tree(&hundred_shots());

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();

    assert_eq!(scan.shot_count(), 100, "60 pairs + 30 JPEG + 10 RAW");
    assert_eq!(
        scan.assets().count(),
        160,
        "60 pairs are two assets each, the other 40 one each"
    );
    assert!(scan.problems.is_empty(), "{:?}", scan.problems);
}

#[test]
fn every_shot_picks_the_candidate_f11_requires() {
    let f = Fixtures::new();
    let root = f.card_tree(&hundred_shots());

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();

    let mut jpeg_candidates = 0;
    let mut raw_candidates = 0;

    for shot in &scan.shots {
        match shot.candidate().kind {
            AssetKind::Jpeg => {
                jpeg_candidates += 1;
                assert!(
                    !shot.needs_derivation,
                    "{} has a JPEG, nothing to derive",
                    shot.stem
                );
            }
            AssetKind::Raw => {
                raw_candidates += 1;
                assert!(
                    shot.needs_derivation,
                    "{} is RAW only, F14 must produce its candidate",
                    shot.stem
                );
                assert_eq!(shot.assets.len(), 1, "a RAW-only shot has one asset");
            }
            other => panic!("unexpected candidate kind {other:?} on {}", shot.stem),
        }
    }

    // F11: "JPEG present → the JPEG is the candidate."
    assert_eq!(jpeg_candidates, 90, "60 pairs plus 30 JPEG-only");
    assert_eq!(raw_candidates, 10);
    assert_eq!(scan.awaiting_derivation(), 10);
}

#[test]
fn a_pairs_raw_is_recorded_but_is_not_the_candidate() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::RawPlusJpeg]);

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();
    let shot = &scan.shots[0];

    assert_eq!(shot.assets.len(), 2, "the RAW is recorded");
    assert_eq!(shot.candidate().kind, AssetKind::Jpeg);
    assert!(shot.assets.iter().any(|a| a.kind == AssetKind::Raw));
}

// ---------------------------------------------------------------------------
// Acceptance 2 — dimensions from metadata, never by decoding
// ---------------------------------------------------------------------------

#[test]
fn dimensions_come_from_metadata_not_from_decoding() {
    // F11: "Pixel dimensions come from EXIF metadata, never by decoding the
    // image." The fixture's metadata says 6000x4000; its pixel data is
    // undecodable. Anything that decoded to find the size would fail here.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    let broken = f.jpeg_with_unreadable_pixels("broken.jpg", 6000, 4000);
    std::fs::rename(&broken, dir.join("IMG_0001.JPG")).unwrap();

    // The premise: this file genuinely cannot be decoded. Without this the test
    // would pass on a file that simply happened to decode.
    assert!(
        phototools_core::media::decode(&dir.join("IMG_0001.JPG")).is_err(),
        "the fixture must be undecodable, or this test proves nothing"
    );

    let scan = scan_card(&Card::at(&dir).unwrap(), &progress()).unwrap();

    assert_eq!(scan.shot_count(), 1);
    let asset = scan.shots[0].candidate();
    assert_eq!(
        (asset.width, asset.height),
        (6000, 4000),
        "the scan read the dimensions from metadata"
    );
    assert!(
        scan.problems.is_empty(),
        "and did not treat it as a problem"
    );
}

#[test]
fn a_file_whose_metadata_carries_no_dimensions_is_recorded_as_unknown() {
    // The honest answer when the card does not say. Resolving it by decoding is
    // exactly what F11 forbids.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();
    let plain = f.jpeg_without_exif("plain.jpg", 64, 48);
    std::fs::rename(&plain, dir.join("IMG_0001.JPG")).unwrap();

    let scan = scan_card(&Card::at(&dir).unwrap(), &progress()).unwrap();

    assert_eq!(scan.shot_count(), 1);
    assert!(scan.shots[0].candidate().dimensions_unknown());
}

// ---------------------------------------------------------------------------
// Acceptance 3 — the card is never written to (G5)
// ---------------------------------------------------------------------------

/// Every file under `root`, with its size and contents hash, for comparison.
fn snapshot(root: &Path) -> Vec<(String, u64, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .to_string();
            let bytes = std::fs::read(entry.path()).unwrap();
            out.push((rel, bytes.len() as u64, bytes));
        }
    }
    out
}

#[test]
fn a_read_only_card_scans_and_is_byte_identical_afterwards() {
    // G5 and F11's stated invariant. The card is made read-only so that a write
    // fails rather than passing unnoticed, and compared byte for byte after.
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::RawPlusJpeg, ShotKind::JpegOnly, ShotKind::RawOnly]);

    let before = snapshot(&root);
    assert!(
        !before.is_empty(),
        "the fixture must have written something"
    );

    #[cfg(unix)]
    let restore = {
        use std::os::unix::fs::PermissionsExt;
        let mut dirs = Vec::new();
        for entry in walkdir::WalkDir::new(&root) {
            let entry = entry.unwrap();
            if entry.file_type().is_dir() {
                dirs.push(entry.path().to_path_buf());
            }
        }
        // Deepest first, so a parent is still writable while its children change.
        dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for dir in &dirs {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        }
        dirs
    };

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();

    // Staging reads from the card and writes only to the staging directory.
    let staging = f.path().join("staging");
    let candidates: Vec<_> = scan.candidates().cloned().collect();
    let staged = stage_all(&candidates, &staging, &progress());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in restore.iter().rev() {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    assert_eq!(scan.shot_count(), 3);
    assert!(staged.all_verified(), "{:?}", staged.failed);
    assert_eq!(staged.staged.len(), 3);

    assert_eq!(
        snapshot(&root),
        before,
        "the card must be byte-identical after a scan and a staging copy"
    );
}

#[test]
fn staged_copies_are_verified_against_the_hash_the_scan_computed() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::JpegOnly, ShotKind::JpegOnly]);
    let staging = f.path().join("staging");

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();
    let candidates: Vec<_> = scan.candidates().cloned().collect();
    let result = stage_all(&candidates, &staging, &progress());

    assert!(result.all_verified());
    for staged in &result.staged {
        let on_disk = std::fs::read(&staged.staged).unwrap();
        let from_card = std::fs::read(&staged.source).unwrap();
        assert_eq!(on_disk, from_card, "the copy is the photograph");
    }
}

// ---------------------------------------------------------------------------
// Acceptance 4 — performance
// ---------------------------------------------------------------------------

#[test]
fn a_four_hundred_shot_card_scans_within_the_budget() {
    // §9.1: a 400-shot card scans in under 10 seconds. The figure is recorded in
    // the phase report.
    let f = Fixtures::new();
    let mut kinds = vec![ShotKind::RawPlusJpeg; 240];
    kinds.extend(vec![ShotKind::JpegOnly; 120]);
    kinds.extend(vec![ShotKind::RawOnly; 40]);
    let root = f.card_tree(&kinds);

    let card = Card::at(&root).unwrap();
    let started = std::time::Instant::now();
    let scan = scan_card(&card, &progress()).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(scan.shot_count(), 400);
    println!("400-shot scan: {:?}", elapsed);
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "§9.1: a 400-shot scan must finish within 10s, took {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// Simulated card mode (build plan §6.3)
// ---------------------------------------------------------------------------

#[test]
fn a_plain_directory_scans_exactly_like_a_card() {
    // §6.3: "A configuration option or CLI flag points the scan at an arbitrary
    // path, and the pipeline behaves exactly as if a card were mounted there."
    // The proof is that the two scans agree on everything but the label.
    let f = Fixtures::new();

    let with_dcim = f.card_tree_named("EOS_DIGITAL", &[ShotKind::RawPlusJpeg, ShotKind::JpegOnly]);

    // The same files, copied out of DCIM into a flat folder — §6.3's second
    // reason: re-running ingest over already-copied files.
    let flat = f.path().join("EOS_DIGITAL_copied");
    std::fs::create_dir_all(&flat).unwrap();
    for entry in walkdir::WalkDir::new(with_dcim.join("DCIM")) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            std::fs::copy(entry.path(), flat.join(entry.file_name())).unwrap();
        }
    }

    let card = scan_card(&Card::at(&with_dcim).unwrap(), &progress()).unwrap();
    let folder = scan_card(&Card::at(&flat).unwrap(), &progress()).unwrap();

    assert_eq!(card.shot_count(), folder.shot_count());
    assert_eq!(card.awaiting_derivation(), folder.awaiting_derivation());

    let card_stems: Vec<&str> = card.shots.iter().map(|s| s.stem.as_str()).collect();
    let folder_stems: Vec<&str> = folder.shots.iter().map(|s| s.stem.as_str()).collect();
    assert_eq!(card_stems, folder_stems);

    let card_hashes: Vec<&str> = card.candidates().map(|a| a.sha256.as_str()).collect();
    let folder_hashes: Vec<&str> = folder.candidates().map(|a| a.sha256.as_str()).collect();
    assert_eq!(
        card_hashes, folder_hashes,
        "the same photographs either way"
    );
}

#[test]
fn a_simulated_card_is_marked_as_simulated_but_behaves_no_differently() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::JpegOnly]);
    let card = Card::at(&root).unwrap();

    // Nothing in the pipeline branches on this; it is reported, not acted on.
    assert_eq!(card.origin(), Origin::Simulated);
    assert!(card.looks_like_a_card(), "the fixture has a DCIM tree");
    assert_eq!(
        scan_card(&card, &progress()).unwrap().shot_count(),
        1,
        "a simulated card scans like any other"
    );
}

// ---------------------------------------------------------------------------
// Card identity (F10)
// ---------------------------------------------------------------------------

#[test]
fn a_reinserted_card_is_recognised_as_one_already_seen() {
    // F10: identity is the volume label plus a fingerprint over the contents.
    let f = Fixtures::new();
    let root = f.card_tree_named("EOS_DIGITAL", &[ShotKind::RawPlusJpeg, ShotKind::JpegOnly]);

    let first = Fingerprint::of(&Card::at(&root).unwrap()).unwrap();
    let second = Fingerprint::of(&Card::at(&root).unwrap()).unwrap();

    assert_eq!(first.card_id(), second.card_id());
    assert_eq!(first.volume_label.as_deref(), Some("EOS_DIGITAL"));
}

#[test]
fn shooting_more_frames_makes_it_a_different_card_state() {
    let f = Fixtures::new();
    let root = f.card_tree_named("EOS_DIGITAL", &[ShotKind::JpegOnly]);
    let before = Fingerprint::of(&Card::at(&root).unwrap()).unwrap();

    let extra = f.jpeg_without_exif("extra.jpg", 32, 32);
    std::fs::rename(
        &extra,
        root.join("DCIM").join("100CANON").join("IMG_9999.JPG"),
    )
    .unwrap();
    let after = Fingerprint::of(&Card::at(&root).unwrap()).unwrap();

    assert_ne!(before.hash, after.hash);
}

// ---------------------------------------------------------------------------
// Junk, and the ledger
// ---------------------------------------------------------------------------

#[test]
fn card_junk_forms_no_shots() {
    // The fixture plants .DS_Store and MISC/AUTPRINT.MRK, which a real card
    // carries and which are not photographs.
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::JpegOnly]);

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();

    assert_eq!(scan.shot_count(), 1);
    assert!(
        !scan
            .assets()
            .any(|a| a.rel_path.contains("DS_Store") || a.rel_path.contains("AUTPRINT")),
        "junk must not reach the shot list"
    );
}

#[test]
fn identical_bytes_hash_identically() {
    let f = Fixtures::new();
    let dir = f.path().join("shots");
    std::fs::create_dir(&dir).unwrap();

    let a = f.jpeg_without_exif("a.jpg", 32, 32);
    std::fs::rename(&a, dir.join("IMG_0001.JPG")).unwrap();
    std::fs::copy(dir.join("IMG_0001.JPG"), dir.join("IMG_0002.JPG")).unwrap();

    let scanned = scan_files(&dir, &progress()).unwrap();
    assert_eq!(scanned.assets.len(), 2);
    assert_eq!(
        scanned.assets[0].sha256, scanned.assets[1].sha256,
        "sha256 is the deduplication key (F16)"
    );

    // Two files, two shots — the same photograph twice is still two frames on
    // the card. Deduplication is F16's job, not the scan's.
    assert_eq!(group_into_shots(scanned.assets).len(), 2);
}

#[test]
fn a_scan_persists_cards_shots_and_assets() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::RawPlusJpeg, ShotKind::JpegOnly, ShotKind::RawOnly]);
    let ledger = Ledger::open_in_memory().unwrap();

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();
    record_scan(&scan, &ledger).unwrap();

    assert_eq!(ledger.count("cards").unwrap(), 1);
    assert_eq!(ledger.count("shots").unwrap(), 3);
    assert_eq!(ledger.count("assets").unwrap(), 4);
    assert_eq!(ledger.shots_without_candidate().unwrap(), 0);
}

#[test]
fn rescanning_a_card_updates_rather_than_duplicates() {
    let f = Fixtures::new();
    let root = f.card_tree(&hundred_shots());
    let ledger = Ledger::open_in_memory().unwrap();

    let scan = scan_card(&Card::at(&root).unwrap(), &progress()).unwrap();
    record_scan(&scan, &ledger).unwrap();
    record_scan(&scan, &ledger).unwrap();

    assert_eq!(ledger.count("cards").unwrap(), 1);
    assert_eq!(ledger.count("shots").unwrap(), 100);
    assert_eq!(ledger.count("assets").unwrap(), 160);
}
