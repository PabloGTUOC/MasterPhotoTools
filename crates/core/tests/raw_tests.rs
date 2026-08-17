//! Phase 10 acceptance: RAW to JPEG (F14).

mod fixtures;

use chrono::Datelike;
use fixtures::Fixtures;
use phototools_core::config::Thresholds;
use phototools_core::ingest::derivation::{derive_batch, derive_batch_with, DerivationRequest};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::raw::{
    default_ladder, largest_embedded_jpeg, raw_to_jpeg, run_ladder, RawSource,
};
use phototools_core::media::read_meta;

fn progress() -> InMemoryProgress {
    InMemoryProgress::new()
}

// ---------------------------------------------------------------------------
// Rung 1 — the embedded preview
// ---------------------------------------------------------------------------

#[test]
fn the_embedded_preview_is_extracted_byte_for_byte() {
    // F14 rung 1: the camera's own render, "effectively free to extract". Free
    // means exactly that — the bytes come out unchanged, not re-encoded.
    let f = Fixtures::new();
    let (raw, expected) = f.raw_stub_with_preview("shot.nef", 800, 600);

    let derived = raw_to_jpeg(&raw).unwrap();

    assert_eq!(derived.source, RawSource::EmbeddedPreview);
    assert_eq!(
        derived.bytes, expected,
        "the preview must be sliced out, not re-encoded"
    );
    assert_eq!((derived.width, derived.height), (800, 600));
}

#[test]
fn the_largest_preview_wins_over_the_thumbnail() {
    // A RAW carries several JPEGs: a 160×120 index thumbnail and the
    // full-resolution render. Only the latter is worth publishing.
    let f = Fixtures::new();
    let (raw, full_size) = f.raw_with_thumbnail_and_preview("shot.arw");

    let derived = raw_to_jpeg(&raw).unwrap();

    assert_eq!(derived.source, RawSource::EmbeddedPreview);
    assert_eq!((derived.width, derived.height), (1600, 1200));
    assert_eq!(derived.bytes, full_size);
}

#[test]
fn a_raw_with_no_preview_falls_through_rung_one() {
    // The extractor must decline rather than invent something.
    let f = Fixtures::new();
    let raw = f.raw_without_preview("bare.nef", 6000, 4000);
    let bytes = std::fs::read(&raw).unwrap();

    assert!(
        largest_embedded_jpeg(&bytes).is_none(),
        "there is no preview to find"
    );
}

// ---------------------------------------------------------------------------
// Ladder order
// ---------------------------------------------------------------------------

#[test]
fn with_a_preview_present_the_later_rungs_are_never_reached() {
    // The acceptance criterion. The real default ladder is used, and the proof
    // is that a stub no other decoder could read still converts: rungs 2 and 3
    // would both fail on it.
    let f = Fixtures::new();
    let (raw, _) = f.raw_stub_with_preview("shot.cr2", 400, 300);

    let derived = run_ladder(&raw, &default_ladder()).unwrap();

    assert_eq!(
        derived.source,
        RawSource::EmbeddedPreview,
        "rung 1 answers, so rungs 2 and 3 have nothing to do"
    );
}

#[test]
fn a_file_no_rung_can_handle_reports_what_each_rung_said() {
    // Rather than "conversion failed", which tells nobody anything.
    let f = Fixtures::new();
    let bogus = f.path().join("bogus.nef");
    std::fs::write(&bogus, b"this is not a RAW file").unwrap();

    let err = raw_to_jpeg(&bogus).unwrap_err();
    let message = err.to_string();

    assert!(message.contains("bogus.nef"), "{message}");
    assert!(message.contains("embedded preview"), "{message}");
    assert!(message.contains("rawler"), "{message}");
}

// ---------------------------------------------------------------------------
// Metadata survives into the output
// ---------------------------------------------------------------------------

