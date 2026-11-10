//! Acceptance tests for RP-13 (`rollchrome_core::automation`).

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use phototools_core::config::{Config, Thresholds};
use phototools_core::error::Error;
use phototools_core::jobs::Progress;
use rollchrome_core::automation::{
    describe, Automation, BatchRequest, DevelopRequest, ExportRequest,
};
use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::media::edit::AdjustmentRecipe;
use rollchrome_core::media::film::Grain;
use rollchrome_core::tools::edit::{export_edited_image_with_options, save_recipe, ExportOptions};
use rollchrome_core::tools::lut_library::import_lut;
use support::dng::DngBuilder;

struct TestFixture {
    _temp: tempfile::TempDir,
    config: Config,
    root: PathBuf,
}

impl TestFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("library");
        let staging = temp.path().join("staging");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&staging).expect("create staging");
        let root = root.canonicalize().expect("canonicalize root");

        let config = Config {
            roots: vec![root.clone()],
            staging_dir: staging,
            publishing_dir: None,
            thresholds: Thresholds::default(),
            database: temp.path().join("ledger.sqlite3"),
        };

        Self {
            _temp: temp,
            config,
            root,
        }
    }

    fn write_dng(&self, dir: &Path, name: &str, pixel_value: u16) -> PathBuf {
        let builder = DngBuilder::new_bayer(16, 16);
        let pixels = vec![pixel_value; 16 * 16];
        let bytes = builder.build(&pixels);
        let path = dir.join(name);
        fs::write(&path, bytes).expect("write DNG");
        path
    }

    fn write_jpeg(&self, dir: &Path, name: &str) -> PathBuf {
        let mut img = image::RgbImage::new(24, 24);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 10) as u8, (y * 10) as u8, 128]);
        }
        let path = dir.join(name);
        img.save(&path).expect("save JPEG");
        path
    }

    fn write_tiff(&self, dir: &Path, name: &str) -> PathBuf {
        let mut img = image::RgbImage::new(24, 24);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 8) as u8, (y * 8) as u8, 100]);
        }
        let path = dir.join(name);
        img.save_with_format(&path, image::ImageFormat::Tiff)
            .expect("save TIFF");
        path
    }

    fn write_cube(&self, dir: &Path, name: &str, red_max: f32) -> PathBuf {
        let content = format!(
            "TITLE \"Test\"\n\
             LUT_3D_SIZE 2\n\
             0.0 0.0 0.0\n\
             {red_max:.4} 0.0 0.0\n\
             0.0 1.0 0.0\n\
             {red_max:.4} 1.0 0.0\n\
             0.0 0.0 1.0\n\
             {red_max:.4} 0.0 1.0\n\
             0.0 1.0 1.0\n\
             {red_max:.4} 1.0 1.0\n"
        );
        let path = dir.join(name);
        fs::write(&path, content).expect("write cube");
        path
    }
}

#[derive(Default)]
struct NoopProgress;

impl Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

const FIXED_G6_SENTENCE: &str = "That path is outside the folders this application may touch.";

