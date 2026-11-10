//! Automation layer over `rollchrome-core` (RP-13).
//!
//! Shared by `rollchrome-cli` and the MCP server (`automation::mcp`). Enforces
//! G5 (card volume read-only refusal), G6 (path resolution against configured
//! roots before any filesystem access), and dry-run plan hash verification.

pub mod mcp;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::jobs::Progress;
use phototools_core::tools::{Plan, Skip};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::media::develop::OutputFormat;
use crate::media::edit::lut::Lut;
use crate::media::film::Grain;
use crate::media::looks::builtin::{builtin_look_list, BuiltinLookInfo};
use crate::tools::develop::{
    execute_develop, plan_develop as core_plan_develop, DevelopItem, DevelopPlan, DevelopSummary,
};
use crate::tools::develop_folder::{raw_names, refuse_card_folder, FolderStore};
use crate::tools::edit::{
    export_edited_image_with_options_with_resolver, find_volume_root, load_recipe_for_image,
    ExportOptions, ExportResult,
};
use crate::tools::lut::{BulkLutAction, BulkLutParams, BulkLutSummary, BulkLutTool};
use crate::tools::lut_library::{
    find_lut_by_name_or_sha256, list_luts as core_list_luts, resolve_recipe_lut, LutLibraryList,
};

/// A dry-run plan paired with its SHA-256 hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewed<T> {
    pub plan: T,
    pub plan_sha256: String,
}

/// Parameters for planning or running folder RAW development.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DevelopRequest {
    pub folder: String,
    pub look: String,
    pub exposure_ev: f32,
    #[serde(default)]
    pub grain: Grain,
    pub format: OutputFormat,
    pub only: Vec<String>,
}

/// Parameters for planning or running batch 3D LUT grading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchRequest {
    pub inputs: Vec<String>,
    pub recursive: bool,
    pub lut: String,
    pub intensity: f32,
    pub out_dir: String,
}

/// Parameters for exporting a single photograph with its sidecar recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRequest {
    pub path: String,
    pub out_dir: String,
    pub options: ExportOptions,
}

/// Formats an `Error` for user-facing or MCP output, using the fixed G6 sentence
/// for `AccessDenied` so path existence is never leaked.
pub fn describe(e: &Error) -> String {
    match e {
        Error::AccessDenied(_) => {
            "That path is outside the folders this application may touch.".to_string()
        }
        other => other.to_string(),
    }
}

/// Stateless automation engine backed by `Config` and an injectable volume root resolver.
#[derive(Clone)]
pub struct Automation {
    config: Config,
    volume_resolver: fn(&Path) -> Option<PathBuf>,
}