#[test]
fn capture_date_and_camera_survive_from_the_raw_into_the_jpeg() {
    // The acceptance criterion, and the reason it matters: without the capture
    // date Google Photos files the photograph under its upload date.
    //
    // The fixture's *preview* deliberately carries no metadata, so a date found
    // in the output can only have come from the copy step.
    let f = Fixtures::new();
    let raw = f.raw_with_metadata(
        "shot.nef",
        800,
        600,
        "2024:05:30 14:22:11",
        "STUBCAM X1",
        "STUB 35mm f/1.4",
    );

    // The premise: the extracted preview has no date of its own.
    let extracted = raw_to_jpeg(&raw).unwrap();
    let bare = f.path().join("bare.jpg");
    std::fs::write(&bare, &extracted.bytes).unwrap();
    assert!(
        read_meta(&bare).unwrap().capture.is_none(),
        "the preview must start with no date, or this test proves nothing"
    );

    let out = f.path().join("derived");
    let summary = derive_batch(
        &[DerivationRequest {
            source: raw.clone(),
            stem: "IMG_0001".into(),
        }],
        &out,
        &Thresholds::default(),
        &progress(),
    )
    .unwrap();

    assert_eq!(summary.derived.len(), 1, "{:?}", summary.failures);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);

    let shot = &summary.derived[0];
    assert_eq!(shot.rung, RawSource::EmbeddedPreview);
    assert!(
        shot.metadata_verified,
        "F14 requires the metadata to be copied, and §9.2 requires it verified"
    );

    let meta = read_meta(&shot.output).unwrap();
    let capture = meta.capture.expect("the capture date must survive");
    assert_eq!(capture.year(), 2024);
    assert_eq!(capture.month(), 5);
    assert_eq!(capture.day(), 30);
    assert_eq!(capture.format("%H:%M:%S").to_string(), "14:22:11");
    assert_eq!(meta.camera.as_deref(), Some("STUBCAM X1"));
}

#[test]
fn the_lens_survives_too() {
    // Read through exiftool rather than `read_meta`, which does not surface
    // LensModel — the requirement is that the tag reaches the file, and this
    // asserts the file, not our reader.
    let f = Fixtures::new();
    let raw = f.raw_with_metadata(
        "shot.nef",
        400,
        300,
        "2024:05:30 14:22:11",
        "STUBCAM X1",
        "STUB 35mm f/1.4",
    );

    let out = f.path().join("derived");
    let summary = derive_batch(
        &[DerivationRequest {
            source: raw,
            stem: "IMG_0001".into(),
        }],
        &out,
        &Thresholds::default(),
        &progress(),
    )
    .unwrap();

    let written = &summary.derived[0].output;
    let reported = std::process::Command::new("exiftool")
        .arg("-s3")
        .arg("-LensModel")
        .arg(written)
        .output()
        .expect("exiftool is a required dependency (specification §2.6)");

    let lens = String::from_utf8_lossy(&reported.stdout).trim().to_string();
    assert_eq!(lens, "STUB 35mm f/1.4");
}

// ---------------------------------------------------------------------------
// The output passes through F12 and F13
// ---------------------------------------------------------------------------

#[test]
fn a_derivative_over_the_ceiling_is_resized_and_keeps_its_date() {
    // F12: the megapixel ceiling applies to "both the JPEG path and the
    // RAW-derived path". A 24 MP preview must not sail past it.
    let f = Fixtures::new();
    let raw = f.raw_with_metadata(
        "big.nef",
        6000,
        4000,
        "2024:05:30 14:22:11",
        "STUBCAM X1",
        "STUB 35mm f/1.4",
    );

    let out = f.path().join("derived");
    let summary = derive_batch(
        &[DerivationRequest {
            source: raw,
            stem: "IMG_0001".into(),
        }],
        &out,
        &Thresholds::default(),
        &progress(),
    )
    .unwrap();

    let shot = &summary.derived[0];
    assert!(shot.resized, "24 MP is over the 10 MP ceiling");
    assert!(
        (shot.width as u64 * shot.height as u64) <= 10_000_000,
        "{}×{} is still over",
        shot.width,
        shot.height
    );

    // Aspect ratio held, and the date survived the resize as well as the copy.
    let ratio = shot.width as f64 / shot.height as f64;
    assert!((ratio - 1.5).abs() < 0.01, "6000×4000 is 3:2, got {ratio}");
    assert!(shot.metadata_verified);
    assert!(read_meta(&shot.output).unwrap().capture.is_some());
}

#[test]
fn a_derivative_within_the_ceiling_is_not_re_encoded() {
    // The camera's own render is the best version there is; re-encoding it to
    // no purpose would cost quality for nothing.
    let f = Fixtures::new();
    let raw = f.raw_with_metadata(
        "small.nef",
        800,
        600,
        "2024:05:30 14:22:11",
        "STUBCAM X1",
        "STUB 35mm f/1.4",
    );
    let extracted = raw_to_jpeg(&raw).unwrap();

    let out = f.path().join("derived");
    let summary = derive_batch(
        &[DerivationRequest {
            source: raw,
            stem: "IMG_0001".into(),
        }],
        &out,
        &Thresholds::default(),
        &progress(),
    )
    .unwrap();

    let shot = &summary.derived[0];
    assert!(!shot.resized);
    assert_eq!((shot.width, shot.height), (800, 600));

    // The pixels are the preview's. The file differs only by the metadata that
    // was spliced in, so it is at least as large.
    let written = std::fs::read(&shot.output).unwrap();
    assert!(
        written.len() >= extracted.bytes.len(),
        "the output should be the preview plus metadata, not a re-encode"
    );
}