#[test]
fn a_path_outside_the_roots_is_refused_before_anything_runs() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());

    let outside = f._temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let inside_photo = f.write_jpeg(&f.root, "inside.jpg");
    let outside_photo = f.write_jpeg(&outside, "outside.jpg");
    let inside_out = f.root.join("out");
    let outside_out = outside.join("out");
    let cube = f.write_cube(&f.root, "look.cube", 1.0);

    // 1. plan_develop (input folder outside roots)
    let dev_outside = DevelopRequest {
        folder: outside.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };
    let err = auto.plan_develop(&dev_outside).unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 2. run_develop (input folder outside roots)
    let err = auto
        .run_develop(&dev_outside, None, &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 3. plan_batch (input outside roots)
    let batch_in_outside = BatchRequest {
        inputs: vec![outside_photo.to_string_lossy().to_string()],
        recursive: false,
        lut: cube.to_string_lossy().to_string(),
        intensity: 1.0,
        out_dir: inside_out.to_string_lossy().to_string(),
    };
    let err = auto.plan_batch(&batch_in_outside).unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 4. plan_batch (output outside roots)
    let batch_out_outside = BatchRequest {
        inputs: vec![inside_photo.to_string_lossy().to_string()],
        recursive: false,
        lut: cube.to_string_lossy().to_string(),
        intensity: 1.0,
        out_dir: outside_out.to_string_lossy().to_string(),
    };
    let err = auto.plan_batch(&batch_out_outside).unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 5. run_batch (input outside roots)
    let err = auto
        .run_batch(&batch_in_outside, None, &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 6. run_batch (output outside roots)
    let err = auto
        .run_batch(&batch_out_outside, None, &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 7. export (input outside roots)
    let export_in_outside = ExportRequest {
        path: outside_photo.to_string_lossy().to_string(),
        out_dir: inside_out.to_string_lossy().to_string(),
        options: ExportOptions::default(),
    };
    let err = auto.export(&export_in_outside).unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    // 8. export (output outside roots)
    let export_out_outside = ExportRequest {
        path: inside_photo.to_string_lossy().to_string(),
        out_dir: outside_out.to_string_lossy().to_string(),
        options: ExportOptions::default(),
    };
    let err = auto.export(&export_out_outside).unwrap_err();
    assert!(matches!(err, Error::AccessDenied(_)));
    assert_eq!(describe(&err), FIXED_G6_SENTENCE);

    assert!(!outside_out.exists());
    assert!(!inside_out.exists());
}

#[test]
fn a_develop_plan_writes_nothing() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let listing_before: Vec<String> = fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();

    let req = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };
    let reviewed = auto.plan_develop(&req).expect("plan_develop");
    assert_eq!(reviewed.plan.actions.len(), 1);
    assert!(!reviewed.plan_sha256.is_empty());

    let listing_after: Vec<String> = fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();

    assert_eq!(listing_before, listing_after);
    assert!(!f.root.join("Developed").exists());
}

#[test]
fn running_develop_needs_the_hash_of_the_plan_that_was_reviewed() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let req = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };
    let reviewed = auto.plan_develop(&req).expect("plan_develop");

    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let err = auto
        .run_develop(&req, Some(wrong_hash), &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::Refused(_)));
    assert!(!f.root.join("Developed").exists());

    let summary = auto
        .run_develop(&req, Some(&reviewed.plan_sha256), &NoopProgress)
        .expect("run_develop with matching hash");
    assert_eq!(summary.developed, vec!["DSCF0001.jpg"]);
    assert!(f.root.join("Developed/DSCF0001.jpg").is_file());
}

#[test]
fn a_new_file_in_the_way_changes_the_hash_and_refuses_the_run() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());
    f.write_dng(&f.root, "DSCF0001.dng", 2500);
    f.write_dng(&f.root, "DSCF0002.dng", 2500);

    let req = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };
    let reviewed = auto.plan_develop(&req).expect("initial plan");
    assert_eq!(reviewed.plan.actions.len(), 2);

    // Put an output file in the way after the plan was reviewed
    let dev_dir = f.root.join("Developed");
    fs::create_dir_all(&dev_dir).unwrap();
    fs::write(dev_dir.join("DSCF0001.jpg"), b"already here").unwrap();

    let replanned = auto.plan_develop(&req).expect("second plan");
    assert_ne!(reviewed.plan_sha256, replanned.plan_sha256);

    let err = auto
        .run_develop(&req, Some(&reviewed.plan_sha256), &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::Refused(_)));
    assert!(!dev_dir.join("DSCF0002.jpg").exists());
}

#[test]
fn develop_writes_into_developed_and_never_overwrites() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let req1 = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };
    let summary1 = auto
        .run_develop(&req1, None, &NoopProgress)
        .expect("first develop run");
    assert_eq!(summary1.developed, vec!["DSCF0001.jpg"]);

    let out_file = f.root.join("Developed/DSCF0001.jpg");
    let first_bytes = fs::read(&out_file).expect("read first developed file");

    // Second run with different exposure and look: existing file must be skipped and untouched
    let req2 = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "builtin:ricoh-standard".to_string(),
        exposure_ev: 1.5,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };
    let summary2 = auto
        .run_develop(&req2, None, &NoopProgress)
        .expect("second develop run");
    assert!(summary2.developed.is_empty());
    assert_eq!(summary2.skipped.len(), 1);

    let second_bytes = fs::read(&out_file).expect("read developed file after second run");
    assert_eq!(first_bytes, second_bytes);
}

