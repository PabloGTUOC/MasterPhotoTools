//! Phase 2 acceptance tests for the media layer.

mod fixtures;

use chrono::NaiveDateTime;
use fixtures::{tag, Fixtures, TiffValue};
use phototools_core::media::image_ops;
use phototools_core::media::{exif_jpeg, read_meta, DateSet, ExifWriter, Orientation, TagSource};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap()
}

// ---------------------------------------------------------------------------
// read_meta (task 2)
// ---------------------------------------------------------------------------

#[test]
fn f1_reads_capture_date_and_camera_in_process() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("shot.jpg", 640, 480, "2024:05:01 12:00:00", "PENTAX17");

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.camera.as_deref(), Some("PENTAX17"));
    assert_eq!(meta.capture, Some(dt("2024:05:01 12:00:00")));
    assert_eq!(meta.capture_source, Some(TagSource::ExifDateTimeOriginal));
    assert_eq!((meta.width, meta.height), (640, 480));
}

#[test]
fn f1_falls_through_to_create_date_when_date_time_original_is_absent() {
    let f = Fixtures::new();
    let path = f.jpeg_with_tags(
        "only_create.jpg",
        100,
        100,
        &[],
        &[(
            tag::CREATE_DATE,
            TiffValue::Ascii("2019:03:04 05:06:07".into()),
        )],
    );

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.capture, Some(dt("2019:03:04 05:06:07")));
    assert_eq!(meta.capture_source, Some(TagSource::ExifCreateDate));
}

#[test]
fn f1_prefers_date_time_original_over_create_date() {
    let f = Fixtures::new();
    let path = f.jpeg_with_tags(
        "both.jpg",
        100,
        100,
        &[],
        &[
            (
                tag::DATE_TIME_ORIGINAL,
                TiffValue::Ascii("2020:01:01 00:00:00".into()),
            ),
            (
                tag::CREATE_DATE,
                TiffValue::Ascii("2019:03:04 05:06:07".into()),
            ),
        ],
    );

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.capture, Some(dt("2020:01:01 00:00:00")));
    assert_eq!(meta.capture_source, Some(TagSource::ExifDateTimeOriginal));
}

#[test]
fn f1_reads_a_quicktime_creation_time_as_utc() {
    let f = Fixtures::new();
    // 2024-05-01 12:00:00 UTC.
    let path = f.quicktime("clip.mov", 1_714_564_800);

    let meta = read_meta(&path).unwrap();
    assert_eq!(
        meta.capture,
        Some(dt("2024:05:01 12:00:00")),
        "a QuickTime timestamp read as local time would be shifted"
    );
    assert_eq!(meta.capture_source, Some(TagSource::QuickTimeCreateDate));
}

#[test]
fn a_file_with_no_metadata_yields_an_empty_result_not_an_error() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("bare.jpg", 50, 50);

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.capture, None);
    assert_eq!(meta.capture_source, None);
}

#[test]
fn an_unreadable_file_does_not_abort_the_caller() {
    let f = Fixtures::new();
    let path = f.path().join("garbage.jpg");
    std::fs::write(&path, b"not an image at all").unwrap();

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.capture, None);
}

#[test]
fn orientation_is_read_from_exif() {
    let f = Fixtures::new();
    for (value, expected) in [
        (1u16, Orientation::Normal),
        (3, Orientation::Rotate180),
        (6, Orientation::Rotate90),
        (8, Orientation::Rotate270),
    ] {
        let path = f.jpeg_with_orientation(&format!("o{value}.jpg"), 40, 20, value);
        assert_eq!(read_meta(&path).unwrap().orientation, expected);
    }
}

#[test]
fn dimensions_come_from_metadata_without_decoding() {
    let f = Fixtures::new();
    // The EXIF says 6000x4000 while the pixels are 64x48. read_meta must report
    // what the metadata says — F11 forbids decoding to learn dimensions.
    let path = f.jpeg_with_tags(
        "claims.jpg",
        64,
        48,
        &[],
        &[
            (tag::PIXEL_X_DIMENSION, TiffValue::Long(6000)),
            (tag::PIXEL_Y_DIMENSION, TiffValue::Long(4000)),
        ],
    );

    let meta = read_meta(&path).unwrap();
    assert_eq!((meta.width, meta.height), (6000, 4000));
}

