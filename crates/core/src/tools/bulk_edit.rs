//! Bulk application of a full adjustment recipe (ED-18).
//!
//! Applies one complete `AdjustmentRecipe` — including its optional 3D LUT — to
//! every photograph in a folder, preserving EXIF metadata and writing
//! non-overwriting monotonic derivatives with the `_edit` suffix.
//!
//! A *named preset* is not this tool's concern: the command layer turns a preset
//! into a recipe with `tools::load_preset` and hands the recipe over. This module
//! only knows recipes.

use crate::error::Error;
use crate::ingest::scanner;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::edit::lut::Lut;
use crate::media::edit::pipeline::AdjustmentRecipe;
use crate::tools::edit::{
    find_volume_root, is_card_volume_with_resolver, is_identity, is_sidecar, render_and_write,
    CURRENT_RECIPE_VERSION,
};
use crate::tools::lut::is_hidden;
use crate::tools::lut::ACCEPTED;
use crate::tools::lut_library::resolve_recipe_lut;
use crate::tools::{carry_metadata_with, expand_inputs, Derived, Plan, Skip, Tool};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Parameters for a bulk recipe application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkEditParams {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub recipe: AdjustmentRecipe,
    /// The managed LUT library, for resolving `recipe.lut` (`Config::lut_dir()`).
    pub lut_dir: PathBuf,
    pub out_dir: PathBuf,
}

impl BulkEditParams {
    pub fn new(
        inputs: Vec<PathBuf>,
        recipe: AdjustmentRecipe,
        lut_dir: PathBuf,
        out_dir: PathBuf,
    ) -> Self {
        Self {
            inputs,
            recursive: false,
            recipe,
            lut_dir,
            out_dir,
        }
    }
}

/// One planned output of a bulk recipe run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkEditAction {
    pub source: PathBuf,
    pub out_dir: PathBuf,
    pub recipe: AdjustmentRecipe,
    pub recipe_sha256: String,
    pub lut_dir: PathBuf,
}

/// Summary of a bulk recipe run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BulkEditSummary {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<Skip>,
    pub failures: Vec<(PathBuf, String)>,
    pub metadata_skipped: Vec<Skip>,
}

/// Pre-flight sample frames selection for ED-18 preview.
///
/// Returns up to `n` (typically 3–5) source paths spread evenly through the
/// folder in plan order (first, last, and intermediate evenly spaced), rather than taking the first `n`.
pub fn sample_frames(plan: &Plan<BulkEditAction>, n: usize) -> Vec<PathBuf> {
    crate::tools::sample_evenly(&plan.actions, n)
        .into_iter()
        .map(|a| a.source.clone())
        .collect()
}

/// Compute the content hash that locks a batch to an exact recipe.
///
/// `source_sha256` names the photograph a recipe was made on, which is irrelevant
/// to applying it elsewhere, and two recipes that differ only there must lock the
/// same. The version is normalised to `CURRENT_RECIPE_VERSION` so a v1 sidecar
/// loaded into v2 locks identically to a native v2 recipe with the same settings.
pub fn recipe_sha256(recipe: &AdjustmentRecipe) -> String {
    let mut normalised = batch_recipe(recipe);
    normalised.source_sha256.clear();
    normalised.version = CURRENT_RECIPE_VERSION;

    let json =
        serde_json::to_vec(&normalised).expect("AdjustmentRecipe serialises to JSON infallibly");
    scanner::hex(&Sha256::digest(&json))
}

/// The recipe a batch applies: the given one without its masks.
///
/// A mask is placed on one photograph's content and means nothing on another's, so a
/// batch never carries one, as a preset never does (ED-19). Stripped here, where both the
/// plan and the lock are computed, so a recipe with masks and the same recipe without
/// them plan and lock identically.
pub fn batch_recipe(recipe: &AdjustmentRecipe) -> AdjustmentRecipe {
    let mut r = recipe.clone();
    r.masks.clear();
    r
}

pub struct BulkEditTool;

impl Tool for BulkEditTool {
    type Params = BulkEditParams;
    type Action = BulkEditAction;
    type Summary = BulkEditSummary;

