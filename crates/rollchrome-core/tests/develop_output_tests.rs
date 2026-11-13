//! DV-7: developing without a look, and the output settings (folder, long edge, JPEG + TIFF).

#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use phototools_core::config::{Config, Thresholds};
use phototools_core::error::Error;
use rollchrome_core::automation::{Automation, DevelopRequest};
use rollchrome_core::media::develop::{
    develop, downscaled_size, render, DevelopOptions, Look, OutputFormat,
};
use rollchrome_core::media::film::Grain;
use rollchrome_core::tools::develop::{
    execute_develop, plan_develop, plan_develop_with, DevelopItem, DevelopOutput, DevelopStore,
    NEUTRAL_LOOK,
};
use rollchrome_core::tools::develop_folder::FolderStore;
use sha2::{Digest, Sha256};
use support::dng::DngBuilder;

const JPEG: OutputFormat = OutputFormat::Jpeg { quality: 90 };

struct NoopProgress;

impl phototools_core::jobs::Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

fn dng(width: u32, height: u32) -> Vec<u8> {
    DngBuilder::new_bayer(width, height).build(&vec![3000u16; (width * height) as usize])
}

fn item(name: &str, look: Option<&str>) -> DevelopItem {
    DevelopItem {
        name: name.to_string(),
        look: look.map(str::to_string),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
    }
}

/// A store in memory: what was written, and every metadata carry with the size it was given.
#[derive(Default)]
struct MemStore {
    sources: HashMap<String, Vec<u8>>,
    written: BTreeMap<String, Vec<u8>>,
    carried: Vec<(String, u32, u32, OutputFormat)>,
}

impl DevelopStore for MemStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        Ok(self.written.keys().map(|k| k.to_lowercase()).collect())
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        self.sources
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Refused(format!("{name} not found")))
    }
    fn look(&self, look: &str) -> Result<Look, Error> {
        Err(Error::Refused(format!("no library look '{look}' here")))
    }
    fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.written.contains_key(output) {
            return Err(Error::Refused(format!("Developed/{output} already exists")));
        }
        self.written.insert(output.to_string(), bytes.to_vec());
        Ok(())
    }
    fn carry_metadata(
        &mut self,
        _source: &str,
        output: &str,
        width: u32,
        height: u32,
        format: OutputFormat,
    ) -> Result<(), Error> {
        self.carried
            .push((output.to_string(), width, height, format));
        Ok(())
    }
}

fn mem_store_with(name: &str, bytes: Vec<u8>) -> MemStore {
    let mut store = MemStore::default();
    store.sources.insert(name.to_string(), bytes);
    store
}

#[test]
fn neutral_develops_a_photograph_with_no_look_applied() {
    let raw = dng(48, 32);
    let mut store = mem_store_with("a.dng", raw.clone());

    let plan = plan_develop(&[item("a.dng", Some(NEUTRAL_LOOK))], JPEG, &BTreeSet::new());
    assert_eq!(
        plan.actions.len(),
        1,
        "neutral must be planned, not skipped"
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["a.jpg"]);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    let expected = render(
        &develop(&raw, &DevelopOptions { exposure_ev: 0.0 }).unwrap(),
        None,
        JPEG,
    )
    .unwrap();
    assert_eq!(store.written["a.jpg"], expected.bytes);
}

#[test]
fn a_photograph_given_no_look_is_still_left_alone() {
    let plan = plan_develop(&[item("a.dng", None)], JPEG, &BTreeSet::new());
    assert!(plan.actions.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, "no look chosen");
}

#[test]
fn the_default_output_serialises_a_plan_exactly_as_before() {
    let items = [
        item("a.dng", Some(NEUTRAL_LOOK)),
        item("b.dng", Some("builtin:fujifilm-classic-chrome")),
        item("c.dng", None),
    ];
    let before = plan_develop(&items, JPEG, &BTreeSet::new());
    let with = plan_develop_with(&items, JPEG, &DevelopOutput::default(), &BTreeSet::new());

    let before_json = serde_json::to_string(&before).unwrap();
    assert_eq!(before_json, serde_json::to_string(&with).unwrap());
    for key in ["also", "long_edge", "output_folder", "also_output"] {
        assert!(
            !before_json.contains(&format!("\"{key}\"")),
            "a default plan must not carry '{key}': {before_json}"
        );
    }
}