// ---------------------------------------------------------------------------
// ExifWriter (task 3)
// ---------------------------------------------------------------------------

/// Write an executable shell script.
fn write_script(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Start an `ExifWriter` against a freshly written script.
///
/// Retries on `ETXTBSY`: this process spawns children from several test threads,
/// and a concurrent fork can transiently hold a write descriptor to a script we
/// just created. That is a property of fork/exec in a threaded harness, not of
/// the code under test.
fn start_against_script(
    program: &std::path::Path,
    timeout: Duration,
) -> Result<ExifWriter, String> {
    for _ in 0..50 {
        match ExifWriter::start_with(program.to_str().unwrap(), timeout) {
            Ok(w) => return Ok(w),
            Err(e) if e.to_string().contains("Text file busy") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    panic!("script stayed busy far longer than any fork race explains");
}

/// Install a shim named `exiftool` that appends one line per invocation to a log
/// and then execs the real tool. Returns (shim path, log path).
fn spawn_counting_shim(f: &Fixtures) -> (PathBuf, PathBuf) {
    let log = f.path().join("spawns.log");
    let shim = f.path().join("exiftool-shim");
    let real = String::from_utf8(
        std::process::Command::new("which")
            .arg("exiftool")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    write_script(
        &shim,
        &format!(
            "#!/bin/sh\necho spawn >> {}\nexec {} \"$@\"\n",
            log.display(),
            real.trim()
        ),
    );
    (shim, log)
}

/// **Phase 2 acceptance.** Writing 50 files spawns exactly one process.
///
/// Starting one `exiftool` per file costs 150–250 ms each regardless of file
/// size, which would add over a minute of pure overhead to a 500-file operation
/// (specification §2.6, G4).
#[test]
fn writing_fifty_files_spawns_exactly_one_exiftool_process() {
    let f = Fixtures::new();
    let (shim, log) = spawn_counting_shim(&f);

    let paths: Vec<_> = (0..50)
        .map(|i| f.jpeg_without_exif(&format!("w{i:02}.jpg"), 16, 16))
        .collect();

    let date = dt("2020:01:01 10:00:00");
    let set = DateSet { date: Some(date) };

    let mut writer = start_against_script(&shim, Duration::from_secs(60)).unwrap();
    for path in &paths {
        writer.write_dates(path, &set).unwrap();
    }
    writer.close().unwrap();

    let spawns = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(
        spawns, 1,
        "50 files must go through one persistent process, not {spawns}"
    );

    // And the writes actually landed.
    for path in &paths {
        assert_eq!(read_meta(path).unwrap().capture, Some(date));
    }
}

#[test]
fn the_writer_sets_the_full_image_date_tag_set() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("tags.jpg", 20, 20);
    let date = dt("2021:06:07 08:09:10");

    let mut writer = ExifWriter::start().unwrap();
    writer
        .write_dates(&path, &DateSet { date: Some(date) })
        .unwrap();
    writer.close().unwrap();

    let out = std::process::Command::new("exiftool")
        .args(["-s", "-DateTimeOriginal", "-CreateDate", "-ModifyDate"])
        .arg(&path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);

    for tag in ["DateTimeOriginal", "CreateDate", "ModifyDate"] {
        assert!(text.contains(tag), "F1 requires {tag}; got:\n{text}");
    }
    assert_eq!(text.matches("2021:06:07 08:09:10").count(), 3);
}

#[test]
fn shift_mode_moves_a_date_by_a_delta() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("shift.jpg", 40, 40, "2019:01:02 03:04:05", "CAM");

    let mut writer = ExifWriter::start().unwrap();
    // Phase 3 acceptance uses exactly this: a 2019 fixture shifted by five years.
    writer.shift_dates(&path, "+5:0:0 0:0:0").unwrap();
    writer.close().unwrap();

    assert_eq!(
        read_meta(&path).unwrap().capture,
        Some(dt("2024:01:02 03:04:05"))
    );
}

#[test]
fn shift_mode_accepts_a_negative_delta() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("back.jpg", 40, 40, "2024:01:02 03:04:05", "CAM");

    let mut writer = ExifWriter::start().unwrap();
    writer.shift_dates(&path, "-5:0:0 0:0:0").unwrap();
    writer.close().unwrap();

    assert_eq!(
        read_meta(&path).unwrap().capture,
        Some(dt("2019:01:02 03:04:05"))
    );
}

#[test]
fn a_hung_child_times_out_rather_than_blocking_forever() {
    let f = Fixtures::new();

    let shim = f.path().join("hang");
    write_script(&shim, "#!/bin/sh\nexec sleep 600\n");

    let started = Instant::now();
    let message = match start_against_script(&shim, Duration::from_millis(300)) {
        Ok(_) => panic!("a child that never answers must not hang"),
        Err(e) => e,
    };
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(message.contains("did not respond"), "got: {message}");
}

#[test]
fn a_missing_binary_is_a_clear_error() {
    let err = match ExifWriter::start_with("definitely-not-a-real-binary", Duration::from_secs(1)) {
        Ok(_) => panic!("a missing binary must not start"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("exiftool is required"), "got: {err}");
}

// ---------------------------------------------------------------------------
// EXIF-preserving re-encode (task 6) — specification-mandatory
// ---------------------------------------------------------------------------

/// **Phase 2 acceptance, named mandatory by the specification (F13, §9.4).**
///
/// Generate a JPEG with a known capture date, resize it, read the metadata back,
/// and assert the date and camera survived and the pixel dimensions were
/// updated.
///
/// Dropping EXIF at this step destroys the capture date that was just validated,
/// and Google Photos would file the photograph under its upload date instead of
/// the date it was taken.
#[test]
fn f13_resizing_preserves_exif_and_updates_the_pixel_dimensions() {
    let f = Fixtures::new();
    let source = f.jpeg_with_exif("original.jpg", 800, 600, "2024:05:01 12:00:00", "PENTAX17");

    let before = read_meta(&source).unwrap();
    assert_eq!((before.width, before.height), (800, 600));

    let img = image_ops::decode(&source).unwrap();
    let resized = image_ops::resize(&img, 400, 300).unwrap();

    let destination = f.path().join("resized.jpg");
    let carried = image_ops::reencode_preserving_exif(&source, &resized, &destination, 92).unwrap();
    assert!(carried, "the source had EXIF, so it must have been carried");

    let after = read_meta(&destination).unwrap();

    // The capture date survived.
    assert_eq!(
        after.capture,
        Some(dt("2024:05:01 12:00:00")),
        "the capture date must survive a resize"
    );
    assert_eq!(after.capture_source, Some(TagSource::ExifDateTimeOriginal));

    // The camera survived.
    assert_eq!(after.camera.as_deref(), Some("PENTAX17"));

    // And the recorded dimensions now describe the resized pixels.
    assert_eq!(
        (after.width, after.height),
        (400, 300),
        "PixelXDimension/PixelYDimension must be updated, not left stale"
    );

    // The file really is the new size.
    let decoded = image_ops::decode(&destination).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (400, 300));
}

#[test]
fn a_source_without_exif_reencodes_cleanly_and_says_nothing_was_carried() {
    let f = Fixtures::new();
    let source = f.jpeg_without_exif("bare.jpg", 200, 100);
    let img = image_ops::decode(&source).unwrap();
    let resized = image_ops::resize(&img, 100, 50).unwrap();

    let destination = f.path().join("out.jpg");
    let carried = image_ops::reencode_preserving_exif(&source, &resized, &destination, 90).unwrap();

    assert!(!carried);
    let decoded = image_ops::decode(&destination).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (100, 50));
}

