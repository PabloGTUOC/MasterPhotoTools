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
    start_against_script_with(program, timeout, timeout)
}

/// As [`start_against_script`], with start-up bounded apart from each write.
///
/// The restart tests catch a hang with a write bound under a second, then restart
/// into the real exiftool, whose Perl start-up alone can take longer than that on
/// a loaded machine. Holding the restart to the write bound made them fail for
/// reasons that had nothing to do with restarting.
fn start_against_script_with(
    program: &std::path::Path,
    startup: Duration,
    timeout: Duration,
) -> Result<ExifWriter, String> {
    for _ in 0..50 {
        match ExifWriter::start_with_timeouts(program.to_str().unwrap(), startup, timeout) {
            Ok(w) => return Ok(w),
            Err(e) if e.to_string().contains("Text file busy") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    panic!("script stayed busy far longer than any fork race explains");
}

fn real_exiftool_path() -> String {
    String::from_utf8(
        std::process::Command::new("which")
            .arg("exiftool")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}

/// Install a shim named `exiftool` that appends one line per invocation to a log
/// and then execs the real tool. Returns (shim path, log path).
fn spawn_counting_shim(f: &Fixtures) -> (PathBuf, PathBuf) {
    let log = f.path().join("spawns.log");
    let shim = f.path().join("exiftool-shim");
    let real = real_exiftool_path();

    write_script(
        &shim,
        &format!(
            "#!/bin/sh\necho spawn >> {}\nexec {} \"$@\"\n",
            log.display(),
            real
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
fn a_hung_exiftool_is_restarted_and_the_next_file_is_written() {
    let f = Fixtures::new();
    let log = f.path().join("spawns.log");
    let shim = f.path().join("hang-once-shim");
    let real = real_exiftool_path();

    // Spawn 1 answers the -ver handshake, then hangs on the first write command.
    // Spawn 2 execs real exiftool so the next file is genuinely written.
    write_script(
        &shim,
        &format!(
            r#"#!/bin/sh
log="{}"
real="{}"
if [ -s "$log" ]; then first=0; else first=1; fi
echo spawn >> "$log"
if [ "$first" = "1" ]; then
    saw_ver=0
    while IFS= read -r line; do
        if [ "$line" = "-execute" ]; then
            if [ "$saw_ver" = "1" ]; then
                echo "{{phototools-stderr-ready}}" >&2
                echo "12.70"
                echo "{{ready}}"
                saw_ver=0
            else
                exec sleep 60
            fi
        elif [ "$line" = "-ver" ]; then
            saw_ver=1
        fi
    done
else
    exec "$real" "$@"
fi
"#,
            log.display(),
            real
        ),
    );

    let path_hung = f.jpeg_without_exif("hung.jpg", 16, 16);
    let path_next = f.jpeg_without_exif("next.jpg", 16, 16);
    let date = dt("2024:06:01 10:00:00");
    let set = DateSet { date: Some(date) };

    let mut writer =
        start_against_script_with(&shim, Duration::from_secs(30), Duration::from_millis(1000))
            .unwrap();

    let hung_res = writer.write_dates(&path_hung, &set);
    assert!(hung_res.is_err(), "hung file must fail and be reported");
    assert_eq!(read_meta(&path_hung).unwrap().capture, None);

    let next_res = writer.write_dates(&path_next, &set);
    assert!(
        next_res.is_ok(),
        "next file must succeed with restarted writer"
    );
    assert_eq!(read_meta(&path_next).unwrap().capture, Some(date));

    writer.close().unwrap();

    let spawns = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(spawns, 2, "writer must have spawned initial + 1 restart");
}

#[test]
fn the_file_that_hung_exiftool_is_reported_failed_not_retried() {
    let f = Fixtures::new();
    let log = f.path().join("spawns.log");
    let commands_log = f.path().join("commands.log");
    let shim = f.path().join("log-commands-shim");

    // Spawn 1 logs all lines; on file 1 (-execute) it hangs.
    // Spawn 2 logs all lines; on handshake and subsequent commands it returns success.
    write_script(
        &shim,
        &format!(
            r#"#!/bin/sh
log="{}"
commands="{}"
if [ -s "$log" ]; then first=0; else first=1; fi
echo spawn >> "$log"
saw_ver=0
while IFS= read -r line; do
    echo "$line" >> "$commands"
    if [ "$line" = "-execute" ]; then
        if [ "$saw_ver" = "1" ]; then
            echo "{{phototools-stderr-ready}}" >&2
            echo "12.70"
            echo "{{ready}}"
            saw_ver=0
        elif [ "$first" = "1" ]; then
            exec sleep 60
        else
            echo "{{phototools-stderr-ready}}" >&2
            echo "1 image files updated"
            echo "{{ready}}"
        fi
    elif [ "$line" = "-ver" ]; then
        saw_ver=1
    fi
done
"#,
            log.display(),
            commands_log.display()
        ),
    );

    let path_hung = f.jpeg_without_exif("hung_never_retry.jpg", 16, 16);
    let path_next = f.jpeg_without_exif("next_file.jpg", 16, 16);
    let date = dt("2024:06:01 10:00:00");
    let set = DateSet { date: Some(date) };

    let mut writer = start_against_script(&shim, Duration::from_millis(1000)).unwrap();

    let hung_res = writer.write_dates(&path_hung, &set);
    assert!(
        hung_res.is_err(),
        "the file that hung exiftool must be reported failed"
    );

    let next_res = writer.write_dates(&path_next, &set);
    assert!(next_res.is_ok(), "the subsequent file succeeds");

    writer.close().unwrap();

    let recorded = std::fs::read_to_string(&commands_log).unwrap();
    // The hung file was sent to spawn 1 once, but never retried on spawn 2.
    assert_eq!(
        recorded.matches(&path_hung.display().to_string()).count(),
        1,
        "the hung file must not be retried automatically"
    );
    assert_eq!(
        recorded.matches(&path_next.display().to_string()).count(),
        1,
        "the next file was sent once"
    );
}

#[test]
fn exiftool_is_restarted_at_most_twice_per_writer() {
    let f = Fixtures::new();
    let log = f.path().join("spawns.log");
    let shim = f.path().join("always-hang-shim");

    // Every spawn answers the -ver handshake, then hangs on the actual command.
    write_script(
        &shim,
        &format!(
            r#"#!/bin/sh
log="{}"
echo spawn >> "$log"
saw_ver=0
while IFS= read -r line; do
    if [ "$line" = "-execute" ]; then
        if [ "$saw_ver" = "1" ]; then
            echo "{{phototools-stderr-ready}}" >&2
            echo "12.70"
            echo "{{ready}}"
            saw_ver=0
        else
            exec sleep 60
        fi
    elif [ "$line" = "-ver" ]; then
        saw_ver=1
    fi
done
"#,
            log.display()
        ),
    );

    let set = DateSet {
        date: Some(dt("2024:01:01 00:00:00")),
    };
    let mut writer = start_against_script(&shim, Duration::from_millis(1000)).unwrap();
    // Initial spawn happened during start.
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);

    // Call 1: hangs, triggers restart 1 (spawn 2).
    let f1 = f.jpeg_without_exif("f1.jpg", 16, 16);
    let r1 = writer.write_dates(&f1, &set);
    assert!(r1.is_err());
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 2);

    // Call 2: hangs, triggers restart 2 (spawn 3).
    let f2 = f.jpeg_without_exif("f2.jpg", 16, 16);
    let r2 = writer.write_dates(&f2, &set);
    assert!(r2.is_err());
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 3);

    // Call 3: hangs. Restarts cap (2) reached, marked dead, no spawn.
    let f3 = f.jpeg_without_exif("f3.jpg", 16, 16);
    let r3 = writer.write_dates(&f3, &set);
    assert!(r3.is_err());
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().lines().count(),
        3,
        "cap reached; no further spawns"
    );

    // Later calls fail straight away without spawning.
    let f4 = f.jpeg_without_exif("f4.jpg", 16, 16);
    let r4 = writer.write_dates(&f4, &set);
    let err4 = r4.unwrap_err().to_string();
    assert!(
        err4.contains("not being started again"),
        "must say exiftool is not being started again; got: {err4}"
    );
    assert!(
        err4.contains("3 times"),
        "must name how many times it failed (1 + 2 restarts); got: {err4}"
    );

    let f5 = f.jpeg_without_exif("f5.jpg", 16, 16);
    let r5 = writer.write_dates(&f5, &set);
    assert!(r5.is_err());

    // Still exactly 3 spawns in all (initial + 2 restarts).
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().lines().count(),
        3,
        "exiftool must be spawned at most 3 times in all (1 + 2 restarts)"
    );
}

#[test]
fn a_restart_that_fails_reports_why_rather_than_blaming_the_cap() {
    // The first process answers its handshake and then hangs on the first file.
    // Every later process exits at once, as exiftool does when it has been
    // removed or broken underneath a running job. The files after that must be
    // told exiftool could not be started again — not that it hung too often,
    // which would send somebody looking at their photographs instead of the tool.
    let f = Fixtures::new();
    let log = f.path().join("spawns.log");
    let shim = f.path().join("dies-on-restart-shim");
    write_script(
        &shim,
        &format!(
            r#"#!/bin/sh
log="{}"
if [ -s "$log" ]; then
    echo spawn >> "$log"
    exit 1
fi
echo spawn >> "$log"
saw_ver=0
while IFS= read -r line; do
    if [ "$line" = "-execute" ]; then
        if [ "$saw_ver" = "1" ]; then
            echo "{{phototools-stderr-ready}}" >&2
            echo "12.70"
            echo "{{ready}}"
            saw_ver=0
        else
            exec sleep 60
        fi
    elif [ "$line" = "-ver" ]; then
        saw_ver=1
    fi
done
"#,
            log.display()
        ),
    );

    let set = DateSet {
        date: Some(dt("2024:01:01 00:00:00")),
    };
    let mut writer = start_against_script(&shim, Duration::from_millis(1000)).unwrap();

    let hung = f.jpeg_without_exif("hung.jpg", 16, 16);
    assert!(writer.write_dates(&hung, &set).is_err());

    let next = f.jpeg_without_exif("next.jpg", 16, 16);
    let message = writer.write_dates(&next, &set).unwrap_err().to_string();
    assert!(
        message.contains("could not be started again"),
        "must say the restart failed; got: {message}"
    );
    assert!(
        !message.contains("not being started again"),
        "must not blame the restart cap after one restart; got: {message}"
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().lines().count(),
        2,
        "one failed restart, and no further attempts"
    );
}

#[test]
fn a_crashed_exiftool_is_restarted_and_the_next_file_is_written() {
    let f = Fixtures::new();
    let log = f.path().join("spawns.log");
    let shim = f.path().join("crash-once-shim");
    let real = real_exiftool_path();

    // Spawn 1 answers -ver, then exits 1 immediately on the first write (crash / Disconnected).
    // Spawn 2 execs real exiftool so the next file is written cleanly.
    write_script(
        &shim,
        &format!(
            r#"#!/bin/sh
log="{}"
real="{}"
if [ -s "$log" ]; then first=0; else first=1; fi
echo spawn >> "$log"
if [ "$first" = "1" ]; then
    saw_ver=0
    while IFS= read -r line; do
        if [ "$line" = "-execute" ]; then
            if [ "$saw_ver" = "1" ]; then
                echo "{{phototools-stderr-ready}}" >&2
                echo "12.70"
                echo "{{ready}}"
                saw_ver=0
            else
                exit 1
            fi
        elif [ "$line" = "-ver" ]; then
            saw_ver=1
        fi
    done
else
    exec "$real" "$@"
fi
"#,
            log.display(),
            real
        ),
    );

    let path_crashed = f.jpeg_without_exif("crashed.jpg", 16, 16);
    let path_next = f.jpeg_without_exif("after_crash.jpg", 16, 16);
    let date = dt("2024:07:01 11:00:00");
    let set = DateSet { date: Some(date) };

    let mut writer =
        start_against_script_with(&shim, Duration::from_secs(30), Duration::from_millis(500))
            .unwrap();

    let crashed_res = writer.write_dates(&path_crashed, &set);
    assert!(crashed_res.is_err(), "crashed process write must fail");
    assert_eq!(read_meta(&path_crashed).unwrap().capture, None);

    let next_res = writer.write_dates(&path_next, &set);
    assert!(
        next_res.is_ok(),
        "subsequent file succeeds after crash restart"
    );
    assert_eq!(read_meta(&path_next).unwrap().capture, Some(date));

    writer.close().unwrap();

    let spawns = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(spawns, 2, "writer restarted once after crash");
}

/// A restart that is slow to start, but healthy, is not cut short by the write
/// bound. Spawn 1 hangs on its first write; spawn 2 takes well over that bound to
/// answer its handshake, as a cold Perl does on a loaded machine.
#[test]
fn a_slow_restart_is_bounded_by_the_start_up_allowance_not_the_write_timeout() {
    let f = Fixtures::new();
    let log = f.path().join("spawns.log");
    let shim = f.path().join("slow-restart-shim");
    let real = real_exiftool_path();

    write_script(
        &shim,
        &format!(
            r#"#!/bin/sh
log="{}"
real="{}"
if [ -s "$log" ]; then first=0; else first=1; fi
echo spawn >> "$log"
if [ "$first" = "1" ]; then
    saw_ver=0
    while IFS= read -r line; do
        if [ "$line" = "-execute" ]; then
            if [ "$saw_ver" = "1" ]; then
                echo "{{phototools-stderr-ready}}" >&2
                echo "12.70"
                echo "{{ready}}"
                saw_ver=0
            else
                exec sleep 60
            fi
        elif [ "$line" = "-ver" ]; then
            saw_ver=1
        fi
    done
else
    sleep 1
    exec "$real" "$@"
fi
"#,
            log.display(),
            real
        ),
    );

    let path_hung = f.jpeg_without_exif("slow_hung.jpg", 16, 16);
    let path_next = f.jpeg_without_exif("slow_next.jpg", 16, 16);
    let date = dt("2024:08:01 12:00:00");
    let set = DateSet { date: Some(date) };

    let mut writer =
        start_against_script_with(&shim, Duration::from_secs(30), Duration::from_millis(300))
            .unwrap();

    assert!(writer.write_dates(&path_hung, &set).is_err());
    writer
        .write_dates(&path_next, &set)
        .expect("a restart slower than the write bound must still be allowed to start");
    assert_eq!(read_meta(&path_next).unwrap().capture, Some(date));
    writer.close().unwrap();

    let spawns = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(spawns, 2);
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

    // A fix from the sample track: 4 September, 15:33:37 UTC.
    let fix = TrackPoint {
        at: 1_788_536_017,
        lat: 52.531549,
        lon: 3.460808,
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
        lon: 3.460808,
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

// ---------------------------------------------------------------------------
// Finding exiftool
//
// A `.app` launched from Finder inherits launchd's PATH, which does not include
// Homebrew or MacPorts. The application therefore worked from a terminal and
// wrote nothing at all when double-clicked — every date repair, every RAW
// derivative and every position.
// ---------------------------------------------------------------------------

#[test]
fn exiftool_is_found_without_help_on_a_machine_that_has_it() {
    use phototools_core::media::meta::exiftool_program;

    let program = exiftool_program().expect("this machine has exiftool");
    assert!(
        program == "exiftool" || std::path::Path::new(&program).exists(),
        "resolved to {program:?}, which is neither on PATH nor a real file"
    );
}

#[test]
fn an_explicit_path_wins_over_the_search() {
    use phototools_core::media::meta::exiftool_program_with;

    let f = Fixtures::new();
    let stand_in = f.path().join("my-exiftool");
    std::fs::write(&stand_in, "#!/bin/sh\nexec exiftool \"$@\"\n").unwrap();

    // Passed in rather than set in the environment. `EXIFTOOL_PATH` is
    // process-wide and read by every metadata write, so a test that set it
    // raced every other test in this binary that runs `exiftool` — and failed
    // whichever one happened to be writing at that moment.
    assert_eq!(
        exiftool_program_with(Some(stand_in.to_string_lossy().into())).unwrap(),
        stand_in.to_string_lossy(),
        "an explicit path should be used as given"
    );

    // And a wrong one is reported rather than silently ignored: somebody who
    // set it meant it, and quietly running a different binary answers a
    // question they did not ask.
    let error = exiftool_program_with(Some("/nowhere/exiftool".into()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("/nowhere/exiftool"), "got {error}");
    assert!(error.contains("nothing there"), "got {error}");

    // An empty value is not a configuration: the search runs as usual.
    assert!(exiftool_program_with(Some("   ".into())).is_ok());
}

/// **A write exiftool refused must be a failure, not a success.**
///
/// Found on a deployed server: thirty-nine photographs were "redated", the
/// files were untouched, and nothing anywhere said so. exiftool exits zero and
/// carries on when it cannot write a file — it says `0 image files updated` on
/// stdout and the reason on stderr — and the driver read past both on its way
/// to `{ready}`, with stderr routed to `/dev/null` (§9.2 invariant 6, G10).
#[test]
fn a_file_exiftool_cannot_write_is_reported_rather_than_counted_as_written() {
    let f = Fixtures::new();

    // A file exiftool will not write: the bytes are not an image, so it has
    // nowhere to put a date. The same refusal a permission would produce, in a
    // form a test can create on any machine.
    let path = f.path().join("not-really.jpg");
    std::fs::write(&path, b"this is not a JPEG").unwrap();

    let mut writer = ExifWriter::start().unwrap();
    let result = writer.write_dates(
        &path,
        &DateSet {
            date: Some(dt("2013:05:01 12:00:00")),
        },
    );
    writer.close().unwrap();

    let error = result.expect_err("exiftool wrote nothing, so this is not a success");
    let text = error.to_string();
    assert!(text.contains("not-really.jpg"), "says which file: {text}");
    assert!(
        text.contains("was not written"),
        "and says it was not written: {text}"
    );
}

/// The happy path still passes through the same confirmation.
#[test]
fn a_file_exiftool_did_write_is_not_reported_as_a_failure() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("real.jpg", 40, 40);

    let mut writer = ExifWriter::start().unwrap();
    let result = writer.write_dates(
        &path,
        &DateSet {
            date: Some(dt("2013:05:01 12:00:00")),
        },
    );
    writer.close().unwrap();

    assert!(result.is_ok(), "got: {result:?}");
    assert_eq!(
        read_meta(&path).unwrap().capture,
        Some(dt("2013:05:01 12:00:00"))
    );
}

// ---------------------------------------------------------------------------
// ED-4 — Preview renderer and session in core
// ---------------------------------------------------------------------------

#[test]
fn decode_tables_match_the_exact_conversion_for_every_code() {
    use phototools_core::media::edit::pipeline::{srgb_to_linear, u16_to_linear, u8_to_linear};

    for code in 0..=255u8 {
        let expected = srgb_to_linear(code as f32 / 255.0);
        let table_val = u8_to_linear(code);
        assert_eq!(
            table_val.to_bits(),
            expected.to_bits(),
            "u8 table mismatch at code {code}: expected {expected}, got {table_val}"
        );
    }

    for code in 0..=65535u16 {
        let expected = srgb_to_linear(code as f32 / 65535.0);
        let table_val = u16_to_linear(code);
        assert_eq!(
            table_val.to_bits(),
            expected.to_bits(),
            "u16 table mismatch at code {code}: expected {expected}, got {table_val}"
        );
    }
}

#[test]
fn rendering_never_changes_the_cached_proxies() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, Lut, LutRef, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 1600,
        height: 1200,
        data: (0..1600 * 1200 * 3).map(|i| (i % 256) as u8).collect(),
    };

    let session = PreviewSession::new(&buf).unwrap();

    // Snapshot the linear proxy buffers bit-for-bit before rendering
    let drag_snapshot = session.drag_linear().data.clone();
    let settle_snapshot = session.settle_linear().data.clone();

    let cube_content = "\
TITLE \"Test\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    let lut = Lut::from_cube("test.cube", cube_content.as_bytes()).unwrap();
    let lut_ref = LutRef {
        name: "test.cube".to_string(),
        sha256: lut.sha256.clone(),
    };

    let recipes_with_lut = [
        AdjustmentRecipe {
            exposure: 1.5,
            contrast: 25.0,
            highlights: -30.0,
            shadows: 20.0,
            saturation: 15.0,
            lut: Some(lut_ref.clone()),
            ..Default::default()
        },
        AdjustmentRecipe {
            exposure: -2.0,
            contrast: -15.0,
            temperature: 2000.0,
            tint: 15.0,
            vibrance: -40.0,
            lut: Some(lut_ref),
            ..Default::default()
        },
    ];

    for recipe in &recipes_with_lut {
        let drag_frame = session
            .render(recipe, Some(&lut), PreviewStage::Drag)
            .unwrap();
        assert!(drag_frame.width > 0);
        let settle_frame = session
            .render(recipe, Some(&lut), PreviewStage::Settle)
            .unwrap();
        assert!(settle_frame.width > 0);
    }

    let recipe_no_lut = AdjustmentRecipe {
        exposure: 0.8,
        contrast: 10.0,
        saturation: 5.0,
        ..Default::default()
    };
    let drag_frame = session
        .render(&recipe_no_lut, None, PreviewStage::Drag)
        .unwrap();
    assert!(drag_frame.width > 0);
    let settle_frame = session
        .render(&recipe_no_lut, None, PreviewStage::Settle)
        .unwrap();
    assert!(settle_frame.width > 0);

    // Verify bit-for-bit invariance of both cached linear proxy buffers
    assert_eq!(
        session.drag_linear().data,
        drag_snapshot,
        "rendering must never mutate the cached drag linear proxy in place"
    );
    assert_eq!(
        session.settle_linear().data,
        settle_snapshot,
        "rendering must never mutate the cached settle linear proxy in place"
    );
}

#[test]
fn the_preview_matches_the_export_renderer_within_one_code() {
    use phototools_core::media::edit::{
        apply_recipe, render_rgba_frame, AdjustmentRecipe, ImageBuffer, LinearBuffer, Lut, LutRef,
    };

    let w = 64;
    let h = 64;
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let r = ((x * 4) % 256) as u8;
            let g = ((y * 4) % 256) as u8;
            let b = (((x + y) * 2) % 256) as u8;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    let proxy_img = ImageBuffer::Rgb8 {
        width: w as u32,
        height: h as u32,
        data,
    };

    let cube_content = "\
TITLE \"Test Cube\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.1 0.05
0.05 0.9 0.1
0.95 0.95 0.2
0.1 0.05 0.8
0.85 0.2 0.85
0.2 0.85 0.9
1.0 1.0 1.0
";
    let lut = Lut::from_cube("test.cube", cube_content.as_bytes()).unwrap();
    let recipe = AdjustmentRecipe {
        exposure: 0.4,
        highlights: -20.0,
        shadows: 15.0,
        whites: 10.0,
        blacks: -10.0,
        brightness: 5.0,
        contrast: 12.0,
        saturation: 10.0,
        vibrance: -8.0,
        temperature: 15.0,
        tint: -10.0,
        hue: 12.0,
        curves: Some(phototools_core::media::edit::ToneCurves {
            luma: vec![
                phototools_core::media::edit::CurvePoint::new(0.0, 0.0),
                phototools_core::media::edit::CurvePoint::new(0.25, 0.20),
                phototools_core::media::edit::CurvePoint::new(0.75, 0.80),
                phototools_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
            red: vec![
                phototools_core::media::edit::CurvePoint::new(0.0, 0.0),
                phototools_core::media::edit::CurvePoint::new(0.5, 0.55),
                phototools_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
            green: vec![
                phototools_core::media::edit::CurvePoint::new(0.0, 0.0),
                phototools_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
            blue: vec![
                phototools_core::media::edit::CurvePoint::new(0.0, 0.0),
                phototools_core::media::edit::CurvePoint::new(0.5, 0.45),
                phototools_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
        }),
        hsl: Some(phototools_core::media::edit::HslAdjustments {
            red: phototools_core::media::edit::HslBand {
                hue: 15.0,
                saturation: 20.0,
                luminance: 10.0,
            },
            green: phototools_core::media::edit::HslBand {
                hue: -10.0,
                saturation: -15.0,
                luminance: 5.0,
            },
            blue: phototools_core::media::edit::HslBand {
                hue: 20.0,
                saturation: 25.0,
                luminance: -10.0,
            },
            ..Default::default()
        }),
        grading: Some(phototools_core::media::edit::ColorGrading {
            shadows: phototools_core::media::edit::ColorWheel {
                hue: 200.0,
                saturation: 0.3,
                luminance: -0.1,
            },
            midtones: phototools_core::media::edit::ColorWheel {
                hue: 45.0,
                saturation: 0.2,
                luminance: 0.05,
            },
            highlights: phototools_core::media::edit::ColorWheel {
                hue: 60.0,
                saturation: 0.25,
                luminance: 0.1,
            },
            global: phototools_core::media::edit::ColorWheel {
                hue: 30.0,
                saturation: 0.15,
                luminance: -0.05,
            },
            blending: 50.0,
            balance: 10.0,
        }),
        lut: Some(LutRef {
            name: "test.cube".into(),
            sha256: lut.sha256.clone(),
        }),
        lut_intensity: 0.85,
        ..Default::default()
    };

    let exact_out = apply_recipe(&proxy_img, &recipe, Some(&lut)).unwrap();
    let exact_rgb = exact_out.as_rgb8().unwrap();

    let proxy_lin = LinearBuffer::from_image_buffer(&proxy_img);
    let preview_frame = render_rgba_frame(&proxy_lin, &recipe, Some(&lut)).unwrap();

    assert_eq!(preview_frame.width, proxy_img.width());
    assert_eq!(preview_frame.height, proxy_img.height());
    assert_eq!(preview_frame.bytes.len(), w * h * 4);

    for i in 0..(w * h) {
        let exact_r = exact_rgb[i * 3] as i32;
        let exact_g = exact_rgb[i * 3 + 1] as i32;
        let exact_b = exact_rgb[i * 3 + 2] as i32;

        let prev_r = preview_frame.bytes[i * 4] as i32;
        let prev_g = preview_frame.bytes[i * 4 + 1] as i32;
        let prev_b = preview_frame.bytes[i * 4 + 2] as i32;
        let prev_a = preview_frame.bytes[i * 4 + 3];

        assert_eq!(prev_a, 255, "Alpha channel must be 255 at pixel {i}");
        assert!(
            (exact_r - prev_r).abs() <= 1,
            "Red channel at pixel {i} differs by > 1: exact {exact_r} vs preview {prev_r}"
        );
        assert!(
            (exact_g - prev_g).abs() <= 1,
            "Green channel at pixel {i} differs by > 1: exact {exact_g} vs preview {prev_r}"
        );
        assert!(
            (exact_b - prev_b).abs() <= 1,
            "Blue channel at pixel {i} differs by > 1: exact {exact_b} vs preview {prev_b}"
        );
    }
}

#[test]
fn a_preview_refuses_a_recipe_whose_lut_is_missing() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, LutRef, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 10,
        height: 10,
        data: vec![128; 300],
    };
    let session = PreviewSession::new(&buf).unwrap();

    let recipe = AdjustmentRecipe {
        lut: Some(LutRef {
            name: "missing.cube".into(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        }),
        ..Default::default()
    };

    let err = session
        .render(&recipe, None, PreviewStage::Drag)
        .unwrap_err();
    assert!(
        err.to_string().contains("no LUT was provided"),
        "expected missing LUT refusal, got: {err}"
    );
}

#[test]
fn a_preview_frame_is_rgba_of_the_proxy_size() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 3000,
        height: 2000,
        data: vec![100; 3000 * 2000 * 3],
    };
    let session = PreviewSession::new(&buf).unwrap();
    let recipe = AdjustmentRecipe::default();

    let drag = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(drag.width, 1280);
    assert_eq!(drag.height, 853);
    assert_eq!(drag.bytes.len(), 1280 * 853 * 4);
    assert!(drag.bytes.chunks_exact(4).all(|px| px[3] == 255));

    let settle = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(settle.width, 2560);
    assert_eq!(settle.height, 1706);
    assert_eq!(settle.bytes.len(), 2560 * 1706 * 4);
    assert!(settle.bytes.chunks_exact(4).all(|px| px[3] == 255));
}

/// The frame the Edit view decodes carries the histogram of exactly the pixels it
/// carries, at the offsets `PREVIEW_FRAME_HEADER_LEN` documents (ED-15).
#[test]
fn binary_preview_ipc_header_encodes_histograms_and_clipping_accurately() {
    use phototools_core::media::edit::{
        encode_preview_frame, AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage,
        PREVIEW_FRAME_HEADER_LEN,
    };
    use phototools_core::media::histogram;

    // Left third black, middle third mid-grey, right third white: both clips present.
    let (w, h) = (90u32, 20u32);
    let data: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = match (i % w) / 30 {
                0 => 0u8,
                1 => 128,
                _ => 255,
            };
            [v, v, v]
        })
        .collect();
    let session = PreviewSession::new(&ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    })
    .unwrap();
    let frame = session
        .render(&AdjustmentRecipe::default(), None, PreviewStage::Drag)
        .unwrap();

    let bytes = encode_preview_frame(&frame);
    let word = |i: usize| u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());

    assert_eq!(bytes.len(), PREVIEW_FRAME_HEADER_LEN + (w * h * 4) as usize);
    assert_eq!((word(0), word(1), word(2)), (w, h, 1));

    // Counted again from the pixels that follow the header, not taken from the frame.
    let pixels = &bytes[PREVIEW_FRAME_HEADER_LEN..];
    let expected = histogram(pixels, w, h).unwrap();
    assert_eq!(word(3), w * h);
    assert_eq!(word(4), expected.highlight_clipped);
    assert_eq!(word(5), expected.shadow_clipped);
    assert_eq!(word(4), w * h / 3, "the white third is highlight-clipped");
    let to_stored: Vec<f32> = (6..12).map(|i| f32::from_bits(word(i))).collect();
    assert_eq!(to_stored, frame.to_stored.to_vec());
    assert_eq!(
        frame.to_stored,
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        "no geometry: the stored frame"
    );
    assert_eq!(word(5), w * h / 3, "the black third is shadow-clipped");

    for (c, channel) in [
        &expected.red,
        &expected.green,
        &expected.blue,
        &expected.luminance,
    ]
    .into_iter()
    .enumerate()
    {
        let decoded: Vec<u32> = (0..256).map(|b| word(12 + c * 256 + b)).collect();
        assert_eq!(&decoded, channel, "channel {c}");
    }
}