// ---------------------------------------------------------------------------
// Batching
// ---------------------------------------------------------------------------

#[test]
fn a_batch_derives_every_shot_and_verifies_each_one() {
    let f = Fixtures::new();
    let mut requests = Vec::new();

    for i in 0..12 {
        let raw = f.raw_with_metadata(
            &format!("shot{i}.nef"),
            400,
            300,
            &format!("2024:05:30 14:{:02}:11", i),
            "STUBCAM X1",
            "STUB 35mm f/1.4",
        );
        requests.push(DerivationRequest {
            source: raw,
            stem: format!("IMG_{i:04}"),
        });
    }

    let out = f.path().join("derived");
    let summary = derive_batch(&requests, &out, &Thresholds::default(), &progress()).unwrap();

    assert_eq!(summary.derived.len(), 12, "{:?}", summary.failures);
    assert!(summary.failures.is_empty());
    assert!(summary.all_metadata_verified());
    assert_eq!(summary.by_rung(RawSource::EmbeddedPreview), 12);

    // Stable order, so the review grid does not shuffle between runs.
    let stems: Vec<&str> = summary.derived.iter().map(|d| d.stem.as_str()).collect();
    let mut sorted = stems.clone();
    sorted.sort();
    assert_eq!(stems, sorted);

    for shot in &summary.derived {
        assert!(shot.output.exists());
        assert!(read_meta(&shot.output).unwrap().capture.is_some());
    }
}

#[test]
fn one_unreadable_raw_does_not_cost_the_others_their_derivation() {
    let f = Fixtures::new();
    let good = f.raw_with_metadata(
        "good.nef",
        400,
        300,
        "2024:05:30 14:22:11",
        "STUBCAM X1",
        "STUB 35mm f/1.4",
    );
    let bad = f.path().join("bad.nef");
    std::fs::write(&bad, b"not a RAW").unwrap();

    let out = f.path().join("derived");
    let summary = derive_batch(
        &[
            DerivationRequest {
                source: good,
                stem: "GOOD".into(),
            },
            DerivationRequest {
                source: bad,
                stem: "BAD".into(),
            },
        ],
        &out,
        &Thresholds::default(),
        &progress(),
    )
    .unwrap();

    assert_eq!(summary.derived.len(), 1);
    assert_eq!(summary.derived[0].stem, "GOOD");
    assert_eq!(summary.failures.len(), 1);
}

// ---------------------------------------------------------------------------
// G4 — one exiftool process for the batch
// ---------------------------------------------------------------------------

#[test]
fn deriving_a_batch_starts_one_exiftool_process_not_one_per_file() {
    // G4. Starting one per file costs 150–250 ms each regardless of size, which
    // on a card of RAW-only frames is more than the decoding.
    //
    // The shim is passed explicitly rather than placed on `PATH`: the harness
    // runs its tests as threads of one process, so a `PATH` override would be
    // seen by every other test running at the same time. That is not a
    // hypothetical — it is what the first version of this test did, and another
    // test's own `exiftool` call was counted here.
    let f = Fixtures::new();
    let counter = f.path().join("exiftool-invocations");

    let shim = f.path().join("counting-exiftool");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho x >> {}\nexec exiftool \"$@\"\n",
            counter.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut requests = Vec::new();
    for i in 0..8 {
        let raw = f.raw_with_metadata(
            &format!("shot{i}.nef"),
            200,
            150,
            "2024:05:30 14:22:11",
            "STUBCAM X1",
            "STUB 35mm f/1.4",
        );
        requests.push(DerivationRequest {
            source: raw,
            stem: format!("IMG_{i:04}"),
        });
    }

    let out = f.path().join("derived");
    let summary = derive_batch_with(
        &requests,
        &out,
        &Thresholds::default(),
        &progress(),
        &shim.to_string_lossy(),
    )
    .unwrap();

    assert_eq!(summary.derived.len(), 8, "{:?}", summary.failures);
    assert!(summary.all_metadata_verified());

    let invocations = std::fs::read_to_string(&counter)
        .unwrap_or_default()
        .lines()
        .count();
    assert_eq!(
        invocations, 1,
        "G4: one persistent process for the batch, not one per file"
    );
}