    /// Dry run. Creates nothing.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        self.plan_with_resolver(p, find_volume_root)
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        // Tool::apply has no external reviewed hash to enforce; the real lock is
        // apply_with, which the command layer must use. Passing the plan's own
        // hash makes an unchecked apply harmless but consistent.
        let expected = if plan.actions.is_empty() {
            String::new()
        } else {
            plan.actions[0].recipe_sha256.clone()
        };
        self.apply_with(plan, &expected, progress, None)
    }
}

impl BulkEditTool {
    /// Plan with an injectable volume root resolver (for testing card volume refusal).
    pub fn plan_with_resolver<F>(
        &self,
        p: &BulkEditParams,
        root_resolver: F,
    ) -> ToolResult<Plan<BulkEditAction>>
    where
        F: Fn(&Path) -> Option<PathBuf>,
    {
        // Core refuses output directory on an SD card volume (G5) before anything runs.
        if is_card_volume_with_resolver(&p.out_dir, root_resolver) {
            return Err(Error::Refused(
                "Cannot output to a card volume: card media is read-only (G5)".into(),
            ));
        }

        // Resolve the LUT at plan time so a missing or malformed LUT fails the dry run,
        // not the run.
        let recipe = batch_recipe(&p.recipe);
        let lut = if let Some(lut_ref) = &recipe.lut {
            let (_, lut) = resolve_recipe_lut(&p.lut_dir, lut_ref)?;
            Some(lut)
        } else {
            None
        };

        // Refuse a recipe that would write byte-identical copies.
        if is_identity(&recipe, lut.as_ref()) {
            return Err(Error::Refused(
                "Recipe does not change anything; nothing to apply".into(),
            ));
        }

        let (files, mut skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);

        let recipe_sha256 = recipe_sha256(&recipe);
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

            actions.push(BulkEditAction {
                source,
                out_dir: p.out_dir.clone(),
                recipe: recipe.clone(),
                recipe_sha256: recipe_sha256.clone(),
                lut_dir: p.lut_dir.clone(),
            });
        }

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    /// Apply with an optional custom exiftool program (for G4 single-process testing)
    /// and an expected recipe content hash that locks the batch to the reviewed recipe.
    pub fn apply_with(
        &self,
        plan: Plan<BulkEditAction>,
        expected_recipe_sha256: &str,
        progress: &dyn Progress,
        exiftool_program: Option<&str>,
    ) -> ToolResult<BulkEditSummary> {
        let total = plan.actions.len() as u64;
        let mut summary = BulkEditSummary {
            written: Vec::new(),
            skipped: plan.skipped,
            failures: Vec::new(),
            metadata_skipped: Vec::new(),
        };

        if plan.actions.is_empty() {
            progress.report(0, 0, "done");
            return Ok(Outcome { data: summary });
        }

        // Capture everything needed from the first action before consuming the plan.
        let first_recipe = plan.actions[0].recipe.clone();
        let recorded_hash = plan.actions[0].recipe_sha256.clone();
        let lut_dir = plan.actions[0].lut_dir.clone();

        // The batch is locked to the exact recipe reviewed during dry run.
        if expected_recipe_sha256 != recorded_hash.as_str() {
            return Err(Error::Refused(
                "The recipe changed since the dry run; run another dry run".into(),
            ));
        }

        // Re-derive the hash from the recipe stored in the plan to catch any mutation
        // between plan and apply.
        if recipe_sha256(&first_recipe).as_str() != recorded_hash.as_str() {
            return Err(Error::Refused(
                "The recipe changed since the dry run; run another dry run".into(),
            ));
        }

        // Resolve the LUT again for the run. If it left the library (or changed hash),
        // the run is refused.
        let lut = if let Some(lut_ref) = &first_recipe.lut {
            let (path, lut) = resolve_recipe_lut(&lut_dir, lut_ref)?;
            Some((path, lut))
        } else {
            None
        };

        let mut derived: Vec<Derived> = Vec::new();

        // Process files sequentially: a 36 MP linear frame requires ~433 MB of f32 memory.
        // Rayon parallelism is employed within each image, so processing files
        // sequentially avoids massive memory spikes and thrashing.
        // Never decode many full frames simultaneously.
        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            let lut_ref: Option<&Lut> = lut.as_ref().map(|(_, l)| l);
            match render_and_write(
                &action.source,
                &action.recipe,
                lut_ref,
                &action.out_dir,
                "_edit",
            ) {
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