#[test]
fn jpeg_and_tiff_writes_both_and_carries_metadata_to_both() {
    let raw = dng(48, 32);
    let mut store = mem_store_with("a.dng", raw.clone());
    let output = DevelopOutput {
        also_tiff: true,
        ..Default::default()
    };

    let plan = plan_develop_with(
        &[item("a.dng", Some(NEUTRAL_LOOK))],
        JPEG,
        &output,
        &BTreeSet::new(),
    );
    assert_eq!(plan.actions[0].also_output.as_deref(), Some("a.tif"));
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["a.jpg", "a.tif"]);
    let developed = develop(&raw, &DevelopOptions { exposure_ev: 0.0 }).unwrap();
    assert_eq!(
        store.written["a.jpg"],
        render(&developed, None, JPEG).unwrap().bytes
    );
    assert_eq!(
        store.written["a.tif"],
        render(&developed, None, OutputFormat::Tiff).unwrap().bytes
    );
    let formats: Vec<_> = store.carried.iter().map(|c| (c.0.as_str(), c.3)).collect();
    assert_eq!(
        formats,
        vec![("a.jpg", JPEG), ("a.tif", OutputFormat::Tiff)]
    );
}

#[test]
fn an_existing_tiff_skips_the_photograph_when_both_are_asked_for() {
    let existing: BTreeSet<String> = ["a.tif".to_string()].into();
    let output = DevelopOutput {
        also_tiff: true,
        ..Default::default()
    };
    let plan = plan_develop_with(
        &[item("a.dng", Some(NEUTRAL_LOOK))],
        JPEG,
        &output,
        &existing,
    );

    assert!(
        plan.actions.is_empty(),
        "the JPEG alone must not be written"
    );
    assert_eq!(plan.skipped[0].reason, "Developed/a.tif already exists");
}