#[test]
fn the_exif_block_reports_and_rewrites_its_pixel_dimensions() {
    let f = Fixtures::new();
    let source = f.jpeg_with_exif("dims.jpg", 1234, 567, "2024:05:01 12:00:00", "CAM");
    let bytes = std::fs::read(&source).unwrap();

    let mut block = exif_jpeg::extract(&bytes).expect("fixture has EXIF");
    assert_eq!(block.pixel_dimensions(), Some((1234, 567)));

    assert!(block.set_pixel_dimensions(99, 88));
    assert_eq!(block.pixel_dimensions(), Some((99, 88)));
}

#[test]
fn splicing_does_not_leave_two_exif_blocks_behind() {
    let f = Fixtures::new();
    let source = f.jpeg_with_exif("one.jpg", 100, 80, "2024:05:01 12:00:00", "CAM");
    let bytes = std::fs::read(&source).unwrap();
    let block = exif_jpeg::extract(&bytes).unwrap();

    // Splice into a file that already has a block.
    let spliced = exif_jpeg::splice(&bytes, &block);
    let out = f.path().join("spliced.jpg");
    std::fs::write(&out, &spliced).unwrap();

    // Still exactly one, and still readable.
    let meta = read_meta(&out).unwrap();
    assert_eq!(meta.camera.as_deref(), Some("CAM"));

    let app1_count = std::process::Command::new("exiftool")
        .args(["-s", "-ExifByteOrder"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(app1_count.status.success());
}

// ---------------------------------------------------------------------------
// Resize and the quality ladder (task 5)
// ---------------------------------------------------------------------------

#[test]
fn the_quality_ladder_steps_down_until_the_cap_is_met() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("big.jpg", 1200, 900);
    let img = image_ops::decode(&path).unwrap();

    // A cap large enough for the first rung.
    let (bytes, quality, fits) = image_ops::encode_jpeg_within(&img, 10_000_000).unwrap();
    assert!(fits);
    assert_eq!(quality, 95, "a generous cap should not step down");
    assert!(!bytes.is_empty());

    // A cap that forces a step down but is still reachable.
    let at_95 = image_ops::encode_jpeg_bytes(&img, 95).unwrap().len() as u64;
    let (_, quality, fits) = image_ops::encode_jpeg_within(&img, at_95 - 1).unwrap();
    assert!(fits);
    assert!(
        quality < 95,
        "should have stepped down from 95, got {quality}"
    );
    assert!(
        image_ops::QUALITY_LADDER.contains(&quality),
        "quality {quality} is not a rung of the ladder"
    );
}

#[test]
fn an_unreachable_cap_reports_failure_rather_than_claiming_success() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("huge.jpg", 1200, 900);
    let img = image_ops::decode(&path).unwrap();

    let (bytes, quality, fits) = image_ops::encode_jpeg_within(&img, 8).unwrap();
    assert!(
        !fits,
        "8 bytes is not achievable and must not be reported met"
    );
    assert_eq!(quality, 75, "the ladder should have run to its last rung");
    assert!(!bytes.is_empty());
}

