//! Integration tests for develop folder operations (DV-2b).

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::PathBuf;

use phototools_core::media::meta::read_meta;
use phototools_core::media::ExifWriter;
use rollchrome_core::media::develop::{prophoto_icc_profile, OutputFormat};
use rollchrome_core::tools::develop::{execute_develop, plan_develop, DevelopItem};
use rollchrome_core::tools::develop_folder::FolderStore;
use support::dng::DngBuilder;

struct TestEnv {
    temp: tempfile::TempDir,
    raw_dir: PathBuf,
    lut_dir: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let raw_dir = temp.path().join("raws");
        let lut_dir = temp.path().join("luts");
        fs::create_dir_all(&raw_dir).expect("create raw_dir");
        fs::create_dir_all(&lut_dir).expect("create lut_dir");

        // Create a simple identity LUT in lut_dir
        let lut_content = "\
TITLE \"Identity\"
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
        fs::write(lut_dir.join("Identity.cube"), lut_content).expect("write LUT");

        Self {
            temp,
            raw_dir,
            lut_dir,
        }
    }

    fn create_dng(&self, name: &str, width: u32, height: u32) -> PathBuf {
        let builder = DngBuilder::new_bayer(width, height);
        let pixels = vec![3000u16; (width * height) as usize];
        let bytes = builder.build(&pixels);
        let path = self.raw_dir.join(name);
        fs::write(&path, bytes).expect("write DNG");
        path
    }
}

#[derive(Default)]
struct NoopProgress;

impl phototools_core::jobs::Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests 13–19
// ---------------------------------------------------------------------------

#[test]
fn a_developed_jpeg_carries_the_originals_date_camera_and_position() {
    let env = TestEnv::new();
    let dng_path = env.create_dng("DSCF0001.dng", 32, 32);

    // Set DateTimeOriginal, Make, Model, and GPS position on the source DNG
    let mut writer = ExifWriter::start().expect("start exiftool");
    writer
        .set_tags(
            &dng_path,
            &[
                "-DateTimeOriginal=2024:06:15 14:30:00".to_string(),
                "-Make=FUJIFILM".to_string(),
                "-Model=X-T5".to_string(),
                "-GPSLatitude=52.531549".to_string(),
                "-GPSLatitudeRef=N".to_string(),
                "-GPSLongitude=3.460808".to_string(),
                "-GPSLongitudeRef=E".to_string(),
                "-GPSAltitude=36.4".to_string(),
                "-GPSAltitudeRef=0".to_string(),
            ],
        )
        .expect("set tags");
    writer.close().expect("close writer");

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Jpeg { quality: 90 },
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["DSCF0001.jpg"]);
    assert!(summary.failures.is_empty());
    assert!(summary.metadata_skipped.is_empty());

    let output_path = store.output_dir().join("DSCF0001.jpg");
    let meta = read_meta(&output_path).expect("read metadata");

    // Capture date
    let capture_str = meta.capture.map(|dt| dt.to_string()).unwrap_or_default();
    assert!(
        capture_str.contains("2024-06-15 14:30:00"),
        "expected capture date 2024-06-15 14:30:00, got {capture_str}"
    );

    // Camera
    let camera_str = meta.camera.unwrap_or_default();
    assert!(
        camera_str.contains("X-T5"),
        "expected camera X-T5, got {camera_str}"
    );

    // Position
    let gps = meta.gps.expect("GPS position carried");
    assert!(
        (gps.lat - 52.531549).abs() < 1e-4,
        "latitude mismatch: {}",
        gps.lat
    );
    assert!(
        (gps.lon - 3.460808).abs() < 1e-4,
        "longitude mismatch: {}",
        gps.lon
    );
}

