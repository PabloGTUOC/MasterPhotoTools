//! The archive operations (F1–F9)

pub mod f1_dates;
pub mod f2_takeout;
pub mod f3_rename;
pub mod f4_split;
pub mod f5_contact;
pub mod f6_transform;
pub mod f7_border;
pub mod f8_tiff;
pub mod f9_browser;

use crate::jobs::{Progress, ToolResult};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skip {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan<T> {
    pub actions: Vec<T>,
    pub skipped: Vec<Skip>,
}

pub trait Tool {
    type Params;
    type Action;
    type Summary;

    /// Dry run. Never touches disk (specification principle 5).
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>>;
    fn apply(&self, plan: Plan<Self::Action>, progress: &dyn Progress)
        -> ToolResult<Self::Summary>;
}

/// Expand a mix of files and directories into the acceptable files among them.
///
/// Anything rejected is reported as a [`Skip`] with a reason rather than
/// silently dropped: a caller that passed twenty files and got twelve outputs
/// needs to know why.
pub fn expand_inputs(
    inputs: &[std::path::PathBuf],
    recursive: bool,
    accepted: &[&str],
) -> (Vec<std::path::PathBuf>, Vec<Skip>) {
    use std::collections::BTreeSet;

    let acceptable = |path: &std::path::Path| -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| accepted.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false)
    };

    let mut files: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut skipped = Vec::new();

    for input in inputs {
        if !input.exists() {
            skipped.push(Skip {
                file: input.to_string_lossy().to_string(),
                reason: "File not found".into(),
            });
            continue;
        }

        if input.is_file() {
            if acceptable(input) {
                files.insert(input.clone());
            } else {
                skipped.push(Skip {
                    file: input.to_string_lossy().to_string(),
                    reason: "Not a file type this tool accepts".into(),
                });
            }
            continue;
        }

        // A directory contributes the acceptable files inside it.
        let mut dirs = vec![input.clone()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                skipped.push(Skip {
                    file: dir.to_string_lossy().to_string(),
                    reason: "Directory could not be read".into(),
                });
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if recursive {
                        dirs.push(path);
                    }
                } else if acceptable(&path) {
                    files.insert(path);
                }
            }
        }
    }

    (files.into_iter().collect(), skipped)
}