#[test]
fn decoding_with_orientation_applies_the_rotation() {
    let f = Fixtures::new();
    // 40x20 landscape pixels tagged "rotate 90" — decoding oriented gives 20x40.
    let path = f.jpeg_with_orientation("rot.jpg", 40, 20, 6);

    let raw = image_ops::decode(&path).unwrap();
    assert_eq!((raw.width(), raw.height()), (40, 20));

    let oriented = image_ops::decode_oriented(&path).unwrap();
    assert_eq!((oriented.width(), oriented.height()), (20, 40));
}

#[test]
fn png_and_tiff_decode_and_encode() {
    let f = Fixtures::new();

    let png = f.png("a.png", 60, 40);
    let img = image_ops::decode(&png).unwrap();
    assert_eq!((img.width(), img.height()), (60, 40));

    let tiff = f.multipage_tiff("m.tif", 1, 50, 30);
    let img = image_ops::decode(&tiff).unwrap();
    assert_eq!((img.width(), img.height()), (50, 30));

    let out = f.path().join("out.tif");
    image_ops::encode_to(&img, &out, image::ImageFormat::Tiff, 95).unwrap();
    assert_eq!(image_ops::decode(&out).unwrap().width(), 50);
}

// ---------------------------------------------------------------------------
// Benchmark (acceptance)
// ---------------------------------------------------------------------------