#[test]
fn develop_on_a_card_is_refused_before_anything_is_written() {
    let f = TestFixture::new();
    let card_root = f.root.join("card_volume");
    let dcim_folder = card_root.join("DCIM/100FUJI");
    fs::create_dir_all(&dcim_folder).unwrap();
    f.write_dng(&dcim_folder, "DSCF0001.dng", 2500);

    fn resolve_card_root(p: &Path) -> Option<PathBuf> {
        p.ancestors()
            .find(|a| a.join("DCIM").is_dir())
            .map(Path::to_path_buf)
    }

    let auto = Automation::with_volume_resolver(f.config.clone(), resolve_card_root);
    let req = DevelopRequest {
        folder: dcim_folder.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };

    let plan_err = auto.plan_develop(&req).unwrap_err();
    assert!(matches!(plan_err, Error::Refused(_)));
    assert!(plan_err.to_string().contains("card media is read-only"));

    let run_err = auto.run_develop(&req, None, &NoopProgress).unwrap_err();
    assert!(matches!(run_err, Error::Refused(_)));
    assert!(run_err.to_string().contains("card media is read-only"));
    assert!(!dcim_folder.join("Developed").exists());
}

#[test]
fn batch_takes_jpeg_and_tiff_and_skips_raw_with_a_reason() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());

    let jpg = f.write_jpeg(&f.root, "one.jpg");
    let tif = f.write_tiff(&f.root, "two.tiff");
    let raw = f.write_dng(&f.root, "three.dng", 2000);
    let cube = f.write_cube(&f.root, "grade.cube", 0.8);
    let out_dir = f.root.join("batch_out");

    let req = BatchRequest {
        inputs: vec![
            jpg.to_string_lossy().to_string(),
            tif.to_string_lossy().to_string(),
            raw.to_string_lossy().to_string(),
        ],
        recursive: false,
        lut: cube.to_string_lossy().to_string(),
        intensity: 1.0,
        out_dir: out_dir.to_string_lossy().to_string(),
    };

    let reviewed = auto.plan_batch(&req).expect("plan_batch");
    assert_eq!(reviewed.plan.actions.len(), 2);
    assert_eq!(reviewed.plan.skipped.len(), 1);
    assert!(reviewed.plan.skipped[0].file.ends_with("three.dng"));
    assert!(!reviewed.plan.skipped[0].reason.is_empty());
}

#[test]
fn batch_finds_a_cube_by_library_name() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());

    let cube_src = f.write_cube(&f.root, "CinemaWarm.cube", 0.9);
    let lut_dir = rollchrome_core::config::lut_dir(&f.config);
    import_lut(&lut_dir, &cube_src).expect("import LUT into library");

    let jpg = f.write_jpeg(&f.root, "photo.jpg");
    let out_dir = f.root.join("graded");

    let req = BatchRequest {
        inputs: vec![jpg.to_string_lossy().to_string()],
        recursive: false,
        lut: "CinemaWarm.cube".to_string(),
        intensity: 1.0,
        out_dir: out_dir.to_string_lossy().to_string(),
    };

    let reviewed = auto.plan_batch(&req).expect("plan_batch by library name");
    assert_eq!(reviewed.plan.actions.len(), 1);

    let summary = auto
        .run_batch(&req, Some(&reviewed.plan_sha256), &NoopProgress)
        .expect("run_batch by library name");
    assert_eq!(summary.written.len(), 1);
    assert!(out_dir.join("photo_lut.jpg").is_file());
}

#[test]
fn a_cube_changed_after_the_plan_refuses_the_run() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());

    let cube = f.write_cube(&f.root, "mutable.cube", 1.0);
    let jpg = f.write_jpeg(&f.root, "photo.jpg");
    let out_dir = f.root.join("graded");

    let req = BatchRequest {
        inputs: vec![jpg.to_string_lossy().to_string()],
        recursive: false,
        lut: cube.to_string_lossy().to_string(),
        intensity: 1.0,
        out_dir: out_dir.to_string_lossy().to_string(),
    };

    let reviewed = auto.plan_batch(&req).expect("initial plan_batch");

    // Modify the .cube file after planning
    f.write_cube(&f.root, "mutable.cube", 0.5);

    let err = auto
        .run_batch(&req, Some(&reviewed.plan_sha256), &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::Refused(_)));
    assert!(!out_dir.exists());
}