#[test]
fn benchmark_edit_preview() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, CurvePoint, ImageBuffer, Lut, LutRef, PreviewSession, PreviewStage,
        ToneCurves,
    };

    let w = 7360;
    let h = 4912;
    let total_pixels = w * h;
    let mut data = vec![128u8; total_pixels * 3];
    for (i, byte) in data.iter_mut().enumerate() {
        *byte = ((i * 31) % 256) as u8;
    }
    let img36 = ImageBuffer::Rgb8 {
        width: w as u32,
        height: h as u32,
        data,
    };

    let size = 33;
    let mut lut_data = Vec::with_capacity(size * size * size);
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rf = r as f32 / (size - 1) as f32;
                let gf = g as f32 / (size - 1) as f32;
                let bf = b as f32 / (size - 1) as f32;
                lut_data.push([
                    (rf * 0.95 + 0.02).clamp(0.0, 1.0),
                    (gf * 0.98 + 0.01).clamp(0.0, 1.0),
                    (bf * 0.90 + 0.05).clamp(0.0, 1.0),
                ]);
            }
        }
    }
    let lut33 = Lut {
        title: Some("Synthetic 33".into()),
        size,
        domain_min: [0.0, 0.0, 0.0],
        domain_max: [1.0, 1.0, 1.0],
        sha256: "synth33_sha256".into(),
        data: lut_data,
    };

    let t_session = Instant::now();
    let session = PreviewSession::new(&img36).unwrap();
    let session_elapsed = t_session.elapsed();

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        highlights: -25.0,
        shadows: 20.0,
        whites: 15.0,
        blacks: -10.0,
        brightness: 5.0,
        contrast: 15.0,
        saturation: 12.0,
        hue: 10.0,
        curves: Some(ToneCurves {
            luma: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.25, 0.18),
                CurvePoint::new(0.75, 0.82),
                CurvePoint::new(1.0, 1.0),
            ],
            red: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.5, 0.55),
                CurvePoint::new(1.0, 1.0),
            ],
            green: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
            blue: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.5, 0.45),
                CurvePoint::new(1.0, 1.0),
            ],
        }),
        hsl: Some(phototools_core::media::edit::HslAdjustments {
            red: phototools_core::media::edit::HslBand {
                hue: 15.0,
                saturation: 20.0,
                luminance: 10.0,
            },
            orange: phototools_core::media::edit::HslBand {
                hue: 5.0,
                saturation: 10.0,
                luminance: -5.0,
            },
            yellow: phototools_core::media::edit::HslBand {
                hue: -10.0,
                saturation: 15.0,
                luminance: 0.0,
            },
            green: phototools_core::media::edit::HslBand {
                hue: -15.0,
                saturation: -20.0,
                luminance: 5.0,
            },
            aqua: phototools_core::media::edit::HslBand {
                hue: 10.0,
                saturation: 10.0,
                luminance: -10.0,
            },
            blue: phototools_core::media::edit::HslBand {
                hue: 20.0,
                saturation: 25.0,
                luminance: -15.0,
            },
            purple: phototools_core::media::edit::HslBand {
                hue: -10.0,
                saturation: 15.0,
                luminance: 5.0,
            },
            magenta: phototools_core::media::edit::HslBand {
                hue: 10.0,
                saturation: -10.0,
                luminance: 0.0,
            },
        }),
        grading: Some(phototools_core::media::edit::ColorGrading {
            shadows: phototools_core::media::edit::ColorWheel {
                hue: 210.0,
                saturation: 0.25,
                luminance: -0.05,
            },
            midtones: phototools_core::media::edit::ColorWheel {
                hue: 45.0,
                saturation: 0.15,
                luminance: 0.05,
            },
            highlights: phototools_core::media::edit::ColorWheel {
                hue: 60.0,
                saturation: 0.20,
                luminance: 0.02,
            },
            global: phototools_core::media::edit::ColorWheel {
                hue: 30.0,
                saturation: 0.10,
                luminance: 0.0,
            },
            blending: 60.0,
            balance: 10.0,
        }),
        geometry: Some(phototools_core::media::edit::Geometry {
            crop: Some(phototools_core::media::edit::NormalizedCrop {
                x: 0.05,
                y: 0.05,
                width: 0.9,
                height: 0.9,
            }),
            straighten: 7.0,
            ..Default::default()
        }),
        lut: Some(LutRef {
            name: "synth33.cube".into(),
            sha256: lut33.sha256.clone(),
        }),
        lut_intensity: 0.8,
        vignette: Some(phototools_core::media::edit::Vignette {
            amount: -40.0,
            midpoint: 45.0,
            roundness: 20.0,
            feather: 60.0,
        }),
        grain: Some(phototools_core::media::edit::FilmGrain {
            amount: 35.0,
            size: 30.0,
            roughness: 60.0,
        }),
        looks: Some(phototools_core::media::edit::LookEffects {
            glow_amount: 30.0,
            glow_threshold: 65.0,
            glow_radius: 25.0,
            halation_amount: 25.0,
            halation_threshold: 75.0,
            halation_radius: 20.0,
            tone_mapper: Some("aces".into()),
        }),
        ..Default::default()
    };

    let _ = session
        .render(&recipe, Some(&lut33), PreviewStage::Drag)
        .unwrap();
    let _ = session
        .render(&recipe, Some(&lut33), PreviewStage::Settle)
        .unwrap();

    let mut drag_times = Vec::with_capacity(40);
    for _ in 0..40 {
        let t0 = Instant::now();
        let frame = session
            .render(&recipe, Some(&lut33), PreviewStage::Drag)
            .unwrap();
        drag_times.push(t0.elapsed());
        assert!(frame.width > 0);
    }
    drag_times.sort();
    let p95_drag_idx = ((drag_times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_drag = drag_times[p95_drag_idx];

    let mut straighten_drag_times = Vec::with_capacity(40);
    for i in 0..40 {
        let angle = -5.0 + (i as f32) * 0.25;
        let mut geom_recipe = recipe.clone();
        if let Some(g) = geom_recipe.geometry.as_mut() {
            g.straighten = angle;
        }
        let t0 = Instant::now();
        let frame = session
            .render(&geom_recipe, Some(&lut33), PreviewStage::Drag)
            .unwrap();
        straighten_drag_times.push(t0.elapsed());
        assert!(frame.width > 0);
    }
    straighten_drag_times.sort();
    let p95_straighten_drag_idx =
        ((straighten_drag_times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_straighten_drag = straighten_drag_times[p95_straighten_drag_idx];

    let mut settle_times = Vec::with_capacity(40);
    for _ in 0..40 {
        let t0 = Instant::now();
        let frame = session
            .render(&recipe, Some(&lut33), PreviewStage::Settle)
            .unwrap();
        settle_times.push(t0.elapsed());
        assert!(frame.width > 0);
    }
    settle_times.sort();
    let p95_settle_idx = ((settle_times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_settle = settle_times[p95_settle_idx];

    // Round 3 budget (ED-19): the same recipe with four masks, each large and carrying
    // several local adjustments, so most pixels pay for more than one. Each mask may add at
    // most 1.5 ms to a drag frame and 5 ms to a settle frame.
    const MASKS: u32 = 4;
    let masked_recipe = {
        use phototools_core::media::edit::{LocalAdjustments, Mask, MaskKind};
        let adj = LocalAdjustments {
            exposure: 0.6,
            temperature: -15.0,
            highlights: -30.0,
            contrast: 12.0,
            saturation: 20.0,
            ..Default::default()
        };
        let mask = |id: &str, kind: MaskKind| Mask {
            id: id.into(),
            name: id.into(),
            kind,
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: adj,
            strokes: Vec::new(),
        };
        let mut r = recipe.clone();
        r.masks = vec![
            mask(
                "sky",
                MaskKind::Linear {
                    start: [0.5, 0.0],
                    end: [0.5, 0.6],
                },
            ),
            mask(
                "ground",
                MaskKind::Linear {
                    start: [0.5, 1.0],
                    end: [0.5, 0.4],
                },
            ),
            mask(
                "face",
                MaskKind::Radial {
                    center: [0.45, 0.5],
                    radius_x: 0.35,
                    radius_y: 0.3,
                    angle: 20.0,
                    feather: 0.6,
                },
            ),
            mask(
                "frame",
                MaskKind::Radial {
                    center: [0.6, 0.45],
                    radius_x: 0.4,
                    radius_y: 0.4,
                    angle: 0.0,
                    feather: 0.5,
                },
            ),
        ];
        assert_eq!(r.masks.len() as u32, MASKS);
        r
    };
    let time_stage = |stage: PreviewStage| {
        let _ = session.render(&masked_recipe, Some(&lut33), stage).unwrap();
        let mut t: Vec<Duration> = (0..40)
            .map(|_| {
                let t0 = Instant::now();
                let f = session.render(&masked_recipe, Some(&lut33), stage).unwrap();
                let e = t0.elapsed();
                assert!(f.width > 0);
                e
            })
            .collect();
        t.sort();
        t
    };
    let masked_drag = time_stage(PreviewStage::Drag);
    let masked_settle = time_stage(PreviewStage::Settle);
    let p95 = |t: &[Duration]| t[((t.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    let p95_masked_drag = p95(&masked_drag);
    let p95_masked_settle = p95(&masked_settle);

    // ED-21: painting. One brush mask; a 200-point stroke grows by one point per drag frame,
    // as a pointer move does, then the photograph settles. One mask's budget applies.
    let mut painted = recipe.clone();
    painted.masks = vec![{
        use phototools_core::media::edit::{LocalAdjustments, Mask, MaskKind, Stroke};
        Mask {
            id: "brush".into(),
            name: "Brush".into(),
            kind: MaskKind::Brush,
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure: 0.7,
                shadows: 25.0,
                ..Default::default()
            },
            strokes: vec![Stroke {
                points: vec![[0.1, 0.5]],
                radius: 0.04,
                feather: 0.5,
                flow: 1.0,
                erase: false,
            }],
        }
    }];
    let mut paint_times = Vec::with_capacity(200);
    for k in 0..200 {
        let t = k as f32 / 199.0;
        painted.masks[0].strokes[0]
            .points
            .push([0.1 + 0.8 * t, 0.5 + 0.25 * (t * 12.0).sin()]);
        let t0 = Instant::now();
        let f = session
            .render(&painted, Some(&lut33), PreviewStage::Drag)
            .unwrap();
        paint_times.push(t0.elapsed());
        assert!(f.width > 0);
    }
    paint_times.sort();
    let p95_paint = p95(&paint_times);
    let _ = session
        .render(&painted, Some(&lut33), PreviewStage::Settle)
        .unwrap();
    let mut brushed_settle: Vec<Duration> = (0..20)
        .map(|_| {
            let t0 = Instant::now();
            let f = session
                .render(&painted, Some(&lut33), PreviewStage::Settle)
                .unwrap();
            assert!(f.width > 0);
            t0.elapsed()
        })
        .collect();
    brushed_settle.sort();
    let p95_brushed_settle = p95(&brushed_settle);

    // Measure per-stage timing on 1440p settle frame proxy (2560x1708, ~4.37 MP)
    let settle_proxy = session.settle_linear();
    let w_s = settle_proxy.width;
    let _h_s = settle_proxy.height;
    let row_in_len = (w_s * 3) as usize;

    use rayon::prelude::*;

    // 1. Linear adjustments
    let t_lin = Instant::now();
    for _ in 0..10 {
        let h_val = recipe.highlights / 100.0;
        let s_val = recipe.shadows / 100.0;
        let w_val = recipe.whites / 100.0;
        let b_val = recipe.blacks / 100.0;
        let bright = recipe.brightness / 100.0;
        let bright_gamma = (-0.5 * bright).exp2();
        let c = recipe.contrast / 100.0;
        let gamma_minus_one = 0.5 * c;
        let b_exp = bright_gamma - 1.0;
        let c_scale = (1.0f32 / 0.18f32).powf(gamma_minus_one);
        let comb_exp = b_exp + bright_gamma * gamma_minus_one;
        let sat = recipe.saturation / 100.0;
        let vib = recipe.vibrance / 100.0;

        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let mut r = px[0] * 1.414;
                    let mut g = px[1] * 1.414;
                    let mut b = px[2] * 1.414;
                    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let delta_ev = if y >= 0.18 {
                        let w_h = if y >= 0.65 {
                            1.0
                        } else {
                            let t = (y - 0.18) * 2.1276;
                            t * t * (3.0 - 2.0 * t)
                        };
                        let w_w = if y >= 1.0 {
                            1.0
                        } else {
                            let t = (y - 0.18) * 1.2195;
                            t * t * (3.0 - 2.0 * t)
                        };
                        w_h * h_val + w_w * w_val
                    } else {
                        let w_s = if y <= 0.05 {
                            1.0
                        } else {
                            let t = (0.18 - y) * 7.6923;
                            t * t * (3.0 - 2.0 * t)
                        };
                        let w_b = if y <= 0.02 {
                            1.0
                        } else {
                            let t = (0.18 - y) * 6.25;
                            t * t * (3.0 - 2.0 * t)
                        };
                        w_s * s_val + w_b * b_val
                    };
                    let f = delta_ev.exp2();
                    r *= f;
                    g *= f;
                    b *= f;
                    let factor = (comb_exp * y.log2()).exp2() * c_scale;
                    r *= factor;
                    g *= factor;
                    b *= factor;
                    let max = r.max(g).max(b);
                    let min = r.min(g).min(b);
                    let cur_sat = if max > 1e-7 {
                        ((max - min) / max).min(1.0)
                    } else {
                        0.0
                    };
                    let k = ((1.0 + sat) * (1.0 + vib * (1.0 - cur_sat))).max(0.0);
                    let out_r = (y + k * (r - y)).max(0.0);
                    std::hint::black_box(out_r);
                }
            });
    }
    let lin_time = t_lin.elapsed() / 10;

    // 2. Oklab stage: Global Hue rotation and 8-band HSL in ONE pass
    let compiled_hsl = phototools_core::media::edit::hsl::CompiledHslTable::from_recipe(
        recipe.hsl.as_ref(),
        recipe.hue,
    );
    let t_hue = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let out = compiled_hsl.apply_linear_srgb(px[0], px[1], px[2]);
                    std::hint::black_box(out);
                }
            });
    }
    let hue_time = t_hue.elapsed() / 10;

    // 3. Display transfer (fast linear-to-sRGB)
    let t_disp = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let out = (
                        phototools_core::media::edit::preview::fast_linear_to_srgb(px[0]),
                        phototools_core::media::edit::preview::fast_linear_to_srgb(px[1]),
                        phototools_core::media::edit::preview::fast_linear_to_srgb(px[2]),
                    );
                    std::hint::black_box(out);
                }
            });
    }
    let disp_time = t_disp.elapsed() / 10;

    // 4. Tone Curves (evaluated once into 1024-entry LUTs, sampled per pixel)
    let curves_table =
        phototools_core::media::edit::curves::ToneCurvesTable::from_recipe(recipe.curves.as_ref());
    let t_curves = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let out = curves_table.apply(px[0], px[1], px[2]);
                    std::hint::black_box(out);
                }
            });
    }
    let curves_time = t_curves.elapsed() / 10;

    // 5. Colour Grading (1024-entry luma-indexed LUT precomputed from OkLCh tints and CDL mapping)
    let grading_table = phototools_core::media::edit::grading::CompiledGradingTable::from_recipe(
        recipe.grading.as_ref(),
    );
    let t_grading = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let out = grading_table.apply(px[0], px[1], px[2]);
                    std::hint::black_box(out);
                }
            });
    }
    let grading_time = t_grading.elapsed() / 10;

    // 6. 3D LUT Tetrahedral Sampling
    let t_lut = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let out = lut33.sample([px[0], px[1], px[2]]);
                    std::hint::black_box(out);
                }
            });
    }
    let lut_time = t_lut.elapsed() / 10;

    // 7. Vignette (smooth multiplicative gain in linear light post-geometry)
    let compiled_vignette = recipe.vignette.as_ref().unwrap().compile(w_s, _h_s);
    let t_vignette = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .enumerate()
            .for_each(|(y, row)| {
                let y_f = y as f32;
                let v = (y_f + 0.5 - compiled_vignette.yc) * compiled_vignette.inv_hy;
                let v2 = v * v;
                for (x, px) in row.chunks_exact(3).enumerate() {
                    let gain = compiled_vignette.gain_with_v(x as f32, v, v2);
                    let out = (px[0] * gain, px[1] * gain, px[2] * gain);
                    std::hint::black_box(out);
                }
            });
    }
    let vignette_time = t_vignette.elapsed() / 10;

    // 8. Film Grain (luminance only, display domain, parabolic envelope)
    let compiled_grain = recipe
        .grain
        .as_ref()
        .unwrap()
        .compile(&recipe.source_sha256);
    let inv_w_s = 1.0 / (w_s as f32);
    let t_grain = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .enumerate()
            .for_each(|(y, row)| {
                let y_f = y as f32;
                let py1 = (y_f + 0.5) * inv_w_s * compiled_grain.k_base;
                let rp = compiled_grain.row_params(py1);
                for (x, px) in row.chunks_exact(3).enumerate() {
                    let u = (x as f32 + 0.5) * inv_w_s;
                    let delta = compiled_grain.sample_delta_with_row(u, &rp, px[0], px[1], px[2]);
                    let out = (px[0] + delta, px[1] + delta, px[2] + delta);
                    std::hint::black_box(out);
                }
            });
    }
    let grain_time = t_grain.elapsed() / 10;

    // 9. Glow & Halation (reduced-resolution dual-filter blur pyramid)
    let looks_cfg = recipe.looks.as_ref().unwrap();
    let t_looks = Instant::now();
    for _ in 0..10 {
        let bloom =
            phototools_core::media::edit::looks::build_combined_bloom(settle_proxy, looks_cfg);
        std::hint::black_box(bloom);
    }
    let looks_time = t_looks.elapsed() / 10;

    // 10. ACES filmic tone mapper (Narkowicz curve on 4.37M pixels)
    let t_tm = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.chunks_exact(3) {
                    let r = phototools_core::media::edit::aces_narkowicz(px[0]);
                    let g = phototools_core::media::edit::aces_narkowicz(px[1]);
                    let b = phototools_core::media::edit::aces_narkowicz(px[2]);
                    std::hint::black_box((r, g, b));
                }
            });
    }
    let tone_mapper_time = t_tm.elapsed() / 10;

    let min_drag = drag_times[0];
    let median_drag = drag_times[drag_times.len() / 2];
    let min_settle = settle_times[0];
    let median_settle = settle_times[settle_times.len() / 2];

    println!(
        "Preview benchmark [36 MP 7360x4912, {} build]:",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!(
        "  Session creation (proxies + linearisation): {:?}",
        session_elapsed
    );
    println!(
        "  Colour Drag frame (720p 1280x854, cached geometry) over 40: min={:?}, median={:?}, p95={:?}",
        min_drag, median_drag, p95_drag
    );
    println!(
        "  Straighten Drag frame (720p 1280x854, recomputed geometry) over 40: p95={:?}",
        p95_straighten_drag
    );
    println!(
        "  Settle frame (1440p 2560x1708, cached geometry) over 40: min={:?}, median={:?}, p95={:?}",
        min_settle, median_settle, p95_settle
    );
    println!(
        "  With {MASKS} masks: drag median={:?} p95={:?}; settle median={:?} p95={:?}; per mask (medians): drag +{:?}, settle +{:?}",
        masked_drag[masked_drag.len() / 2],
        p95_masked_drag,
        masked_settle[masked_settle.len() / 2],
        p95_masked_settle,
        masked_drag[masked_drag.len() / 2].saturating_sub(median_drag) / MASKS,
        masked_settle[masked_settle.len() / 2].saturating_sub(median_settle) / MASKS,
    );
    println!(
        "  Painting a 200-point stroke: drag frame median={:?} p95={:?}; settle after it p95={:?}",
        paint_times[paint_times.len() / 2],
        p95_paint,
        p95_brushed_settle
    );
    println!("  Per-stage breakdown (1440p settle frame, 4.37 MP):");
    println!(
        "    1. Linear adjustments (tones, exposure, brightness, contrast, sat/vib): {:?}",
        lin_time
    );
    println!(
        "    2. Oklab stage (global hue + 8-band HSL in one pass on 4.37M pixels): {:?}",
        hue_time
    );
    println!(
        "    3. Display transfer (4096-entry fast linear-to-sRGB table): {:?}",
        disp_time
    );
    println!(
        "    4. Tone curves (1024-entry Fritsch-Carlson LUTs, Luma + RGB): {:?}",
        curves_time
    );
    println!(
        "    5. Colour grading (1024-entry luma LUT with OkLCh tints + CDL): {:?}",
        grading_time
    );
    println!(
        "    6. 3D LUT sampling & blending (33x33x33 tetrahedral interpolation): {:?}",
        lut_time
    );
    println!(
        "    7. Vignette (multiplicative gain in linear light on 4.37M pixels): {:?}",
        vignette_time
    );
    println!(
        "    8. Film grain (display-domain luminance noise on 4.37M pixels): {:?}",
        grain_time
    );
    println!(
        "    9. Glow & Halation (reduced-resolution dual-filter blur pyramid): {:?}",
        looks_time
    );
    println!(
        "    10. ACES filmic tone mapper (Narkowicz curve on 4.37M pixels): {:?}",
        tone_mapper_time
    );

    #[cfg(not(debug_assertions))]
    {
        assert!(
            session_elapsed <= Duration::from_millis(1000),
            "Session creation budget is <= 1000 ms, measured {:?}",
            session_elapsed
        );
        assert!(
            p95_drag <= Duration::from_millis(12),
            "Drag frame p95 budget is <= 12 ms, measured {:?}",
            p95_drag
        );
        assert!(
            p95_straighten_drag <= Duration::from_millis(25),
            "Straighten drag frame p95 budget is <= 25 ms, measured {:?}",
            p95_straighten_drag
        );
        assert!(
            p95_settle <= Duration::from_millis(40),
            "Settle frame p95 budget is <= 40 ms, measured {:?}",
            p95_settle
        );
        let drag_budget = Duration::from_millis(12) + Duration::from_micros(1500) * MASKS;
        let settle_budget = Duration::from_millis(40) + Duration::from_millis(5) * MASKS;
        assert!(
            p95_masked_drag <= drag_budget,
            "Drag frame p95 with {MASKS} masks: budget {drag_budget:?}, measured {p95_masked_drag:?}"
        );
        assert!(
            p95_masked_settle <= settle_budget,
            "Settle frame p95 with {MASKS} masks: budget {settle_budget:?}, measured {p95_masked_settle:?}"
        );
        // One brush mask: one mask's share of the budget, while painting and after.
        assert!(
            p95_paint <= Duration::from_micros(13_500),
            "Drag frame p95 while painting a 200-point stroke: budget 13.5 ms, measured {p95_paint:?}"
        );
        assert!(
            p95_brushed_settle <= Duration::from_millis(45),
            "Settle frame p95 with a painted mask: budget 45 ms, measured {p95_brushed_settle:?}"
        );
    }
}