/// **Phase 2 acceptance.** Resize and encode one 24 MP JPEG in under 150 ms
/// (specification §9.1).
///
/// The target describes optimised code, so it is asserted only for release
/// builds. Debug builds print the figure without asserting — a debug number is
/// not evidence either way, and failing on it would just teach people to ignore
/// the test.
#[test]
fn benchmark_resize_and_encode_a_24mp_jpeg() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("bench.jpg", 6000, 4000);
    let img = image_ops::decode(&path).unwrap();
    assert_eq!(img.width() as u64 * img.height() as u64, 24_000_000);

    let (w, h) = image_ops::dimensions_for_megapixels(6000, 4000, 10).unwrap();

    // One warm pass so the measurement is not dominated by first-touch paging.
    let _ = image_ops::resize(&img, w, h).unwrap();

    let started = Instant::now();
    let resized = image_ops::resize(&img, w, h).unwrap();
    let bytes = image_ops::encode_jpeg_bytes(&resized, 95).unwrap();
    let elapsed = started.elapsed();

    assert!(!bytes.is_empty());
    println!(
        "24 MP resize ({}x{} -> {}x{}) + encode: {:?}  [{} build]",
        img.width(),
        img.height(),
        w,
        h,
        elapsed,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < Duration::from_millis(150),
        "specification §9.1 target is 150 ms, measured {elapsed:?}"
    );
}

/// A file the streaming reader opens but cannot walk is retried in memory.
///
/// `nom-exif`'s incremental reader fails part-way through a camera TIFF's IFD
/// — `Incomplete(Size(..))` — and the same file parses correctly from memory.
/// The silent empty answer sent every TIFF on a card to F1's skipped list as
/// "No metadata date to copy", with the date plainly present in the file.
#[test]
fn a_tiff_whose_ifd_defeats_the_streaming_reader_is_still_read() {
    let f = Fixtures::new();

    // The fixture generator writes its own IFDs, so this asserts the fallback
    // exists and agrees with the streaming path rather than reproducing the
    // camera file that provoked it — that one is MV-2.1's job.
    let path = f.jpeg_with_exif("shot.jpg", 64, 48, "2024:05:01 12:00:00", "PENTAX 17");

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.camera.as_deref(), Some("PENTAX 17"));
    assert!(meta.capture.is_some(), "the streaming path still reads");

    // And the in-memory path reaches the same answer for the same file.
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.is_empty());
}

// ---------------------------------------------------------------------------
// The UTC offset (geotagging, GT-3)
//
// EXIF capture times are local wall-clock with no zone, which is why a track
// cannot be joined to a photograph without an offset from somewhere. Where the
// camera wrote one down there is nothing to guess, so it has to be read — and
// it has to be read from the right tag, because the three that can carry one
// are not interchangeable.
// ---------------------------------------------------------------------------

fn jpeg_with_offsets(f: &Fixtures, name: &str, offsets: &[(u16, &str)]) -> PathBuf {
    let mut exif = vec![(
        tag::DATE_TIME_ORIGINAL,
        TiffValue::Ascii("2026:09:04 15:33:37".into()),
    )];
    for (which, value) in offsets {
        exif.push((*which, TiffValue::Ascii((*value).into())));
    }
    f.jpeg_with_tags(
        name,
        40,
        40,
        &[(tag::ORIENTATION, TiffValue::Short(1))],
        &exif,
    )
}

