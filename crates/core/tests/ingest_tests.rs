mod fixtures;

use phototools_core::ingest::{group_assets, ingest_card, Scanner};
use phototools_core::ledger::Ledger;

#[test]
fn test_ingest_flow() {
    let f = fixtures::Fixtures::new();
    let root = f.card_tree(2);

    let assets = Scanner::scan(&root).unwrap();
    // 2 shots * 2 files (CR2 + JPG) + 1 dup = 5 assets.
    // The .DS_Store should be ignored.
    assert_eq!(assets.len(), 5);

    let shots = group_assets(assets.clone());
    // Stems: IMG_0000, IMG_0001, IMG_DUP -> 3 shots
    assert_eq!(shots.len(), 3);

    let dup = assets
        .iter()
        .find(|a| a.path.file_name().unwrap() == "IMG_DUP.JPG")
        .unwrap();
    let orig = assets
        .iter()
        .find(|a| a.path.file_name().unwrap() == "IMG_0000.JPG")
        .unwrap();
    assert_eq!(dup.sha256, orig.sha256);
}

#[test]
fn test_ledger_persistence() {
    let f = fixtures::Fixtures::new();
    let root = f.card_tree(2);
    let ledger = Ledger::open_in_memory().unwrap();

    ingest_card(&root, &ledger).unwrap();

    let conn = ledger.inner();

    // 1 card
    let cards_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cards", [], |row| row.get(0))
        .unwrap();
    assert_eq!(cards_count, 1);

    // 3 shots
    let shots_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM shots", [], |row| row.get(0))
        .unwrap();
    assert_eq!(shots_count, 3);

    // 5 assets
    let assets_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(assets_count, 5);
}