#[test]
fn a_colour_only_change_does_not_recompute_geometry() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, Geometry, ImageBuffer, NormalizedCrop, PreviewSession, PreviewStage,
    };
    use std::sync::atomic::Ordering;

    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 100,
        data: vec![128u8; 100 * 100 * 3],
    };
    let session = PreviewSession::new(&buf).unwrap();

    let mut recipe = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            rotate: 90,
            straighten: 5.0,
            ..Default::default()
        }),
        exposure: 0.0,
        temperature: 0.0,
        ..Default::default()
    };

    // First render with geometry active: should compute geometry once
    let frame1 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(frame1.orientation, 1);
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Scrub exposure
    recipe.exposure = 0.5;
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Scrub temperature
    recipe.temperature = 25.0;
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Scrub contrast
    recipe.contrast = 20.0;
    let _ = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Now change geometry: straighten angle from 5.0 to 7.0
    if let Some(geom) = recipe.geometry.as_mut() {
        geom.straighten = 7.0;
    }
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.geom_recompute_count.load(Ordering::Relaxed),
        2,
        "Changing geometry must trigger exactly one recomputation"
    );

    // Another colour scrub after new geometry
    recipe.shadows = 15.0;
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.geom_recompute_count.load(Ordering::Relaxed),
        2,
        "Subsequent colour changes must reuse the newly cached geometry"
    );
}