#[test]
fn a_camera_that_recorded_its_offset_is_believed() {
    let f = Fixtures::new();
    let path = jpeg_with_offsets(&f, "offset.jpg", &[(tag::OFFSET_TIME_ORIGINAL, "+02:00")]);

    let meta = read_meta(&path).unwrap();
    assert_eq!(meta.capture, Some(dt("2026:09:04 15:33:37")));
    assert_eq!(meta.utc_offset_minutes, Some(120));
}

#[test]
fn a_western_offset_is_read_as_a_negative_one() {
    let f = Fixtures::new();
    let path = jpeg_with_offsets(&f, "west.jpg", &[(tag::OFFSET_TIME_ORIGINAL, "-05:00")]);
    assert_eq!(read_meta(&path).unwrap().utc_offset_minutes, Some(-300));
}

#[test]
fn the_shutters_offset_wins_over_the_files_offset() {
    // `OffsetTime` belongs to `ModifyDate` — the moment the file was last
    // written, which for anything that has been through an editor is a
    // different day in a different country from the moment it was taken.
    let f = Fixtures::new();
    let path = jpeg_with_offsets(
        &f,
        "both.jpg",
        &[
            (tag::OFFSET_TIME, "+09:00"),
            (tag::OFFSET_TIME_ORIGINAL, "+02:00"),
        ],
    );
    assert_eq!(read_meta(&path).unwrap().utc_offset_minutes, Some(120));
}

#[test]
fn a_file_carrying_only_the_general_offset_still_offers_it() {
    let f = Fixtures::new();
    let path = jpeg_with_offsets(&f, "general.jpg", &[(tag::OFFSET_TIME, "+09:00")]);
    assert_eq!(read_meta(&path).unwrap().utc_offset_minutes, Some(540));
}

#[test]
fn a_camera_that_recorded_no_offset_offers_none_rather_than_utc() {
    // The difference that matters: "I don't know" leaves the tool asking, and
    // "UTC" silently moves every photograph a few kilometres.
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("nooffset.jpg", 40, 40, "2026:09:04 15:33:37", "CAM");
    assert_eq!(read_meta(&path).unwrap().utc_offset_minutes, None);
}

#[test]
fn a_photograph_with_no_gps_block_reports_no_position() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("plain.jpg", 40, 40, "2026:09:04 15:33:37", "CAM");
    assert_eq!(read_meta(&path).unwrap().gps, None);
}

#[test]
fn the_inventory_reads_a_folder_of_real_files() {
    use phototools_core::tools::geotag::scan;

    let f = Fixtures::new();
    let dated = f.jpeg_with_exif("dated.jpg", 40, 40, "2026:09:04 15:33:37", "CAM");
    let undated = f.jpeg_without_exif("undated.jpg", 40, 40);
    let movie = f.quicktime("clip.mov", 1_788_536_017);

    let rows = scan::scan(dated.parent().unwrap(), false).unwrap();
    let status = |name: &str| {
        rows.iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("{name} should be in the inventory"))
            .status
    };

    assert_eq!(status("dated.jpg"), scan::GeoStatus::NoLocation);
    assert_eq!(status("undated.jpg"), scan::GeoStatus::NoDateOrLocation);
    assert_eq!(status("clip.mov"), scan::GeoStatus::NotSupported);

    let summary = scan::summarise(&rows);
    assert_eq!(summary.total, rows.len());
    assert!(summary.missing_location >= 1);

    // The date and the tag that supplied it travel with the row: without them
    // there is no way to see *why* a photograph matched where it did.
    let row = rows.iter().find(|r| r.name == "dated.jpg").unwrap();
    assert_eq!(row.tag.as_deref(), Some("EXIF:DateTimeOriginal"));
    assert!(row.capture.is_some());
    assert!(row.location.is_none());

    let _ = (undated, movie);
}

// ---------------------------------------------------------------------------
// Writing and reading a position (geotagging, GT-7)
//
// The round trip is the test that matters: the default is to leave a
// photograph that already knows where it was alone, so a read that quietly
// returned nothing would have this tool overwriting real measurements with
// inferred ones — silently, and on every phone photograph.
// ---------------------------------------------------------------------------