impl Automation {
    /// Creates an automation engine using the standard `find_volume_root` resolver.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            volume_resolver: find_volume_root,
        }
    }

    /// Creates an automation engine with a custom volume root resolver (for card-safety tests).
    pub fn with_volume_resolver(config: Config, f: fn(&Path) -> Option<PathBuf>) -> Self {
        Self {
            config,
            volume_resolver: f,
        }
    }

    fn resolve_input(&self, path: &str) -> Result<PathBuf, Error> {
        self.config.resolve(Path::new(path)).map_err(|e| match e {
            Error::AccessDenied(_) => Error::AccessDenied(
                "That path is outside the folders this application may touch.".to_string(),
            ),
            other => other,
        })
    }

    fn resolve_inputs(&self, paths: &[String]) -> Result<Vec<PathBuf>, Error> {
        if paths.is_empty() {
            return Err(Error::Refused("No paths were supplied.".to_string()));
        }
        paths.iter().map(|p| self.resolve_input(p)).collect()
    }

    fn resolve_output(&self, path: &str) -> Result<PathBuf, Error> {
        self.config
            .resolve_for_create(Path::new(path))
            .map_err(|e| match e {
                Error::AccessDenied(_) => Error::AccessDenied(
                    "That path is outside the folders this application may touch.".to_string(),
                ),
                other => other,
            })
    }

    /// Lists all shipped built-in film looks.
    pub fn list_looks(&self) -> Vec<BuiltinLookInfo> {
        #[allow(unused_mut)]
        let mut looks = builtin_look_list();
        #[cfg(feature = "film")]
        looks.extend(crate::media::film::film_look_list());
        looks
    }

    /// Lists all 3D LUTs in the managed LUT library.
    pub fn list_luts(&self) -> Result<LutLibraryList, Error> {
        let lut_dir = crate::config::lut_dir(&self.config);
        core_list_luts(&lut_dir)
    }

    /// Plans folder RAW development without writing anything to disk.
    pub fn plan_develop(&self, r: &DevelopRequest) -> Result<Reviewed<DevelopPlan>, Error> {
        let resolved_folder = self.resolve_input(&r.folder)?;
        refuse_card_folder(&resolved_folder, self.volume_resolver)?;
        let dev_folder = resolved_folder.join("Developed");
        self.resolve_output(&dev_folder.to_string_lossy())?;

        let folder_raw_names = raw_names(&resolved_folder)?;
        let look_opt = if r.look.is_empty() {
            None
        } else {
            Some(r.look.clone())
        };

        let mut items = Vec::new();
        let mut missing_skipped = Vec::new();

        if r.only.is_empty() {
            for name in folder_raw_names {
                items.push(DevelopItem {
                    name,
                    look: look_opt.clone(),
                    exposure_ev: r.exposure_ev,
                    grain: r.grain,
                });
            }
        } else {
            for name in &r.only {
                if name.is_empty()
                    || name == "."
                    || name == ".."
                    || name.contains('/')
                    || name.contains('\\')
                {
                    items.push(DevelopItem {
                        name: name.clone(),
                        look: look_opt.clone(),
                        exposure_ev: r.exposure_ev,
                        grain: r.grain,
                    });
                } else if !resolved_folder.join(name).is_file() {
                    missing_skipped.push(Skip {
                        file: name.clone(),
                        reason: "not found in folder".to_string(),
                    });
                } else {
                    items.push(DevelopItem {
                        name: name.clone(),
                        look: look_opt.clone(),
                        exposure_ev: r.exposure_ev,
                        grain: r.grain,
                    });
                }
            }
        }

        let lut_dir = crate::config::lut_dir(&self.config);
        let film_data_dir = crate::config::film_data_dir(&self.config);
        let store =
            FolderStore::with_film_data(resolved_folder, lut_dir.clone(), Some(film_data_dir));
        let existing = store.existing_outputs()?;
        let mut plan = core_plan_develop(&items, r.format, &existing);
        plan.skipped.extend(missing_skipped);

        let mut hasher = Sha256::new();
        let plan_bytes = serde_json::to_vec(&plan)
            .map_err(|e| Error::Internal(format!("failed to serialize develop plan: {e}")))?;
        hasher.update(&plan_bytes);

        let mut non_builtin_looks: BTreeSet<&str> = plan
            .actions
            .iter()
            .map(|a| a.look.as_str())
            .filter(|l| !l.starts_with("builtin:") && !l.starts_with("film:"))
            .collect();
        if !r.look.is_empty() && !r.look.starts_with("builtin:") && !r.look.starts_with("film:") {
            non_builtin_looks.insert(r.look.as_str());
        }
        for look_id in non_builtin_looks {
            if let Some((_, lut)) = find_lut_by_name_or_sha256(&lut_dir, look_id)? {
                hasher.update(lut.sha256.as_bytes());
            }
        }

        let plan_sha256 = phototools_core::ingest::scanner::hex(&hasher.finalize());
        Ok(Reviewed { plan, plan_sha256 })
    }

    /// Executes folder RAW development. When `expected` is `Some`, re-plans and refuses
    /// if the plan's SHA-256 differs from `expected` before writing anything.
    pub fn run_develop(
        &self,
        r: &DevelopRequest,
        expected: Option<&str>,
        p: &dyn Progress,
    ) -> Result<DevelopSummary, Error> {
        let reviewed = self.plan_develop(r)?;
        if let Some(exp) = expected {
            if reviewed.plan_sha256 != exp {
                return Err(Error::Refused(format!(
                    "The develop plan changed since it was reviewed (expected {exp}, found {}); run plan_develop again",
                    reviewed.plan_sha256
                )));
            }
        }

        let resolved_folder = self.resolve_input(&r.folder)?;
        let lut_dir = crate::config::lut_dir(&self.config);
        let film_data_dir = crate::config::film_data_dir(&self.config);
        let mut store = FolderStore::with_film_data(resolved_folder, lut_dir, Some(film_data_dir));
        Ok(execute_develop(reviewed.plan, &mut store, p))
    }

    fn resolve_batch_lut(&self, lut_query: &str) -> Result<(PathBuf, Lut), Error> {
        if lut_query.trim().is_empty() {
            return Err(Error::Refused("LUT name or path is empty".to_string()));
        }
        let lut_dir = crate::config::lut_dir(&self.config);
        if !lut_query.contains('/') && !lut_query.contains('\\') && !lut_query.starts_with('.') {
            if let Some(found) = find_lut_by_name_or_sha256(&lut_dir, lut_query)? {
                return Ok(found);
            }
            return Err(Error::Refused(format!(
                "LUT '{lut_query}' was not found in the library"
            )));
        }

        let resolved_path = self.resolve_input(lut_query)?;
        let is_cube = resolved_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cube"));
        if !is_cube {
            return Err(Error::Refused(format!(
                "LUT '{lut_query}' must be a .cube file"
            )));
        }
        let lut = Lut::from_file(&resolved_path)?;
        Ok((resolved_path, lut))
    }

    /// Plans batch 3D LUT grading without writing anything to disk.
    pub fn plan_batch(&self, r: &BatchRequest) -> Result<Reviewed<Plan<BulkLutAction>>, Error> {
        let resolved_inputs = self.resolve_inputs(&r.inputs)?;
        let resolved_out = self.resolve_output(&r.out_dir)?;
        let (lut_path, lut) = self.resolve_batch_lut(&r.lut)?;

        let mut params = BulkLutParams::new(resolved_inputs, lut_path, r.intensity, resolved_out);
        params.recursive = r.recursive;

        let plan = BulkLutTool
            .plan_with_resolver(&params, self.volume_resolver)?
            .data;

        let mut hasher = Sha256::new();
        let plan_bytes = serde_json::to_vec(&plan)
            .map_err(|e| Error::Internal(format!("failed to serialize batch plan: {e}")))?;
        hasher.update(&plan_bytes);
        hasher.update(lut.sha256.as_bytes());
        let plan_sha256 = phototools_core::ingest::scanner::hex(&hasher.finalize());

        Ok(Reviewed { plan, plan_sha256 })
    }

    /// Executes batch 3D LUT grading. When `expected` is `Some`, re-plans and refuses
    /// if the plan's SHA-256 differs from `expected` before writing anything.
    pub fn run_batch(
        &self,
        r: &BatchRequest,
        expected: Option<&str>,
        p: &dyn Progress,
    ) -> Result<BulkLutSummary, Error> {
        let reviewed = self.plan_batch(r)?;
        if let Some(exp) = expected {
            if reviewed.plan_sha256 != exp {
                return Err(Error::Refused(format!(
                    "The batch plan changed since it was reviewed (expected {exp}, found {}); run plan_batch again",
                    reviewed.plan_sha256
                )));
            }
        }

        Ok(BulkLutTool.apply_with(reviewed.plan, p, None)?.data)
    }

    /// Exports a single photograph using its saved `.photoedit` sidecar (or default recipe).
    pub fn export(&self, r: &ExportRequest) -> Result<ExportResult, Error> {
        let resolved_path = self.resolve_input(&r.path)?;
        let resolved_out = self.resolve_output(&r.out_dir)?;

        if phototools_core::media::raw::is_raw(&resolved_path) {
            return Err(Error::Refused(
                "RAW files are developed in the Develop tab. Open the JPEG or TIFF it writes."
                    .to_string(),
            ));
        }

        let recipe = load_recipe_for_image(&resolved_path)?.unwrap_or_default();
        let lut_dir = crate::config::lut_dir(&self.config);
        let lut_opt = match &recipe.lut {
            Some(lut_ref) => {
                let (_, lut) = resolve_recipe_lut(&lut_dir, lut_ref)?;
                Some(lut)
            }
            None => None,
        };

        export_edited_image_with_options_with_resolver(
            &resolved_path,
            &recipe,
            lut_opt.as_ref(),
            &resolved_out,
            &r.options,
            self.volume_resolver,
        )
    }
}