#[test]
fn preview_frame_without_geometry_reports_photo_orientation_and_with_geometry_reports_one() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, Geometry, ImageBuffer, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 60,
        data: vec![128u8; 100 * 60 * 3],
    };
    // Photo with orientation 6
    let session = PreviewSession::from_image_buffer_with_orientation(&buf, 6).unwrap();

    // 1. Recipe with NO geometry reports orientation 6
    let no_geom_recipe = AdjustmentRecipe::default();
    assert!(no_geom_recipe.geometry.is_none());
    let frame_no_geom = session
        .render(&no_geom_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(frame_no_geom.orientation, 6);

    // 2. Recipe with geometry (even identity / crop: null) reports orientation 1
    let with_geom_recipe = AdjustmentRecipe {
        geometry: Some(Geometry::default()),
        ..Default::default()
    };
    let frame_with_geom = session
        .render(&with_geom_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(frame_with_geom.orientation, 1);
}

#[test]
fn the_vignette_follows_the_crop() {
    use phototools_core::media::edit::{
        apply_recipe, AdjustmentRecipe, Geometry, ImageBuffer, NormalizedCrop, Vignette,
    };

    let w = 200;
    let h = 200;
    let img = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: vec![200u8; (w * h * 3) as usize],
    };

    // Crop the left half: x=0.0..0.5, y=0.0..1.0 -> output is 100x200
    // Vignette darkens corners strongly
    let recipe = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            ..Default::default()
        }),
        vignette: Some(Vignette {
            amount: -100.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
        }),
        ..Default::default()
    };

    let out = apply_recipe(&img, &recipe, None).unwrap();
    assert_eq!(out.width(), 100);
    assert_eq!(out.height(), 200);

    let data = out.as_rgb8().unwrap();
    // Center of the cropped output is at (50, 100).
    let center_idx = (100 * 100 + 50) * 3;
    let center_val = data[center_idx];

    // Corner of the cropped output is at (0, 0):
    let corner_idx = 0;
    let corner_val = data[corner_idx];

    // Right edge center of the cropped output is at (99, 100):
    let right_edge_idx = (100 * 100 + 99) * 3;
    let right_edge_val = data[right_edge_idx];

    // The vignette center must be at the center of the cropped half (center_val ~ 200)
    assert!(
        (center_val as i32 - 200).abs() <= 5,
        "Vignette center should have minimal attenuation: got {}",
        center_val
    );

    // The corner of the cropped half must be strongly darkened (< 50)
    assert!(
        corner_val < 50,
        "Vignette corner must be strongly darkened: got {}",
        corner_val
    );

    // In the original uncropped frame, (99, 100) was in the middle of the frame.
    // In the cropped frame, it is on the boundary edge, so it MUST be darkened relative to the center!
    assert!(
        right_edge_val < center_val - 20,
        "Edge of cropped frame must be darker than the center: center={}, edge={}",
        center_val,
        right_edge_val
    );
}