#[test]
fn a_long_edge_downscales_the_jpeg_and_the_tiff_after_the_look() {
    let raw = dng(120, 80);
    let full = develop(&raw, &DevelopOptions { exposure_ev: 0.0 }).unwrap();
    let (want_w, want_h) = downscaled_size(full.image.width(), full.image.height(), 64);
    assert!(
        want_w < full.image.width(),
        "the fixture must be larger than the edge"
    );

    let mut store = mem_store_with("a.dng", raw);
    let output = DevelopOutput {
        also_tiff: true,
        long_edge: Some(64),
        ..Default::default()
    };
    let plan = plan_develop_with(
        &[item("a.dng", Some("builtin:fujifilm-classic-chrome"))],
        JPEG,
        &output,
        &BTreeSet::new(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);

    for name in ["a.jpg", "a.tif"] {
        let img = image::load_from_memory(&store.written[name]).unwrap();
        assert_eq!((img.width(), img.height()), (want_w, want_h), "{name}");
    }
    for carried in &store.carried {
        assert_eq!((carried.1, carried.2), (want_w, want_h), "{}", carried.0);
    }
}

#[test]
fn a_long_edge_larger_than_the_frame_keeps_its_size() {
    let raw = dng(48, 32);
    let developed = develop(&raw, &DevelopOptions { exposure_ev: 0.0 }).unwrap();
    let mut store = mem_store_with("a.dng", raw);
    let output = DevelopOutput {
        also_tiff: true,
        long_edge: Some(10_000),
        ..Default::default()
    };
    let plan = plan_develop_with(
        &[item("a.dng", Some(NEUTRAL_LOOK))],
        JPEG,
        &output,
        &BTreeSet::new(),
    );
    execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(
        store.written["a.jpg"],
        render(&developed, None, JPEG).unwrap().bytes
    );
    assert_eq!(
        store.written["a.tif"],
        render(&developed, None, OutputFormat::Tiff).unwrap().bytes
    );
}

#[test]
fn output_settings_that_mean_nothing_are_refused() {
    let zero = DevelopOutput {
        long_edge: Some(0),
        ..Default::default()
    };
    assert!(matches!(zero.check(JPEG), Err(Error::Refused(_))));

    let tiff_twice = DevelopOutput {
        also_tiff: true,
        ..Default::default()
    };
    assert!(matches!(
        tiff_twice.check(OutputFormat::Tiff),
        Err(Error::Refused(_))
    ));
    assert!(tiff_twice.check(JPEG).is_ok());
    assert!(DevelopOutput::default().check(OutputFormat::Tiff).is_ok());
}

#[test]
fn a_chosen_output_folder_receives_the_files_and_developed_is_not_created() {
    let temp = tempfile::tempdir().unwrap();
    let raw_dir = temp.path().join("raws");
    let lut_dir = temp.path().join("luts");
    let chosen = temp.path().join("exports").join("trip");
    fs::create_dir_all(&raw_dir).unwrap();
    fs::create_dir_all(&lut_dir).unwrap();
    fs::write(raw_dir.join("a.dng"), dng(48, 32)).unwrap();

    let output = DevelopOutput {
        folder: Some(chosen.display().to_string()),
        ..Default::default()
    };
    let mut store =
        FolderStore::new(raw_dir.clone(), lut_dir.clone()).with_output_dir(chosen.clone());
    let plan = plan_develop_with(
        &[item("a.dng", Some(NEUTRAL_LOOK))],
        JPEG,
        &output,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["a.jpg"]);
    assert!(
        summary.metadata_skipped.is_empty(),
        "{:?}",
        summary.metadata_skipped
    );
    assert!(chosen.join("a.jpg").is_file());
    assert!(!raw_dir.join("Developed").exists());

    // A second run names the chosen folder when it refuses to overwrite.
    let store = FolderStore::new(raw_dir, lut_dir).with_output_dir(chosen.clone());
    let again = plan_develop_with(
        &[item("a.dng", Some(NEUTRAL_LOOK))],
        JPEG,
        &output,
        &store.existing_outputs().unwrap(),
    );
    assert!(again.actions.is_empty());
    assert_eq!(
        again.skipped[0].reason,
        format!("a.jpg in {} already exists", chosen.display())
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    config: Config,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let staging = temp.path().join("staging");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&staging).unwrap();
    let root = root.canonicalize().unwrap();
    let config = Config {
        roots: vec![root.clone()],
        staging_dir: staging,
        publishing_dir: None,
        thresholds: Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };
    Fixture {
        _temp: temp,
        config,
        root,
    }
}

fn request(folder: &Path, output: DevelopOutput) -> DevelopRequest {
    DevelopRequest {
        folder: folder.to_string_lossy().to_string(),
        look: NEUTRAL_LOOK.to_string(),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
        format: JPEG,
        only: Vec::new(),
        output,
    }
}

#[test]
fn automation_hashes_a_default_request_as_the_plan_alone_and_runs_neutral() {
    let f = fixture();
    fs::write(f.root.join("a.dng"), dng(48, 32)).unwrap();
    let auto = Automation::new(f.config.clone());

    // A request written before DV-7, with no "output" key, still parses.
    let json = serde_json::json!({
        "folder": f.root.to_string_lossy(),
        "look": "neutral",
        "exposure_ev": 0.0,
        "grain": "off",
        "noise_reduction": 0.0,
        "format": { "kind": "jpeg", "quality": 90 },
        "only": []
    });
    let old: DevelopRequest = serde_json::from_value(json).unwrap();
    assert_eq!(old.output, DevelopOutput::default());

    let reviewed = auto.plan_develop(&old).unwrap();
    let plan_hash: String = Sha256::digest(serde_json::to_vec(&reviewed.plan).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        reviewed.plan_sha256, plan_hash,
        "neutral adds no LUT to the hash"
    );

    let summary = auto
        .run_develop(&old, Some(&reviewed.plan_sha256), &NoopProgress)
        .unwrap();
    assert_eq!(summary.developed, vec!["a.jpg"]);
    assert!(f.root.join("Developed").join("a.jpg").is_file());
}

#[test]
fn automation_refuses_an_output_folder_outside_the_roots() {
    let f = fixture();
    fs::write(f.root.join("a.dng"), dng(48, 32)).unwrap();
    let outside = f._temp.path().join("elsewhere");
    let auto = Automation::new(f.config.clone());

    let req = request(
        &f.root,
        DevelopOutput {
            folder: Some(outside.display().to_string()),
            ..Default::default()
        },
    );
    assert!(matches!(
        auto.plan_develop(&req),
        Err(Error::AccessDenied(_))
    ));
    assert!(!outside.exists());

    let inside = f.root.join("exports");
    let req = request(
        &f.root,
        DevelopOutput {
            folder: Some(inside.display().to_string()),
            long_edge: Some(16),
            also_tiff: true,
        },
    );
    let reviewed = auto.plan_develop(&req).unwrap();
    assert_eq!(reviewed.plan.long_edge, Some(16));
    let summary = auto
        .run_develop(&req, Some(&reviewed.plan_sha256), &NoopProgress)
        .unwrap();
    assert_eq!(summary.developed, vec!["a.jpg", "a.tif"]);
    assert!(inside.join("a.tif").is_file());
}

#[cfg(feature = "film")]
#[test]
fn a_film_look_writes_jpeg_and_tiff_and_a_long_edge_downsizes_them() {
    use rollchrome_core::media::film::ensure_film_data;

    let data = tempfile::tempdir().unwrap();
    let film_dir = ensure_film_data(data.path()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let raw_dir = temp.path().join("raws");
    let lut_dir = temp.path().join("luts");
    fs::create_dir_all(&raw_dir).unwrap();
    fs::create_dir_all(&lut_dir).unwrap();
    fs::write(raw_dir.join("a.dng"), dng(64, 48)).unwrap();

    let mut store = FolderStore::with_film_data(raw_dir, lut_dir, Some(film_dir));
    let output = DevelopOutput {
        also_tiff: true,
        long_edge: Some(32),
        ..Default::default()
    };
    let plan = plan_develop_with(
        &[item("a.dng", Some("film:kodak_portra_400"))],
        JPEG,
        &output,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.developed, vec!["a.jpg", "a.tif"]);
    for name in ["a.jpg", "a.tif"] {
        let (w, h) = image::image_dimensions(store.output_dir().join(name)).unwrap();
        assert_eq!(w.max(h), 32, "{name} is {w}x{h}");
    }
}
