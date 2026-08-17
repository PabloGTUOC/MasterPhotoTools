mod fixtures;

use chrono::NaiveDateTime;
use phototools_core::media::{read_meta, DateSet, ExifWriter, TagSource};

#[test]
fn test_exif_read_roundtrip() {
    let f = fixtures::Fixtures::new();
    let path = f.jpeg_with_exif("test1.jpg", 100, 100, "2024:05:01 12:00:00", "PENTAX17");

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.camera.as_deref(), Some("PENTAX17"));
    assert_eq!(
        meta.capture,
        Some(NaiveDateTime::parse_from_str("2024:05:01 12:00:00", "%Y:%m:%d %H:%M:%S").unwrap())
    );
    assert_eq!(meta.capture_source, Some(TagSource::DateTimeOriginal));
}

#[test]
fn test_exif_writer_single_process() {
    let f = fixtures::Fixtures::new();
    let mut writer = ExifWriter::start().unwrap();

    let paths: Vec<_> = (0..5)
        .map(|i| f.jpeg_without_exif(&format!("test_writer_{}.jpg", i), 10, 10))
        .collect();

    let date = NaiveDateTime::parse_from_str("2020:01:01 10:00:00", "%Y:%m:%d %H:%M:%S").unwrap();
    let set = DateSet { date: Some(date) };

    for path in &paths {
        writer.write_dates(path, &set).unwrap();
    }

    // Drop writer to trigger shutdown
    drop(writer);

    // Verify
    for path in &paths {
        let meta = read_meta(path).unwrap();
        assert_eq!(meta.capture, Some(date));
    }
}

#[test]
fn test_benchmark_24mp_resize() {
    let f = fixtures::Fixtures::new();
    let path = f.jpeg_without_exif("bench_24mp.jpg", 6000, 4000);
    let img = phototools_core::media::image_ops::decode(&path).unwrap();
    let start = std::time::Instant::now();
    let resized = phototools_core::media::image_ops::resize(&img, 1000, 1000).unwrap();
    let duration = start.elapsed();
    println!("Resizing 24MP image took: {:?}", duration);
    assert_eq!(resized.width(), 1000);
}