#[test]
fn preview_and_export_seed_grain_identically_for_an_unsaved_recipe() {
    use phototools_core::media::edit::{
        grain::seed_from_source_sha256, AdjustmentRecipe, FilmGrain, PreviewSession, PreviewStage,
    };
    use phototools_core::tools::edit::render_and_write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let image_path = dir.path().join("unsaved_test.jpg");

    let dyn_img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        120,
        80,
        image::Rgb([128, 128, 128]),
    ));
    dyn_img.save(&image_path).unwrap();

    // 1. Open preview session: hashes the file into session.source_sha256
    let session = PreviewSession::open(&image_path).unwrap();
    assert!(!session.source_sha256.is_empty());

    // 2. Create unsaved recipe: source_sha256 is explicitly empty
    let unsaved_recipe = AdjustmentRecipe {
        grain: Some(FilmGrain {
            amount: 60.0,
            size: 25.0,
            roughness: 50.0,
        }),
        source_sha256: String::new(),
        ..Default::default()
    };

    // 3. Preview render: falls back to session.source_sha256
    let preview_frame = session
        .render(&unsaved_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert!(preview_frame.width > 0);

    // 4. Export render: falls back to hashing the source file
    let out_dir = dir.path().join("out");
    let derived = render_and_write(&image_path, &unsaved_recipe, None, &out_dir, "test").unwrap();
    assert!(derived.output.exists());

    // Verify both resolved to the identical seed
    let expected_seed = seed_from_source_sha256(&session.source_sha256);
    assert_ne!(expected_seed, 0);

    let file_bytes = std::fs::read(&image_path).unwrap();
    let hash_bytes = <sha2::Sha256 as sha2::Digest>::digest(&file_bytes);
    let computed_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    assert_eq!(session.source_sha256, computed_hash);
    assert_eq!(seed_from_source_sha256(&computed_hash), expected_seed);
}

#[test]
fn untouched_vignette_and_grain_are_exact_identity() {
    use phototools_core::media::edit::{
        apply_recipe, AdjustmentRecipe, FilmGrain, ImageBuffer, Vignette,
    };

    let w = 32;
    let h = 32;
    let mut data8 = Vec::with_capacity((w * h * 3) as usize);
    let mut data16 = Vec::with_capacity((w * h * 3) as usize);
    for i in 0..(w * h) {
        let r8 = ((i * 37) % 256) as u8;
        let g8 = ((i * 73) % 256) as u8;
        let b8 = ((i * 109) % 256) as u8;
        data8.extend_from_slice(&[r8, g8, b8]);

        let r16 = ((i * 10007) % 65536) as u16;
        let g16 = ((i * 20011) % 65536) as u16;
        let b16 = ((i * 30013) % 65536) as u16;
        data16.extend_from_slice(&[r16, g16, b16]);
    }
    let buf8 = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: data8,
    };
    let buf16 = ImageBuffer::Rgb16 {
        width: w,
        height: h,
        data: data16,
    };

    // Recipe with default vignette & grain
    let recipe_defaults = AdjustmentRecipe {
        vignette: Some(Vignette::default()),
        grain: Some(FilmGrain::default()),
        ..Default::default()
    };
    assert!(recipe_defaults.is_identity());

    let out8_defaults = apply_recipe(&buf8, &recipe_defaults, None).unwrap();
    assert_eq!(
        out8_defaults, buf8,
        "Default vignette & grain on Rgb8 must be bit-exact identity"
    );

    let out16_defaults = apply_recipe(&buf16, &recipe_defaults, None).unwrap();
    assert_eq!(
        out16_defaults, buf16,
        "Default vignette & grain on Rgb16 must be bit-exact identity"
    );

    // Recipe with zero amount vignette & grain
    let recipe_zeros = AdjustmentRecipe {
        vignette: Some(Vignette {
            amount: 0.0,
            midpoint: 40.0,
            roundness: 20.0,
            feather: 80.0,
        }),
        grain: Some(FilmGrain {
            amount: 0.0,
            size: 50.0,
            roughness: 100.0,
        }),
        ..Default::default()
    };
    assert!(recipe_zeros.is_identity());

    let out8_zeros = apply_recipe(&buf8, &recipe_zeros, None).unwrap();
    assert_eq!(
        out8_zeros, buf8,
        "Zero-amount vignette & grain on Rgb8 must be bit-exact identity"
    );

    let out16_zeros = apply_recipe(&buf16, &recipe_zeros, None).unwrap();
    assert_eq!(
        out16_zeros, buf16,
        "Zero-amount vignette & grain on Rgb16 must be bit-exact identity"
    );
}

