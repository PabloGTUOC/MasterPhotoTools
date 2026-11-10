//! Integration tests for develop folder operations (DV-2b).

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::PathBuf;

use phototools_core::media::meta::read_meta;
use phototools_core::media::ExifWriter;
use rollchrome_core::media::develop::{prophoto_icc_profile, OutputFormat};
use rollchrome_core::media::film::Grain;
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
        grain: Grain::Fine,
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
        grain: Grain::Fine,
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

/// Read InteropIndex (tag 0x0001 in Interop IFD) by walking:
/// IFD0 -> ExifIFD pointer (34665 / 0x8769) -> InteropOffset (0xa005) -> tag 0x0001.
fn read_interop_index(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 8 {
        return None;
    }
    let is_le = match &bytes[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let read_u16 = |buf: &[u8], offset: usize| -> Option<u16> {
        let slice = buf.get(offset..offset + 2)?;
        let arr = [slice[0], slice[1]];
        Some(if is_le {
            u16::from_le_bytes(arr)
        } else {
            u16::from_be_bytes(arr)
        })
    };
    let read_u32 = |buf: &[u8], offset: usize| -> Option<u32> {
        let slice = buf.get(offset..offset + 4)?;
        let arr = [slice[0], slice[1], slice[2], slice[3]];
        Some(if is_le {
            u32::from_le_bytes(arr)
        } else {
            u32::from_be_bytes(arr)
        })
    };

    // Find a tag in the IFD at `ifd_offset`, returning (type, count, value_or_offset_position).
    let find_tag_in_ifd = |ifd_offset: usize, target_tag: u16| -> Option<(u16, u32, usize)> {
        let count = read_u16(bytes, ifd_offset)? as usize;
        let mut cur = ifd_offset + 2;
        for _ in 0..count {
            if cur + 12 > bytes.len() {
                return None;
            }
            let tag = read_u16(bytes, cur)?;
            let type_ = read_u16(bytes, cur + 2)?;
            let val_count = read_u32(bytes, cur + 4)?;
            if tag == target_tag {
                return Some((type_, val_count, cur + 8));
            }
            cur += 12;
        }
        None
    };

    // 1. IFD0
    let ifd0_offset = read_u32(bytes, 4)? as usize;

    // 2. ExifIFD pointer tag 0x8769 (34665) in IFD0
    let (_, _, exif_ptr_offset) = find_tag_in_ifd(ifd0_offset, 0x8769)?;
    let exif_offset = read_u32(bytes, exif_ptr_offset)? as usize;

    // 3. InteropOffset tag 0xa005 in ExifIFD
    let (_, _, interop_ptr_offset) = find_tag_in_ifd(exif_offset, 0xa005)?;
    let interop_offset = read_u32(bytes, interop_ptr_offset)? as usize;

    // 4. InteropIndex tag 0x0001 in InteropIFD
    let (type_, count, val_offset) = find_tag_in_ifd(interop_offset, 0x0001)?;
    if type_ != 2 {
        // ASCII
        return None;
    }
    let data_offset = if count <= 4 {
        val_offset
    } else {
        read_u32(bytes, val_offset)? as usize
    };
    let str_bytes = bytes.get(data_offset..data_offset + count as usize)?;
    let s = std::str::from_utf8(str_bytes).ok()?;
    Some(s.trim_matches('\0').to_string())
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

    // 1. Prove that the test reader sees InteropIndex = R98 on the source DNG after setup
    let source_bytes = fs::read(&dng_path).expect("read source DNG bytes");
    assert_eq!(
        read_interop_index(&source_bytes),
        Some("R98".to_string()),
        "Source DNG must carry InteropIndex = R98 after setup"
    );

    let mut store = FolderStore::new(env.raw_dir.clone(), env.lut_dir.clone());
    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
        grain: Grain::Fine,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Tiff,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["DSCF0001.tif"]);

    let output_path = store.output_dir().join("DSCF0001.tif");

    // 2. Read ColorSpace using nom-exif (G3)
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
        Some(65535),
        "TIFF must report ColorSpace = 65535 (Uncalibrated)"
    );

    // 3. Prove with the same reader that InteropIndex is absent on the output TIFF
    let output_bytes = fs::read(&output_path).expect("read TIFF bytes");
    assert_eq!(
        read_interop_index(&output_bytes),
        None,
        "TIFF must not carry InteropIndex (0x0001) in Interop IFD"
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
        grain: Grain::Fine,
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
        grain: Grain::Fine,
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
    assert_eq!(icc_bytes.len(), expected_profile.len());
    // ColorProfile::encode() in moxcms embeds ColorDateTime::now() into the 12-byte
    // creation_date_time field (bytes 24..36) of the ICC header. If a wall-clock second
    // rolls over between TIFF rendering and this assertion, bytes 24..36 can differ by
    // 1 second. Normalize the timestamp header before byte-for-byte comparison.
    let mut actual_normalized = icc_bytes.clone();
    actual_normalized[24..36].copy_from_slice(&expected_profile[24..36]);
    assert_eq!(
        actual_normalized, expected_profile,
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
        grain: Grain::Fine,
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
        grain: Grain::Fine,
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
            grain: Grain::Fine,
        },
        DevelopItem {
            name: "DSCF0002.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
            grain: Grain::Fine,
        },
        DevelopItem {
            name: "DSCF0003.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
            grain: Grain::Fine,
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