#[test]
fn a_developed_jpeg_says_srgb() {
    let env = TestEnv::new();
    let dng_path = env.create_dng("DSCF0001.dng", 32, 32);

    let mut writer = ExifWriter::start().expect("start exiftool");
    writer
        .set_tags(&dng_path, &["-ExifIFD:ColorSpace#=65535".to_string()])
        .expect("set ColorSpace");
    writer.close().expect("close writer");

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Jpeg { quality: 90 },
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["DSCF0001.jpg"]);

    let output_path = store.output_dir().join("DSCF0001.jpg");

    // Read ColorSpace using nom-exif (G3)
    let mut parser = nom_exif::MediaParser::new();
    let iter = parser
        .parse_exif(nom_exif::MediaSource::open(&output_path).expect("open media"))
        .expect("parse exif");

    let mut color_space = None;
    for entry in iter {
        if entry.tag().code() == 0xa001 {
            if let Some(val) = entry.value() {
                match val {
                    nom_exif::EntryValue::U16(v) => color_space = Some(*v as u32),
                    nom_exif::EntryValue::U32(v) => color_space = Some(*v),
                    _ => {}
                }
            }
        }
    }

    assert_eq!(
        color_space,
        Some(1),
        "JPEG must report ColorSpace = 1 (sRGB)"
    );
}

#[test]
fn a_developed_tiff_says_uncalibrated_and_has_no_interop_index() {
    let env = TestEnv::new();
    let dng_path = env.create_dng("DSCF0001.dng", 32, 32);

    let mut writer = ExifWriter::start().expect("start exiftool");
    writer
        .set_tags(
            &dng_path,
            &[
                "-ExifIFD:ColorSpace#=1".to_string(),
                "-InteropIFD:InteropIndex=R98".to_string(),
            ],
        )
        .expect("set tags");
    writer.close().expect("close writer");

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Tiff,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["DSCF0001.tif"]);

    let output_path = store.output_dir().join("DSCF0001.tif");

    // 1. Read ColorSpace using nom-exif (G3)
    let mut parser = nom_exif::MediaParser::new();
    let iter = parser
        .parse_exif(nom_exif::MediaSource::open(&output_path).expect("open media"))
        .expect("parse exif");

    let mut color_space = None;
    let mut interop_index_found = false;
    for entry in iter {
        if entry.tag().code() == 0xa001 {
            if let Some(val) = entry.value() {
                match val {
                    nom_exif::EntryValue::U16(v) => color_space = Some(*v as u32),
                    nom_exif::EntryValue::U32(v) => color_space = Some(*v),
                    _ => {}
                }
            }
        }
        // InteropIndex is tag 0x0001 in Interop IFD
        if entry.tag().code() == 0x0001 {
            interop_index_found = true;
        }
    }

    assert_eq!(
        color_space,
        Some(65535),
        "TIFF must report ColorSpace = 65535 (Uncalibrated)"
    );
    assert!(!interop_index_found, "InteropIndex should not be present");

    // 2. Also verify through tiff decoder that InteropIndex (or InteropOffset 0xa005) is absent
    let file_bytes = fs::read(&output_path).expect("read TIFF bytes");
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&file_bytes)).unwrap();
    let interop_tag = decoder
        .find_tag(tiff::tags::Tag::Unknown(0xa005))
        .expect("find tag");
    assert!(
        interop_tag.is_none(),
        "InteropOffset (0xa005) must be deleted in TIFF"
    );
}

#[test]
fn a_developed_tiff_is_not_taken_for_a_dng() {
    let env = TestEnv::new();
    env.create_dng("DSCF0001.dng", 32, 32);

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Tiff,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["DSCF0001.tif"]);

    let output_path = store.output_dir().join("DSCF0001.tif");
    let file_bytes = fs::read(&output_path).expect("read TIFF bytes");
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&file_bytes)).unwrap();

    let dng_version = decoder
        .find_tag(tiff::tags::Tag::Unknown(50706))
        .expect("find DNGVersion tag");
    assert!(
        dng_version.is_none(),
        "TIFF must not carry DNGVersion tag (50706)"
    );

    let dng_backward_version = decoder
        .find_tag(tiff::tags::Tag::Unknown(50707))
        .expect("find DNGBackwardVersion tag");
    assert!(
        dng_backward_version.is_none(),
        "TIFF must not carry DNGBackwardVersion tag (50707)"
    );
}

