mod fixtures;

use fixtures::{Fixtures, ShotKind};
use phototools_core::ingest::{group_assets, ingest_card, Scanner};
use phototools_core::ledger::Ledger;

#[test]
fn a_raw_plus_jpeg_pair_groups_into_one_shot() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::RawPlusJpeg, ShotKind::RawPlusJpeg]);

    let assets = Scanner::scan(&root).unwrap();
    let dcim: Vec<_> = assets
        .iter()
        .filter(|a| a.path.to_string_lossy().contains("DCIM"))
        .cloned()
        .collect();
    assert_eq!(dcim.len(), 4, "two pairs is four files");

    let shots = group_assets(dcim);
    assert_eq!(shots.len(), 2, "a pair sharing a stem is one shot");
    for shot in &shots {
        assert_eq!(shot.assets.len(), 2, "shot {} should hold both", shot.id);
    }
}

#[test]
fn a_mixed_card_groups_by_stem() {
    let f = Fixtures::new();
    let root = f.card_tree(&[
        ShotKind::RawPlusJpeg,
        ShotKind::JpegOnly,
        ShotKind::RawOnly,
        ShotKind::RawPlusJpeg,
    ]);

    let assets: Vec<_> = Scanner::scan(&root)
        .unwrap()
        .into_iter()
        .filter(|a| a.path.to_string_lossy().contains("DCIM"))
        .collect();

    // 2 pairs + 1 jpeg + 1 raw = 6 files, 4 shots.
    assert_eq!(assets.len(), 6);
    assert_eq!(group_assets(assets).len(), 4);
}

#[test]
fn dot_files_are_ignored_by_a_scan() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::JpegOnly]);

    let assets = Scanner::scan(&root).unwrap();
    assert!(
        !assets
            .iter()
            .any(|a| a.path.file_name().unwrap() == ".DS_Store"),
        "junk a card carries must not be scanned"
    );
}

#[test]
fn identical_bytes_hash_identically() {
    let f = Fixtures::new();

    // Scan a named subdirectory, not the tempdir itself: the scanner prunes any
    // entry whose name starts with a dot, and tempfile names its directories
    // `.tmpXXXXXX`.
    let dir = f.path().join("shots");
    std::fs::create_dir(&dir).unwrap();

    let a = f.jpeg_without_exif("a.jpg", 32, 32);
    std::fs::rename(&a, dir.join("a.jpg")).unwrap();
    std::fs::copy(dir.join("a.jpg"), dir.join("b.jpg")).unwrap();

    let assets = Scanner::scan(&dir).unwrap();
    assert_eq!(assets.len(), 2);
    assert_eq!(
        assets[0].sha256, assets[1].sha256,
        "sha256 is the deduplication key (F16)"
    );
}

#[test]
fn an_ingest_run_persists_cards_shots_and_assets() {
    let f = Fixtures::new();
    let root = f.card_tree(&[ShotKind::RawPlusJpeg, ShotKind::JpegOnly]);
    let ledger = Ledger::open_in_memory().unwrap();

    ingest_card(&root, &ledger).unwrap();
    let conn = ledger.inner();

    let cards: i64 = conn
        .query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cards, 1);

    let shots: i64 = conn
        .query_row("SELECT COUNT(*) FROM shots", [], |r| r.get(0))
        .unwrap();
    let assets: i64 = conn
        .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))
        .unwrap();
    assert!(shots > 0 && assets >= shots);
}
