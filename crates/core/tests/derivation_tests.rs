mod fixtures;

use phototools_core::ingest::derivation::{DeriveJob, WorkerPool};
use phototools_core::ingest::{CandidateAsset, CandidateShot};
use phototools_core::ledger::Ledger;
use phototools_core::media::meta::read_meta;

#[test]
fn test_derivation_pipeline() {
    let f = fixtures::Fixtures::new();
    let capture_date = "2024:01:01 12:00:00";
    let camera = "DeriveCam";

    // Setup a 4000x3000 image, which will be downscaled to 2000x1500
    let img_path = f.jpeg_with_exif("original.jpg", 4000, 3000, capture_date, camera);

    // Mock asset
    let asset = CandidateAsset {
        path: img_path.clone(),
        sha256: "fake_hash".to_string(),
        bytes: 1000,
    };

    let shot = CandidateShot {
        id: "original".to_string(),
        assets: vec![asset.clone()],
    };

    let staging_dir = f.temp.path().join("staging");

    let job = DeriveJob {
        shot_id: "test_shot_id".to_string(),
        shot,
        primary_asset: asset,
        staging_dir: staging_dir.clone(),
    };

    let ledger = Ledger::open_in_memory().unwrap();

    // We only create 1 thread to test safely
    let pool = WorkerPool::new(1).unwrap();
    pool.process_batch(vec![job], &ledger).unwrap();

    let conn = ledger.inner();

    // Check DB
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM derived", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);

    let (staged_path, width, height): (String, u32, u32) = conn
        .query_row(
            "SELECT staged_path, width, height FROM derived WHERE shot_id = 'test_shot_id'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    // 4000x3000 -> 2000x1500
    assert_eq!(width, 2000);
    assert_eq!(height, 1500);

    // Verify file actually exists and contains EXIF
    let path = std::path::PathBuf::from(staged_path);
    assert!(path.exists());

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.camera.as_deref(), Some(camera));
}