#[test]
fn export_uses_the_photographs_sidecar() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());

    let photo = f.write_jpeg(&f.root, "portrait.jpg");
    let out_default = f.root.join("out_default");
    let out_sidecar = f.root.join("out_sidecar");
    let out_direct = f.root.join("out_direct");
    fs::create_dir_all(&out_direct).unwrap();

    // Export without sidecar
    let res_default = auto
        .export(&ExportRequest {
            path: photo.to_string_lossy().to_string(),
            out_dir: out_default.to_string_lossy().to_string(),
            options: ExportOptions::default(),
        })
        .expect("export without sidecar");
    let bytes_without_sidecar = fs::read(&res_default.path).unwrap();

    // Save a sidecar recipe on the photograph
    let recipe = AdjustmentRecipe {
        exposure: 1.25,
        contrast: 30.0,
        saturation: -25.0,
        ..AdjustmentRecipe::default()
    };
    save_recipe(&photo, &recipe).expect("save sidecar recipe");

    // Export via Automation::export (reads sidecar automatically)
    let res_sidecar = auto
        .export(&ExportRequest {
            path: photo.to_string_lossy().to_string(),
            out_dir: out_sidecar.to_string_lossy().to_string(),
            options: ExportOptions::default(),
        })
        .expect("export with sidecar");
    let bytes_with_sidecar = fs::read(&res_sidecar.path).unwrap();

    // Direct export with the same recipe and options
    let res_direct = export_edited_image_with_options(
        &photo,
        &recipe,
        None,
        &out_direct,
        &ExportOptions::default(),
    )
    .expect("direct export_edited_image_with_options");
    let bytes_direct = fs::read(&res_direct.path).unwrap();

    assert_ne!(bytes_with_sidecar, bytes_without_sidecar);
    assert_eq!(bytes_with_sidecar, bytes_direct);
}

#[test]
fn export_refuses_raw() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());

    let raw = f.write_dng(&f.root, "DSCF0001.dng", 2500);
    let out_dir = f.root.join("exports");

    let err = auto
        .export(&ExportRequest {
            path: raw.to_string_lossy().to_string(),
            out_dir: out_dir.to_string_lossy().to_string(),
            options: ExportOptions::default(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::Refused(_)));
    assert!(!out_dir.exists());
}

#[test]
fn a_cancelled_run_stops_and_reports_what_it_did() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());
    f.write_dng(&f.root, "DSCF0001.dng", 2500);
    f.write_dng(&f.root, "DSCF0002.dng", 2500);
    f.write_dng(&f.root, "DSCF0003.dng", 2500);

    struct CancelAfterFirst {
        cancelled: AtomicBool,
    }

    impl Progress for CancelAfterFirst {
        fn report(&self, _done: u64, _total: u64, _current: &str) {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        fn cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }
    }

    let progress = CancelAfterFirst {
        cancelled: AtomicBool::new(false),
    };
    let req = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "builtin:fujifilm-classic-chrome".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };

    let summary = auto
        .run_develop(&req, None, &progress)
        .expect("cancelled develop run");
    assert_eq!(summary.developed, vec!["DSCF0001.jpg"]);
    assert!(f.root.join("Developed/DSCF0001.jpg").is_file());
    assert!(!f.root.join("Developed/DSCF0002.jpg").exists());
    assert!(!f.root.join("Developed/DSCF0003.jpg").exists());
}

#[test]
fn a_look_changed_after_the_plan_refuses_the_develop_run() {
    let f = TestFixture::new();
    let auto = Automation::new(f.config.clone());
    f.write_dng(&f.root, "DSCF0001.dng", 2500);

    let cube_src = f.write_cube(&f.root, "CustomFilm.cube", 1.0);
    let lut_dir = rollchrome_core::config::lut_dir(&f.config);
    import_lut(&lut_dir, &cube_src).expect("import LUT into library");
    fs::remove_file(&cube_src).expect("remove source cube from RAW folder");

    let req = DevelopRequest {
        folder: f.root.to_string_lossy().to_string(),
        look: "CustomFilm.cube".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 92 },
        only: Vec::new(),
    };

    let reviewed = auto
        .plan_develop(&req)
        .expect("plan_develop with cube look");
    assert_eq!(reviewed.plan.actions.len(), 1);

    // Change the .cube file inside the library after the plan was reviewed
    f.write_cube(&lut_dir, "CustomFilm.cube", 0.5);

    let err = auto
        .run_develop(&req, Some(&reviewed.plan_sha256), &NoopProgress)
        .unwrap_err();
    assert!(matches!(err, Error::Refused(_)));
    assert!(!f.root.join("Developed").exists());
}