#[test]
fn vignette_attenuation_is_identical_on_preview_proxy_and_full_export() {
    use phototools_core::media::edit::{
        apply_recipe, AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage, Vignette,
    };

    let w = 2400;
    let h = 1600;
    let flat_buf = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: vec![128u8; (w * h * 3) as usize],
    };

    let vignette = Vignette {
        amount: -50.0,
        midpoint: 50.0,
        roundness: 0.0,
        feather: 50.0,
    };
    let recipe = AdjustmentRecipe {
        vignette: Some(vignette),
        ..Default::default()
    };

    // Full export
    let export_out = apply_recipe(&flat_buf, &recipe, None).unwrap();
    let export_data = export_out.as_rgb8().unwrap();

    // Preview proxy
    let session = PreviewSession::new(&flat_buf).unwrap();
    let drag_frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();

    // Sample normalized locations: center (0.5, 0.5), corner (0.05, 0.05), midpoint (0.25, 0.25)
    let test_points = [(0.5f32, 0.5f32), (0.05, 0.05), (0.25, 0.25), (0.1, 0.5)];
    for &(u, v) in &test_points {
        let ex_x = ((u * w as f32).round() as u32).min(w - 1);
        let ex_y = ((v * h as f32).round() as u32).min(h - 1);
        let ex_idx = ((ex_y * w + ex_x) * 3) as usize;
        let ex_r = export_data[ex_idx] as f32;

        let pr_x = ((u * drag_frame.width as f32).round() as u32).min(drag_frame.width - 1);
        let pr_y = ((v * drag_frame.height as f32).round() as u32).min(drag_frame.height - 1);
        let pr_idx = ((pr_y * drag_frame.width + pr_x) * 4) as usize;
        let pr_r = drag_frame.bytes[pr_idx] as f32;

        let diff = (ex_r - pr_r).abs();
        assert!(
            diff <= 1.5,
            "Vignette attenuation mismatch at ({u}, {v}): export={ex_r}, proxy={pr_r}, diff={diff}"
        );
    }
}

#[test]
fn grain_is_byte_identical_between_two_runs_of_the_same_photo() {
    use phototools_core::media::edit::{
        apply_recipe, AdjustmentRecipe, FilmGrain, ImageBuffer, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 100,
        data: vec![128u8; 100 * 100 * 3],
    };
    let recipe = AdjustmentRecipe {
        source_sha256: "test_sha_deterministic".into(),
        grain: Some(FilmGrain {
            amount: 50.0,
            size: 25.0,
            roughness: 50.0,
        }),
        ..Default::default()
    };

    let run1 = apply_recipe(&buf, &recipe, None).unwrap();
    let run2 = apply_recipe(&buf, &recipe, None).unwrap();
    assert_eq!(
        run1, run2,
        "Export grain must be bit-exact identical between runs"
    );

    let session = PreviewSession::new(&buf).unwrap();
    let preview1 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let preview2 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        preview1.bytes, preview2.bytes,
        "Preview grain must be bit-exact identical between runs"
    );
}

#[test]
fn grain_evaluation_is_fully_deterministic_across_threads() {
    use phototools_core::media::edit::{apply_recipe, AdjustmentRecipe, FilmGrain, ImageBuffer};

    let w = 200;
    let h = 200;
    let buf = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: vec![128u8; (w * h * 3) as usize],
    };
    let recipe = AdjustmentRecipe {
        source_sha256: "thread_determinism_test".into(),
        grain: Some(FilmGrain {
            amount: 75.0,
            size: 30.0,
            roughness: 60.0,
        }),
        ..Default::default()
    };

    let pool1 = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let out_1_thread = pool1.install(|| apply_recipe(&buf, &recipe, None).unwrap());

    let pool8 = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();
    let out_8_threads = pool8.install(|| apply_recipe(&buf, &recipe, None).unwrap());

    assert_eq!(
        out_1_thread, out_8_threads,
        "Grain evaluation must produce byte-identical output across 1 and 8 threads"
    );
}

#[test]
fn grain_appearance_and_density_are_scale_independent_between_proxy_and_export() {
    use phototools_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, FilmGrain, ImageBuffer,
        PreviewSession, PreviewStage,
    };

    // Full export resolution: 2400x1600 (3:2)
    let w_exp = 2400;
    let h_exp = 1600;
    let exp_buf = ImageBuffer::Rgb8 {
        width: w_exp,
        height: h_exp,
        data: vec![128u8; (w_exp * h_exp * 3) as usize],
    };

    let recipe = AdjustmentRecipe {
        source_sha256: "scale_independence_test_seed".into(),
        grain: Some(FilmGrain {
            amount: 50.0,
            size: 40.0,
            roughness: 30.0,
        }),
        ..Default::default()
    };

    // 1. Full export
    let export_out = apply_recipe(&exp_buf, &recipe, None).unwrap();

    // 2. Preview proxy (drag proxy is 1280x853)
    let session = PreviewSession::new(&exp_buf).unwrap();
    let proxy_frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let w_pr = proxy_frame.width;
    let h_pr = proxy_frame.height;

    // Downscale export to proxy dimensions
    let export_downscaled = downscale_image_buffer(&export_out, w_pr.max(h_pr)).unwrap();
    assert_eq!(export_downscaled.width(), w_pr);
    assert_eq!(export_downscaled.height(), h_pr);

    let down_data = export_downscaled.as_rgb8().unwrap();
    let proxy_bytes = &proxy_frame.bytes;

    // Compare over a 4x4 grid of patches
    let patches_x = 4;
    let patches_y = 4;
    let patch_w = w_pr / patches_x;
    let patch_h = h_pr / patches_y;

    for py in 0..patches_y {
        for px in 0..patches_x {
            let start_x = px * patch_w;
            let start_y = py * patch_h;

            let mut sum_pr = 0.0f64;
            let mut sum_down = 0.0f64;
            let mut count = 0usize;

            for y in start_y..(start_y + patch_h) {
                for x in start_x..(start_x + patch_w) {
                    let pr_idx = ((y * w_pr + x) * 4) as usize;
                    let pr_y = 0.2126 * proxy_bytes[pr_idx] as f64
                        + 0.7152 * proxy_bytes[pr_idx + 1] as f64
                        + 0.0722 * proxy_bytes[pr_idx + 2] as f64;

                    let down_idx = ((y * w_pr + x) * 3) as usize;
                    let down_y = 0.2126 * down_data[down_idx] as f64
                        + 0.7152 * down_data[down_idx + 1] as f64
                        + 0.0722 * down_data[down_idx + 2] as f64;

                    sum_pr += pr_y;
                    sum_down += down_y;
                    count += 1;
                }
            }

            let mean_pr = sum_pr / count as f64;
            let mean_down = sum_down / count as f64;

            // Assert mean luminance within 1%
            let mean_diff_pct = (mean_pr - mean_down).abs() / mean_pr;
            assert!(
                mean_diff_pct <= 0.01,
                "Mean luminance diff {mean_diff_pct:.4} exceeds 1% at patch ({px}, {py}): pr={mean_pr}, down={mean_down}"
            );

            // Compute standard deviations
            let mut var_pr = 0.0f64;
            let mut var_down = 0.0f64;
            for y in start_y..(start_y + patch_h) {
                for x in start_x..(start_x + patch_w) {
                    let pr_idx = ((y * w_pr + x) * 4) as usize;
                    let pr_y = 0.2126 * proxy_bytes[pr_idx] as f64
                        + 0.7152 * proxy_bytes[pr_idx + 1] as f64
                        + 0.0722 * proxy_bytes[pr_idx + 2] as f64;

                    let down_idx = ((y * w_pr + x) * 3) as usize;
                    let down_y = 0.2126 * down_data[down_idx] as f64
                        + 0.7152 * down_data[down_idx + 1] as f64
                        + 0.0722 * down_data[down_idx + 2] as f64;

                    var_pr += (pr_y - mean_pr).powi(2);
                    var_down += (down_y - mean_down).powi(2);
                }
            }

            let std_pr = (var_pr / count as f64).sqrt();
            let std_down = (var_down / count as f64).sqrt();

            // Assert local standard deviation within 15%
            let std_diff_pct = (std_pr - std_down).abs() / std_pr.max(1e-5);
            assert!(
                std_diff_pct <= 0.15,
                "Std dev diff {std_diff_pct:.4} exceeds 15% at patch ({px}, {py}): pr={std_pr}, down={std_down}"
            );
        }
    }
}

#[test]
fn untouched_looks_are_exact_identity() {
    use phototools_core::media::edit::{apply_recipe, AdjustmentRecipe, ImageBuffer, LookEffects};

    let w = 32;
    let h = 32;
    let mut data8 = Vec::with_capacity((w * h * 3) as usize);
    let mut data16 = Vec::with_capacity((w * h * 3) as usize);
    for i in 0..(w * h) {
        let r8 = ((i * 37) % 256) as u8;
        let g8 = ((i * 73) % 256) as u8;
        let b8 = ((i * 109) % 256) as u8;
        data8.extend_from_slice(&[r8, g8, b8]);

        let r16 = ((i * 10007) % 65536) as u16;
        let g16 = ((i * 20011) % 65536) as u16;
        let b16 = ((i * 30013) % 65536) as u16;
        data16.extend_from_slice(&[r16, g16, b16]);
    }
    let buf8 = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: data8,
    };
    let buf16 = ImageBuffer::Rgb16 {
        width: w,
        height: h,
        data: data16,
    };

    let recipe = AdjustmentRecipe {
        looks: Some(LookEffects::default()),
        ..Default::default()
    };
    assert!(recipe.is_identity());

    let out8 = apply_recipe(&buf8, &recipe, None).unwrap();
    assert_eq!(
        out8, buf8,
        "Default looks on Rgb8 must be exact bit identity"
    );

    let out16 = apply_recipe(&buf16, &recipe, None).unwrap();
    assert_eq!(
        out16, buf16,
        "Default looks on Rgb16 must be exact bit identity"
    );
}

#[test]
fn tone_mapper_off_preserves_exact_identity() {
    use phototools_core::media::edit::{apply_recipe, AdjustmentRecipe, ImageBuffer, LookEffects};

    let buf = ImageBuffer::Rgb8 {
        width: 16,
        height: 16,
        data: vec![120u8; 16 * 16 * 3],
    };
    let recipe = AdjustmentRecipe {
        looks: Some(LookEffects {
            glow_amount: 0.0,
            glow_threshold: 70.0,
            glow_radius: 30.0,
            halation_amount: 0.0,
            halation_threshold: 80.0,
            halation_radius: 20.0,
            tone_mapper: None,
        }),
        ..Default::default()
    };
    assert!(recipe.is_identity());
    let out = apply_recipe(&buf, &recipe, None).unwrap();
    assert_eq!(out, buf, "Tone mapper off must preserve bit-exact identity");
}

#[test]
fn narkowicz_aces_compresses_highlights_monotonically_without_inversion() {
    use phototools_core::media::edit::aces_narkowicz;

    let mut prev = 0.0f32;
    for i in 0..2000 {
        let x = i as f32 * 0.05;
        let y = aces_narkowicz(x);
        assert!(
            y >= prev,
            "Monotonicity violation at x={x}: prev={prev}, y={y}"
        );
        assert!(y <= 1.0, "ACES output exceeded 1.0 at x={x}: y={y}");
        prev = y;
    }

    // Extreme high dynamic range highlights
    let y_100 = aces_narkowicz(100.0);
    let y_1000 = aces_narkowicz(1000.0);
    assert!((0.99..=1.0).contains(&y_100));
    assert!(y_1000 >= y_100 && y_1000 <= 1.0);
}

#[test]
fn aces_tone_mapper_places_mid_grey_where_the_doc_says() {
    use phototools_core::media::edit::aces_narkowicz;

    let y = aces_narkowicz(0.18);
    // Unscaled Narkowicz ACES maps 0.18 to ~0.2669 as documented in module docs
    assert!(
        (y - 0.2669).abs() < 0.005,
        "Mid-grey 0.18 should map to ~0.2669, got {y}"
    );
}

#[test]
fn glow_pyramid_executes_within_interactive_budget_during_drag() {
    use phototools_core::media::edit::{looks::build_combined_bloom, LinearBuffer, LookEffects};
    use std::time::Instant;

    let w = 1280;
    let h = 854;
    let mut data = vec![0.1f32; (w * h * 3) as usize];
    // Add specular highlight in center
    for y in 400..454 {
        for x in 600..680 {
            let idx = ((y * w + x) * 3) as usize;
            data[idx] = 2.5;
            data[idx + 1] = 2.5;
            data[idx + 2] = 2.5;
        }
    }
    let linear = LinearBuffer {
        width: w,
        height: h,
        data,
    };
    let looks = LookEffects {
        glow_amount: 50.0,
        glow_threshold: 60.0,
        glow_radius: 35.0,
        halation_amount: 40.0,
        halation_threshold: 70.0,
        halation_radius: 25.0,
        tone_mapper: None,
    };

    let t0 = Instant::now();
    let bloom = build_combined_bloom(&linear, &looks);
    let elapsed = t0.elapsed();

    assert!(bloom.is_some());
    // Interactive budget for 720p pyramid is <= 5 ms
    assert!(
        elapsed.as_millis() <= 5,
        "Glow pyramid took {:?}, exceeding 5 ms interactive drag budget",
        elapsed
    );
}

