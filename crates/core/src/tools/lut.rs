//! The Bulk LUT tool (ED-5).
//!
//! Applies a display-encoded 3D LUT at a configured intensity to a collection
//! of photographs, preserving EXIF metadata (capture dates, camera tags, GPS)
//! and writing non-overwriting monotonic derivatives with the `_lut` suffix.
//!
//! Follows F8 as the tool template.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::edit::lut::Lut;
use crate::media::edit::pipeline::{AdjustmentRecipe, LutRef};
use crate::tools::edit::{
    find_volume_root, is_card_volume_with_resolver, is_sidecar, render_and_write,
};
use crate::tools::{carry_metadata_with, expand_inputs, Derived, Plan, Skip, Tool};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Image formats accepted by `BulkLutTool` (matching formats supported by `decode_image`).
pub const ACCEPTED: [&str; 9] = [
    "jpg", "jpeg", "tif", "tiff", "dng", "nef", "arw", "cr2", "raf",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkLutParams {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub lut_path: PathBuf,
    pub intensity: f32,
    pub out_dir: PathBuf,
}

impl BulkLutParams {
    pub fn new(inputs: Vec<PathBuf>, lut_path: PathBuf, intensity: f32, out_dir: PathBuf) -> Self {
        Self {
            inputs,
            recursive: false,
            lut_path,
            intensity,
            out_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkLutAction {
    pub source: PathBuf,
    pub out_dir: PathBuf,
    pub lut_path: PathBuf,
    pub lut_sha256: String,
    pub intensity: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BulkLutSummary {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<Skip>,
    pub failures: Vec<(PathBuf, String)>,
    pub metadata_skipped: Vec<Skip>,
}

pub(crate) fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

/// Pre-flight sample frames selection for ED-8 preview.
///
/// Returns up to `n` (typically 3–5) source paths spread evenly through the
/// folder in plan order (first, last, and intermediate evenly spaced), rather than taking the first `n`.
pub fn sample_frames(plan: &Plan<BulkLutAction>, n: usize) -> Vec<PathBuf> {
    let actions = &plan.actions;
    let len = actions.len();
    if len == 0 || n == 0 {
        return Vec::new();
    }
    if len <= n {
        return actions.iter().map(|a| a.source.clone()).collect();
    }
    if n == 1 {
        return vec![actions[0].source.clone()];
    }

    (0..n)
        .map(|i| {
            let idx = (i * (len - 1)) / (n - 1);
            actions[idx].source.clone()
        })
        .collect()
}

pub struct BulkLutTool;

impl Tool for BulkLutTool {
    type Params = BulkLutParams;
    type Action = BulkLutAction;
    type Summary = BulkLutSummary;

    /// Dry run. Creates nothing.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        self.plan_with_resolver(p, find_volume_root)
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        self.apply_with(plan, progress, None)
    }
}

impl BulkLutTool {
    /// Plan with an injectable volume root resolver (for testing card volume refusal).
    pub fn plan_with_resolver<F>(
        &self,
        p: &BulkLutParams,
        root_resolver: F,
    ) -> ToolResult<Plan<BulkLutAction>>
    where
        F: Fn(&Path) -> Option<PathBuf>,
    {
        // Core refuses output directory on an SD card volume (G5) before anything runs.
        if is_card_volume_with_resolver(&p.out_dir, root_resolver) {
            return Err(Error::Refused(
                "Cannot output to a card volume: card media is read-only (G5)".into(),
            ));
        }

        // Parse and validate the LUT at dry-run / plan time so malformed LUTs fail before running.
        let lut = Lut::from_file(&p.lut_path)?;
        let lut_sha256 = lut.sha256;

        let (files, mut skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);

        let mut actions = Vec::new();
        for source in files {
            if is_hidden(&source) {
                skipped.push(Skip {
                    file: source.to_string_lossy().to_string(),
                    reason: "Hidden file".into(),
                });
                continue;
            }
            if is_sidecar(&source) {
                skipped.push(Skip {
                    file: source.to_string_lossy().to_string(),
                    reason: "Edit sidecar — travels with its companion image".into(),
                });
                continue;
            }

            actions.push(BulkLutAction {
                source,
                out_dir: p.out_dir.clone(),
                lut_path: p.lut_path.clone(),
                lut_sha256: lut_sha256.clone(),
                intensity: p.intensity.clamp(0.0, 1.0),
            });
        }

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    /// Apply with an optional custom exiftool program (for G4 single-process testing).
    pub fn apply_with(
        &self,
        plan: Plan<BulkLutAction>,
        progress: &dyn Progress,
        exiftool_program: Option<&str>,
    ) -> ToolResult<BulkLutSummary> {
        let total = plan.actions.len() as u64;
        let mut summary = BulkLutSummary {
            written: Vec::new(),
            skipped: plan.skipped,
            failures: Vec::new(),
            metadata_skipped: Vec::new(),
        };

        if plan.actions.is_empty() {
            progress.report(0, 0, "done");
            return Ok(Outcome { data: summary });
        }

        // The LUT is read ONCE from disk and parsed.
        let lut = Lut::from_file(&plan.actions[0].lut_path)?;

        // Invariant: The run is bound to the exact LUT inspected during dry run / preview.
        if lut.sha256 != plan.actions[0].lut_sha256 {
            return Err(Error::Refused(
                "The LUT file changed on disk since the dry run / preview; please run another dry run"
                    .into(),
            ));
        }
        let lut_name = plan.actions[0]
            .lut_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let lut_ref = LutRef {
            name: lut_name,
            sha256: lut.sha256.clone(),
        };
        let recipe = AdjustmentRecipe {
            lut: Some(lut_ref),
            lut_intensity: plan.actions[0].intensity,
            ..AdjustmentRecipe::default()
        };

        let mut derived: Vec<Derived> = Vec::new();

        // Process files sequentially: a 36 MP linear frame requires ~433 MB of f32 memory.
        // Rayon parallelism is employed within each image (pixel chunks and tone curves),
        // so processing files sequentially avoids massive memory spikes and thrashing.
        // Never decode many full frames simultaneously.
        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            match render_and_write(&action.source, &recipe, Some(&lut), &action.out_dir, "_lut") {
                Ok(item) => {
                    summary.written.push(item.output.clone());
                    derived.push(item);
                }
                Err(e) => {
                    summary.failures.push((action.source, e.to_string()));
                }
            }
        }

        // Single exiftool process for the entire batch (G4).
        // upright is false: decoder reads orientation as stored; orientation tag is preserved.
        summary.metadata_skipped = carry_metadata_with(&derived, false, exiftool_program);

        progress.report(total, total, "done");
        Ok(Outcome { data: summary })
    }
}