#[test]
fn the_tiff_keeps_its_own_profile_after_the_metadata_copy() {
    let env = TestEnv::new();
    env.create_dng("DSCF0001.dng", 32, 32);

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Tiff,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["DSCF0001.tif"]);

    let output_path = store.output_dir().join("DSCF0001.tif");
    let file_bytes = fs::read(&output_path).expect("read TIFF bytes");
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&file_bytes)).unwrap();

    let tag_val = decoder
        .find_tag(tiff::tags::Tag::IccProfile)
        .unwrap()
        .expect("tag 34675 present");
    let icc_bytes: Vec<u8> = match tag_val {
        tiff::decoder::ifd::Value::List(l) => l
            .into_iter()
            .map(|v| match v {
                tiff::decoder::ifd::Value::Byte(b) => b,
                _ => panic!("expected Byte in list"),
            })
            .collect(),
        tiff::decoder::ifd::Value::Byte(b) => vec![b],
        _ => panic!("unexpected tag type"),
    };

    let expected_profile = prophoto_icc_profile().expect("expected profile");
    assert_eq!(
        icc_bytes, expected_profile,
        "TIFF embedded ICC profile must match prophoto_icc_profile() byte for byte"
    );
}

#[test]
fn outputs_go_beside_the_raws_in_developed_and_nothing_is_overwritten() {
    let env = TestEnv::new();
    env.create_dng("DSCF0001.dng", 32, 32);

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    assert!(!store.output_dir().exists());

    // First run: develops DSCF0001.dng into Developed/DSCF0001.jpg
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];
    let plan = plan_develop(
        &items,
        OutputFormat::Jpeg { quality: 90 },
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["DSCF0001.jpg"]);
    assert!(store.output_dir().exists());
    assert!(store.output_dir().join("DSCF0001.jpg").exists());

    // Pre-fill Developed/DSCF0002.jpg with known bytes
    let known_bytes = b"pre-filled content that must not be overwritten";
    fs::write(store.output_dir().join("DSCF0002.jpg"), known_bytes).expect("write pre-filled");

    // Add DSCF0002.dng and try to develop it
    env.create_dng("DSCF0002.dng", 32, 32);
    let items2 = vec![DevelopItem {
        name: "DSCF0002.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];
    let plan2 = plan_develop(
        &items2,
        OutputFormat::Jpeg { quality: 90 },
        &store.existing_outputs().unwrap(),
    );
    assert_eq!(plan2.actions.len(), 0);
    assert_eq!(plan2.skipped.len(), 1);
    assert_eq!(plan2.skipped[0].file, "DSCF0002.dng");
    assert_eq!(
        plan2.skipped[0].reason,
        "Developed/DSCF0002.jpg already exists"
    );

    let summary2 = execute_develop(plan2, &mut store, &NoopProgress);
    assert!(summary2.developed.is_empty());
    assert_eq!(summary2.skipped.len(), 1);

    // Verify pre-filled bytes are unchanged
    let current_bytes = fs::read(store.output_dir().join("DSCF0002.jpg")).expect("read file");
    assert_eq!(current_bytes, known_bytes);
}

#[test]
fn one_exiftool_process_carries_the_whole_batch() {
    let env = TestEnv::new();
    env.create_dng("DSCF0001.dng", 32, 32);
    env.create_dng("DSCF0002.dng", 32, 32);
    env.create_dng("DSCF0003.dng", 32, 32);

    // Exiftool counting shim, created without Command::new("which") to respect R1
    let real_exiftool =
        phototools_core::media::meta::exiftool_program().expect("exiftool installed");
    let shim = env.temp.path().join("exiftool-shim");
    let log = env.temp.path().join("spawns.log");

    fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho spawn >> \"{}\"\nexec \"{}\" \"$@\"\n",
            log.display(),
            real_exiftool
        ),
    )
    .expect("write shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("chmod +x shim");
    }

    let mut store = FolderStore::with_exiftool(
        env.raw_dir.clone(),
        env.lut_dir.clone(),
        Some(shim.to_string_lossy().to_string()),
    );

    let items = vec![
        DevelopItem {
            name: "DSCF0001.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
        },
        DevelopItem {
            name: "DSCF0002.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
        },
        DevelopItem {
            name: "DSCF0003.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
        },
    ];

    let plan = plan_develop(
        &items,
        OutputFormat::Jpeg { quality: 90 },
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed.len(), 3);
    assert!(summary.failures.is_empty());
    assert!(summary.metadata_skipped.is_empty());

    let spawns = fs::read_to_string(&log).unwrap_or_default().lines().count();
    assert_eq!(
        spawns, 1,
        "3 photographs developed must start exactly 1 exiftool process (G4), started {spawns}"
    );
}