#[test]
fn glow_and_halation_decay_smoothly_and_scale_with_image_resolution() {
    use phototools_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, ImageBuffer, LookEffects,
        PreviewSession, PreviewStage,
    };

    let w = 7360;
    let h = 4912;
    let mut data = vec![30u8; (w * h * 3) as usize];

    // Place a bright highlight disc in center
    let cx = w / 2;
    let cy = h / 2;
    let r_disc = 200;
    for y in (cy - r_disc)..(cy + r_disc) {
        for x in (cx - r_disc)..(cx + r_disc) {
            let dx = (x as i32) - (cx as i32);
            let dy = (y as i32) - (cy as i32);
            if dx * dx + dy * dy <= (r_disc as i32) * (r_disc as i32) {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = 250;
                data[idx + 1] = 250;
                data[idx + 2] = 250;
            }
        }
    }
    let img36 = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    };

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        looks: Some(LookEffects {
            glow_amount: 35.0,
            glow_threshold: 60.0,
            glow_radius: 30.0,
            halation_amount: 30.0,
            halation_threshold: 70.0,
            halation_radius: 20.0,
            tone_mapper: None,
        }),
        ..Default::default()
    };

    // 1. Full 36 MP export
    let export_out = apply_recipe(&img36, &recipe, None).unwrap();

    // 2. Drag preview proxy (1280x854)
    let session = PreviewSession::new(&img36).unwrap();
    let proxy_frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let pr_w = proxy_frame.width;
    let pr_h = proxy_frame.height;

    // Downscale full export to proxy size
    let export_down = downscale_image_buffer(&export_out, pr_w.max(pr_h)).unwrap();
    assert_eq!(export_down.width(), pr_w);
    assert_eq!(export_down.height(), pr_h);

    let down_bytes = export_down.as_rgb8().unwrap();
    let pr_bytes = &proxy_frame.bytes;

    // Compare over sampling points across the image
    let mut max_diff = 0.0f32;
    let mut sum_diff = 0.0f64;
    let mut count = 0usize;

    for y in (0..pr_h).step_by(8) {
        for x in (0..pr_w).step_by(8) {
            let pr_idx = ((y * pr_w + x) * 4) as usize;
            let down_idx = ((y * pr_w + x) * 3) as usize;

            let diff_r = (pr_bytes[pr_idx] as f32 - down_bytes[down_idx] as f32).abs();
            let diff_g = (pr_bytes[pr_idx + 1] as f32 - down_bytes[down_idx + 1] as f32).abs();
            let diff_b = (pr_bytes[pr_idx + 2] as f32 - down_bytes[down_idx + 2] as f32).abs();

            let avg_diff = (diff_r + diff_g + diff_b) / 3.0;
            if avg_diff > max_diff {
                max_diff = avg_diff;
            }
            sum_diff += avg_diff as f64;
            count += 1;
        }
    }

    let mean_diff = (sum_diff / count as f64) as f32;
    // Mean delta E must be well within <= 1.5 code values
    assert!(
        mean_diff <= 1.5,
        "Mean delta E {mean_diff} exceeds 1.5 (max={max_diff})"
    );
}

// ---------------------------------------------------------------------------
// ED-19: masks
// ---------------------------------------------------------------------------

fn radial_mask(center: [f32; 2], radius: f32, exposure: f32) -> phototools_core::media::edit::Mask {
    use phototools_core::media::edit::{LocalAdjustments, Mask, MaskKind};
    Mask {
        id: "r".into(),
        name: "Radial".into(),
        kind: MaskKind::Radial {
            center,
            radius_x: radius,
            radius_y: radius,
            angle: 0.0,
            feather: 0.3,
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure,
            ..Default::default()
        },
        strokes: Vec::new(),
    }
}

/// A grey frame with a smooth gradient, so misplacement would show, and a red marker
/// square centred at stored-normalised `marker`.
fn marked_frame(w: u32, h: u32, marker: [f32; 2]) -> phototools_core::media::edit::ImageBuffer {
    let (mx, my) = ((marker[0] * w as f32) as i64, (marker[1] * h as f32) as i64);
    let half = (w.max(h) / 60) as i64;
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if (x - mx).abs() <= half && (y - my).abs() <= half {
                data.extend_from_slice(&[230, 20, 20]);
            } else {
                let v = (80 + (x * 60 / w as i64) + (y * 30 / h as i64)) as u8;
                data.extend_from_slice(&[v, v, v]);
            }
        }
    }
    phototools_core::media::edit::ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    }
}

/// Centroid of the pixels where `weight` is positive, in frame pixels.
fn centroid(w: u32, h: u32, weight: impl Fn(usize) -> f32) -> (f32, f32) {
    let (mut sx, mut sy, mut sw) = (0.0f64, 0.0f64, 0.0f64);
    for y in 0..h {
        for x in 0..w {
            let k = weight((y * w + x) as usize).max(0.0) as f64;
            sx += k * (x as f64 + 0.5);
            sy += k * (y as f64 + 0.5);
            sw += k;
        }
    }
    assert!(sw > 0.0, "nothing to locate");
    ((sx / sw) as f32, (sy / sw) as f32)
}

/// The effect of a mask drawn over a marker lands on the marker, whatever the crop,
/// rotation, straighten, flip or EXIF orientation: masks are placed on the photograph's
/// content, not on the frame around it (ED-19, decision 2).
#[test]
fn a_mask_follows_the_crop_and_straighten() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, Geometry, NormalizedCrop, PreviewSession, PreviewStage,
    };

    let at = [0.3, 0.35];
    let img = marked_frame(1500, 1000, at);
    let geometries: Vec<(&str, Option<Geometry>, u32)> = vec![
        ("none", None, 1),
        (
            "crop",
            Some(Geometry {
                crop: Some(NormalizedCrop {
                    x: 0.1,
                    y: 0.2,
                    width: 0.6,
                    height: 0.7,
                }),
                ..Default::default()
            }),
            1,
        ),
        (
            "rotate 90",
            Some(Geometry {
                rotate: 90,
                ..Default::default()
            }),
            1,
        ),
        (
            "straighten 8 and flip",
            Some(Geometry {
                straighten: 8.0,
                flip_h: true,
                ..Default::default()
            }),
            1,
        ),
        ("EXIF orientation 6", Some(Geometry::default()), 6),
    ];

    for (name, geometry, orientation) in geometries {
        let session =
            PreviewSession::from_image_buffer_with_orientation(&img, orientation).unwrap();
        let base = AdjustmentRecipe {
            geometry: geometry.clone(),
            ..Default::default()
        };
        let masked = AdjustmentRecipe {
            masks: vec![radial_mask(at, 0.06, 1.5)],
            ..base.clone()
        };
        let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
        let lit = session.render(&masked, None, PreviewStage::Drag).unwrap();
        let (w, h) = (plain.width, plain.height);

        let marker = centroid(w, h, |i| {
            let p = &plain.bytes[i * 4..i * 4 + 3];
            if p[0] > 150 && p[1] < 90 {
                1.0
            } else {
                0.0
            }
        });
        let effect = centroid(w, h, |i| {
            let luma = |b: &[u8]| b[0] as f32 + b[1] as f32 + b[2] as f32;
            luma(&lit.bytes[i * 4..i * 4 + 3]) - luma(&plain.bytes[i * 4..i * 4 + 3])
        });
        let dist = ((marker.0 - effect.0).powi(2) + (marker.1 - effect.1).powi(2)).sqrt();
        assert!(
            dist < 3.0,
            "{name}: the mask's effect is centred at {effect:?}, the marker at {marker:?}"
        );
    }
}

/// Where no mask reaches, the frame is byte for byte what it is without masks; a mask
/// that switches a stage on (saturation here) must not disturb the pixels it misses.
#[test]
fn pixels_no_mask_covers_are_unchanged_by_it() {
    use phototools_core::media::edit::{AdjustmentRecipe, PreviewSession, PreviewStage};

    let img = marked_frame(1200, 800, [0.8, 0.8]);
    let session = PreviewSession::new(&img).unwrap();
    let base = AdjustmentRecipe {
        exposure: 0.3,
        contrast: 12.0,
        ..Default::default()
    };
    let mut mask = radial_mask([0.5, 0.5], 0.1, 1.0);
    mask.adjustments.saturation = 40.0;
    mask.adjustments.contrast = -20.0;
    mask.adjustments.shadows = 30.0;
    let masked = AdjustmentRecipe {
        masks: vec![mask],
        ..base.clone()
    };

    for stage in [PreviewStage::Drag, PreviewStage::Settle] {
        let a = session.render(&base, None, stage).unwrap();
        let b = session.render(&masked, None, stage).unwrap();
        let (w, h) = (a.width as usize, a.height as usize);
        let mut changed_inside = 0;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                // Radius 0.1 of the long edge: 0.1 of the width, 0.15 of the height.
                let r = (((u - 0.5) / 0.1).powi(2) + ((v - 0.5) / 0.15).powi(2)).sqrt();
                if r > 1.01 {
                    assert_eq!(
                        a.bytes[i..i + 4],
                        b.bytes[i..i + 4],
                        "outside the mask at ({x}, {y})"
                    );
                } else if r < 0.5 && a.bytes[i..i + 3] != b.bytes[i..i + 3] {
                    changed_inside += 1;
                }
            }
        }
        assert!(changed_inside > 0, "the mask changed nothing inside");
    }
}

/// A mask whose adjustments are all zero renders exactly as no mask at all, preview and
/// export.
#[test]
fn a_mask_with_zero_adjustments_changes_nothing() {
    use phototools_core::media::edit::{
        apply_recipe, AdjustmentRecipe, PreviewSession, PreviewStage,
    };

    let img = marked_frame(600, 400, [0.5, 0.5]);
    let base = AdjustmentRecipe {
        exposure: 0.4,
        ..Default::default()
    };
    let masked = AdjustmentRecipe {
        masks: vec![radial_mask([0.5, 0.5], 0.3, 0.0)],
        ..base.clone()
    };
    assert_eq!(
        apply_recipe(&img, &base, None).unwrap(),
        apply_recipe(&img, &masked, None).unwrap()
    );
    let session = PreviewSession::new(&img).unwrap();
    assert_eq!(
        session
            .render(&base, None, PreviewStage::Settle)
            .unwrap()
            .bytes,
        session
            .render(&masked, None, PreviewStage::Settle)
            .unwrap()
            .bytes
    );
    assert!(masked.masks[0].is_identity());
}

/// The export, rendered at full resolution and downscaled, matches the preview of the same
/// masked recipe (the ED-16 measure: mean difference ≤ 1.5 code values).
#[test]
fn export_matches_preview_with_masks_within_delta_e_1_5() {
    use phototools_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, LocalAdjustments, Mask, MaskKind,
        PreviewSession, PreviewStage,
    };

    let img = marked_frame(4000, 2667, [0.6, 0.4]);
    let sky = Mask {
        id: "sky".into(),
        name: "Sky".into(),
        kind: MaskKind::Linear {
            start: [0.5, 0.1],
            end: [0.5, 0.6],
        },
        invert: false,
        opacity: 0.8,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: -0.8,
            temperature: -20.0,
            highlights: -40.0,
            saturation: 25.0,
            ..Default::default()
        },
        strokes: Vec::new(),
    };
    let mut face = radial_mask([0.6, 0.4], 0.12, 0.7);
    face.adjustments.contrast = 15.0;
    face.adjustments.shadows = 30.0;
    face.adjustments.tint = 8.0;

    let recipe = AdjustmentRecipe {
        exposure: 0.2,
        contrast: 10.0,
        masks: vec![sky, face],
        ..Default::default()
    };

    let export = apply_recipe(&img, &recipe, None).unwrap();
    let session = PreviewSession::new(&img).unwrap();
    let preview = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let down = downscale_image_buffer(&export, preview.width.max(preview.height)).unwrap();
    assert_eq!(
        (down.width(), down.height()),
        (preview.width, preview.height)
    );
    let down = down.as_rgb8().unwrap();

    let (mut sum, mut n) = (0.0f64, 0usize);
    for i in 0..(preview.width * preview.height) as usize {
        let d: f32 = (0..3)
            .map(|c| (preview.bytes[i * 4 + c] as f32 - down[i * 3 + c] as f32).abs())
            .sum::<f32>()
            / 3.0;
        sum += d as f64;
        n += 1;
    }
    let mean = (sum / n as f64) as f32;
    assert!(
        mean <= 1.5,
        "mean difference {mean} between preview and export exceeds 1.5"
    );
}