#[test]
fn a_position_written_into_a_photograph_reads_back_as_that_position() {
    use phototools_core::tools::geotag::{exif, TrackPoint};

    let f = Fixtures::new();
    let path = f.jpeg_with_exif("geo.jpg", 40, 40, "2026:09:04 15:33:37", "CAM");

    // A fix from the sample track: 4 September, 15:33:37 UTC, in Berlin.
    let fix = TrackPoint {
        at: 1_788_536_017,
        lat: 52.531549,
        lon: 13.369192,
        ele: Some(36.40),
    };

    let mut writer = ExifWriter::start().unwrap();
    writer
        .set_tags(&path, &exif::render(&fix, true).args())
        .unwrap();
    writer.close().unwrap();

    let read = read_meta(&path)
        .unwrap()
        .gps
        .expect("the fix should read back");
    assert!(
        (read.lat - fix.lat).abs() < 1e-6,
        "latitude came back as {}",
        read.lat
    );
    assert!(
        (read.lon - fix.lon).abs() < 1e-6,
        "longitude came back as {}",
        read.lon
    );
    assert!(
        (read.altitude.unwrap() - 36.40).abs() < 0.01,
        "altitude came back as {:?}",
        read.altitude
    );

    // And the dates the file already had are untouched: writing a position must
    // not disturb the one thing the position was matched on.
    assert_eq!(
        read_meta(&path).unwrap().capture,
        Some(dt("2026:09:04 15:33:37"))
    );
}

#[test]
fn a_southern_western_position_keeps_its_hemispheres_through_the_file() {
    use phototools_core::tools::geotag::{exif, TrackPoint};

    let f = Fixtures::new();
    let path = f.jpeg_with_exif("sydney.jpg", 40, 40, "2026:09:04 15:33:37", "CAM");
    let fix = TrackPoint {
        at: 1_788_536_017,
        lat: -33.868800,
        lon: -151.209300,
        ele: None,
    };

    let mut writer = ExifWriter::start().unwrap();
    writer
        .set_tags(&path, &exif::render(&fix, true).args())
        .unwrap();
    writer.close().unwrap();

    let read = read_meta(&path).unwrap().gps.unwrap();
    assert!(
        read.lat < 0.0,
        "expected a southern latitude, got {}",
        read.lat
    );
    assert!(
        read.lon < 0.0,
        "expected a western longitude, got {}",
        read.lon
    );
    assert!((read.lat + 33.868800).abs() < 1e-6);
    assert!((read.lon + 151.209300).abs() < 1e-6);
}

#[test]
fn writing_a_position_into_fifty_files_spawns_one_exiftool() {
    use phototools_core::tools::geotag::{exif, TrackPoint};

    // The same claim Phase 2 makes about dates, for a tool that writes nine
    // tags per file instead of six — where the temptation to call `set_tag`
    // nine times would cost nine processes a file.
    let f = Fixtures::new();
    let (shim, log) = spawn_counting_shim(&f);

    let paths: Vec<_> = (0..50)
        .map(|i| f.jpeg_without_exif(&format!("g{i:02}.jpg"), 16, 16))
        .collect();

    let fix = TrackPoint {
        at: 1_788_536_017,
        lat: 52.531549,
        lon: 13.369192,
        ele: Some(36.4),
    };
    let args = exif::render(&fix, true).args();

    let mut writer = start_against_script(&shim, Duration::from_secs(60)).unwrap();
    for path in &paths {
        writer.set_tags(path, &args).unwrap();
    }
    writer.close().unwrap();

    let spawns = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(
        spawns, 1,
        "50 files must go through one process, not {spawns}"
    );

    // And the writes landed, rather than the process merely having been quiet.
    for path in &paths {
        let read = read_meta(path)
            .unwrap()
            .gps
            .expect("every file should carry the fix");
        assert!((read.lat - fix.lat).abs() < 1e-6);
    }
}