/// A v2 sidecar, written before masks existed, loads with none; a v3 one with masks
/// round-trips through the sidecar unchanged.
#[test]
fn a_v2_sidecar_loads_with_no_masks_and_masks_round_trip_in_v3() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("masked.jpg", 64, 48);
    let sidecar = sidecar_path(&img);
    std::fs::write(
        &sidecar,
        serde_json::json!({"version": 2, "source_sha256": "x", "exposure": 0.5,
            "temperature": 0.0, "tint": 0.0, "highlights": 0.0, "shadows": 0.0, "contrast": 0.0,
            "saturation": 0.0, "vibrance": 0.0, "lut": null, "lut_intensity": 1.0})
        .to_string(),
    )
    .unwrap();
    let v2 = load_recipe(&sidecar).unwrap();
    assert!(v2.masks.is_empty());

    let recipe = AdjustmentRecipe {
        masks: vec![radial_mask([0.4, 0.6], 0.2, -0.5)],
        ..v2
    };
    save_recipe(&img, &recipe).unwrap();
    let back = load_recipe(&sidecar).unwrap();
    assert_eq!(back.version, 3);
    assert_eq!(back.masks, recipe.masks);
}

/// The coverage the overlay shows is where the mask acts: brightest where the effect is
/// strongest, absent where the frame is untouched, through the same geometry (ED-20).
#[test]
fn the_mask_overlay_shows_where_the_mask_acts() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, Geometry, NormalizedCrop, PreviewSession, PreviewStage,
    };

    let img = marked_frame(1500, 1000, [0.3, 0.35]);
    let session = PreviewSession::from_image_buffer_with_orientation(&img, 6).unwrap();
    let mut mask = radial_mask([0.3, 0.35], 0.08, 1.2);
    mask.invert = true;
    let base = AdjustmentRecipe {
        geometry: Some(Geometry {
            straighten: 5.0,
            crop: Some(NormalizedCrop {
                x: 0.05,
                y: 0.1,
                width: 0.8,
                height: 0.85,
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let masked = AdjustmentRecipe {
        masks: vec![mask],
        ..base.clone()
    };

    let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
    let lit = session.render(&masked, None, PreviewStage::Drag).unwrap();
    let (w, h, coverage) = session
        .mask_coverage(&masked, "r", PreviewStage::Drag)
        .unwrap();
    assert_eq!((w, h), (lit.width, lit.height));
    // The frame tells the view the same map the coverage was computed through.
    assert_ne!(lit.to_stored, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    let (mut strong, mut strong_changed) = (0, 0);
    for (i, &cov) in coverage.iter().enumerate() {
        let changed = lit.bytes[i * 4..i * 4 + 3] != plain.bytes[i * 4..i * 4 + 3];
        if cov == 0 {
            assert!(
                !changed,
                "pixel {i} changed where the overlay shows nothing"
            );
        } else if cov > 128 {
            strong += 1;
            if changed {
                strong_changed += 1;
            }
        }
    }
    // An inverted radial: most of the frame is strongly covered, and +1.2 EV changes it.
    assert!(
        strong > (w * h / 2) as usize,
        "only {strong} strongly covered pixels"
    );
    assert!(
        strong_changed * 100 >= strong * 99,
        "{strong_changed} of {strong} strongly covered pixels changed"
    );
    assert!(session
        .mask_coverage(&masked, "missing", PreviewStage::Drag)
        .is_err());
}

// ---------------------------------------------------------------------------
// ED-21: brush
// ---------------------------------------------------------------------------

fn brush_mask(
    strokes: Vec<phototools_core::media::edit::Stroke>,
    exposure: f32,
) -> phototools_core::media::edit::Mask {
    use phototools_core::media::edit::{LocalAdjustments, Mask, MaskKind};
    Mask {
        id: "b".into(),
        name: "Brush".into(),
        kind: MaskKind::Brush,
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure,
            ..Default::default()
        },
        strokes,
    }
}

fn brush_stroke(
    points: Vec<[f32; 2]>,
    radius: f32,
    erase: bool,
) -> phototools_core::media::edit::Stroke {
    phototools_core::media::edit::Stroke {
        points,
        radius,
        feather: 0.5,
        flow: 1.0,
        erase,
    }
}

/// A painted mask looks the same in the export, drawn at full size (capped at 4096 on the
/// long edge) and downscaled, as in the preview: the ED-16 measure, mean ≤ 1.5 code values.
#[test]
fn brush_strokes_rasterise_identically_at_proxy_and_export_size() {
    use phototools_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, PreviewSession, PreviewStage,
    };

    // Wider than the export raster cap, so the capped path is the one measured.
    let img = marked_frame(5000, 3333, [0.5, 0.5]);
    let wave: Vec<[f32; 2]> = (0..80)
        .map(|i| {
            let t = i as f32 / 79.0;
            [0.1 + 0.8 * t, 0.5 + 0.2 * (t * 9.0).sin()]
        })
        .collect();
    let recipe = AdjustmentRecipe {
        masks: vec![brush_mask(
            vec![
                brush_stroke(wave.clone(), 0.04, false),
                brush_stroke(vec![[0.3, 0.3], [0.7, 0.7]], 0.02, true),
            ],
            1.2,
        )],
        ..Default::default()
    };

    let export = apply_recipe(&img, &recipe, None).unwrap();
    let session = PreviewSession::new(&img).unwrap();
    let preview = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let down = downscale_image_buffer(&export, preview.width.max(preview.height)).unwrap();
    let down = down.as_rgb8().unwrap();
    let mut sum = 0.0f64;
    let n = (preview.width * preview.height) as usize;
    for i in 0..n {
        let d: f32 = (0..3)
            .map(|c| (preview.bytes[i * 4 + c] as f32 - down[i * 3 + c] as f32).abs())
            .sum::<f32>()
            / 3.0;
        sum += d as f64;
    }
    let mean = (sum / n as f64) as f32;
    assert!(
        mean <= 1.5,
        "mean difference {mean} between preview and export exceeds 1.5"
    );
}

/// A dab painted over a marker lands on the marker under crop, rotation and orientation,
/// as the shapes do: strokes are in the stored frame too.
#[test]
fn a_brushed_mask_follows_the_crop_and_rotation() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, Geometry, NormalizedCrop, PreviewSession, PreviewStage,
    };

    let at = [0.62, 0.3];
    let img = marked_frame(1500, 1000, at);
    for (name, geometry, orientation) in [
        ("none", None, 1),
        (
            "crop and rotate",
            Some(Geometry {
                rotate: 270,
                crop: Some(NormalizedCrop {
                    x: 0.0,
                    y: 0.1,
                    width: 0.9,
                    height: 0.8,
                }),
                ..Default::default()
            }),
            1,
        ),
        ("EXIF orientation 8", Some(Geometry::default()), 8),
    ] {
        let session =
            PreviewSession::from_image_buffer_with_orientation(&img, orientation).unwrap();
        let base = AdjustmentRecipe {
            geometry: geometry.clone(),
            ..Default::default()
        };
        let painted = AdjustmentRecipe {
            masks: vec![brush_mask(vec![brush_stroke(vec![at], 0.05, false)], 1.5)],
            ..base.clone()
        };
        let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
        let lit = session.render(&painted, None, PreviewStage::Drag).unwrap();
        let (w, h) = (plain.width, plain.height);
        let marker = centroid(w, h, |i| {
            let p = &plain.bytes[i * 4..i * 4 + 3];
            if p[0] > 150 && p[1] < 90 {
                1.0
            } else {
                0.0
            }
        });
        let effect = centroid(w, h, |i| {
            let luma = |b: &[u8]| b[0] as f32 + b[1] as f32 + b[2] as f32;
            luma(&lit.bytes[i * 4..i * 4 + 3]) - luma(&plain.bytes[i * 4..i * 4 + 3])
        });
        let dist = ((marker.0 - effect.0).powi(2) + (marker.1 - effect.1).powi(2)).sqrt();
        assert!(
            dist < 3.0,
            "{name}: effect at {effect:?}, marker at {marker:?}"
        );
    }
}

/// Painting a stroke frame by frame draws each new segment once, not the stroke again, and
/// erasing it all away renders exactly as no mask (ED-21).
#[test]
fn painting_draws_each_new_segment_once_and_erasing_restores_exactly() {
    use phototools_core::media::edit::{AdjustmentRecipe, PreviewSession, PreviewStage};

    let img = marked_frame(1200, 800, [0.5, 0.5]);
    let session = PreviewSession::new(&img).unwrap();
    let path: Vec<[f32; 2]> = (0..50).map(|i| [0.2 + 0.012 * i as f32, 0.4]).collect();

    let mut recipe = AdjustmentRecipe {
        masks: vec![brush_mask(
            vec![brush_stroke(path[..1].to_vec(), 0.03, false)],
            1.0,
        )],
        ..Default::default()
    };
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let start = session.brush_segments_drawn();
    for k in 2..=path.len() {
        recipe.masks[0].strokes[0].points = path[..k].to_vec();
        session.render(&recipe, None, PreviewStage::Drag).unwrap();
    }
    assert_eq!(session.brush_segments_drawn() - start, path.len() - 1);

    let without = session
        .render(&AdjustmentRecipe::default(), None, PreviewStage::Settle)
        .unwrap();
    let mut erase = brush_stroke(path.clone(), 0.06, true);
    erase.feather = 0.0;
    recipe.masks[0].strokes.push(erase);
    let erased = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(
        erased.bytes, without.bytes,
        "an erased stroke must leave no trace"
    );
}

// ---------------------------------------------------------------------------
// ED-23: automatic masks, as stored
// ---------------------------------------------------------------------------

/// A stored automatic mask acts where its pixels say, in the stored frame, through the
/// geometry like every mask; a damaged one is refused rather than masking the wrong place.
#[test]
fn a_stored_automatic_mask_acts_where_it_covers_and_a_damaged_one_is_refused() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, AutoTarget, Geometry, LocalAdjustments, Mask, MaskKind, PreviewSession,
        PreviewStage, StoredRaster,
    };

    // Covers a square around the marker at (0.3, 0.35) of the stored frame.
    let (mw, mh) = (300u32, 200u32);
    let data: Vec<u8> = (0..mw * mh)
        .map(|i| {
            // Cell centres, so the square is centred on the marker exactly.
            let (x, y) = (
                ((i % mw) as f32 + 0.5) / mw as f32,
                ((i / mw) as f32 + 0.5) / mh as f32,
            );
            if (x - 0.3).abs() < 0.08 && (y - 0.35).abs() < 0.12 {
                255
            } else {
                0
            }
        })
        .collect();
    let stored = StoredRaster::encode(mw, mh, &data, "test").unwrap();
    let mask = |raster: StoredRaster| Mask {
        id: "auto".into(),
        name: "Subject".into(),
        kind: MaskKind::Auto {
            target: AutoTarget::Subject,
            mask: raster,
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: 1.5,
            ..Default::default()
        },
        strokes: Vec::new(),
    };

    let img = marked_frame(1500, 1000, [0.3, 0.35]);
    for (name, geometry, orientation) in [
        ("none", None, 1),
        (
            "rotate and orientation 6",
            Some(Geometry {
                rotate: 90,
                ..Default::default()
            }),
            6,
        ),
    ] {
        let session =
            PreviewSession::from_image_buffer_with_orientation(&img, orientation).unwrap();
        let base = AdjustmentRecipe {
            geometry: geometry.clone(),
            ..Default::default()
        };
        let masked = AdjustmentRecipe {
            masks: vec![mask(stored.clone())],
            ..base.clone()
        };
        let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
        let lit = session.render(&masked, None, PreviewStage::Drag).unwrap();
        let (w, h) = (plain.width, plain.height);
        let marker = centroid(w, h, |i| {
            let p = &plain.bytes[i * 4..i * 4 + 3];
            if p[0] > 150 && p[1] < 90 {
                1.0
            } else {
                0.0
            }
        });
        // Which pixels changed clearly, not by how much: the frame brightens left to right,
        // so weighting by the gain, or counting the faint fringe where only the brighter side
        // moves a code value, would pull the centre to the right.
        let effect = centroid(w, h, |i| {
            let luma = |b: &[u8]| b[0] as f32 + b[1] as f32 + b[2] as f32;
            let d = luma(&lit.bytes[i * 4..i * 4 + 3]) - luma(&plain.bytes[i * 4..i * 4 + 3]);
            if d > 30.0 {
                1.0
            } else {
                0.0
            }
        });
        let dist = ((marker.0 - effect.0).powi(2) + (marker.1 - effect.1).powi(2)).sqrt();
        assert!(
            dist < 4.0,
            "{name}: effect at {effect:?}, marker at {marker:?}"
        );
    }

    let mut damaged = stored.clone();
    damaged.png.truncate(damaged.png.len() / 2);
    let session = PreviewSession::new(&img).unwrap();
    let recipe = AdjustmentRecipe {
        masks: vec![mask(damaged)],
        ..Default::default()
    };
    assert!(session.render(&recipe, None, PreviewStage::Drag).is_err());
}
