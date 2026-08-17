use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::tools::{Plan, Skip, Tool};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum RenameOrder {
    Capture,
    Numeric,
}

pub struct BatchRenameParams {
    pub paths: Vec<PathBuf>,
    pub date: Option<String>,
    pub subject: Option<String>,
    pub camera: Option<String>,
    pub film: Option<String>,
    pub order: RenameOrder,
}

#[derive(Debug, Clone)]
pub struct BatchRenameAction {
    pub source: PathBuf,
    pub target: PathBuf,
}

pub struct BatchRenamerTool;

fn sanitize_date(val: &str) -> Option<String> {
    let re = Regex::new(r"[^\d-]").unwrap();
    let sanitized = re.replace_all(val, "").to_string();
    if sanitized.len() >= 6 {
        Some(sanitized)
    } else {
        None
    }
}

fn sanitize_block(val: &str) -> String {
    let s = val.replace(" ", "").replace("_", "-");
    let re = Regex::new(r"[^A-Za-z0-9-]").unwrap();
    re.replace_all(&s, "").to_string()
}

fn get_numeric_sort_key(name: &str) -> (i64, String) {
    let re = Regex::new(r"\d+").unwrap();
    if let Some(mat) = re.find(name) {
        if let Ok(num) = mat.as_str().parse::<i64>() {
            return (num, name.to_string());
        }
    }
    (i64::MAX, name.to_string())
}

impl Tool for BatchRenamerTool {
    type Params = BatchRenameParams;
    type Action = BatchRenameAction;
    type Summary = ();

    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let mut actions = Vec::new();
        let mut skipped = Vec::new();

        let mut blocks = Vec::new();
        if let Some(d) = &p.date {
            if let Some(sd) = sanitize_date(d) {
                blocks.push(sd);
            }
        }
        if let Some(s) = &p.subject {
            blocks.push(sanitize_block(s));
        }
        if let Some(c) = &p.camera {
            blocks.push(sanitize_block(c));
        }
        if let Some(f) = &p.film {
            blocks.push(sanitize_block(f));
        }

        let prefix = blocks.join("-");

        // Sort the paths based on order
        let mut sorted_paths = p.paths.clone();
        match p.order {
            RenameOrder::Capture => {
                sorted_paths.sort_by(|a, b| {
                    // For full accuracy, fallback to fs_date then name
                    let date_a = crate::tools::f1_dates::get_fs_time(a);
                    let date_b = crate::tools::f1_dates::get_fs_time(b);
                    match date_a.cmp(&date_b) {
                        std::cmp::Ordering::Equal => a.file_name().cmp(&b.file_name()),
                        other => other,
                    }
                });
            }
            RenameOrder::Numeric => {
                sorted_paths.sort_by(|a, b| {
                    let name_a = a.file_name().unwrap_or_default().to_string_lossy();
                    let name_b = b.file_name().unwrap_or_default().to_string_lossy();
                    get_numeric_sort_key(&name_a).cmp(&get_numeric_sort_key(&name_b))
                });
            }
        }

        let digits = std::cmp::max(2, sorted_paths.len().to_string().len());
        let mut target_set = HashSet::new();

        for (i, path) in sorted_paths.iter().enumerate() {
            if !path.exists() {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: "File not found".into(),
                });
                continue;
            }

            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let parent = path.parent().unwrap();

            let seq = format!("{:0width$}", i + 1, width = digits);

            let target_name = if prefix.is_empty() {
                if ext.is_empty() {
                    seq.clone()
                } else {
                    format!("{}.{}", seq, ext)
                }
            } else {
                if ext.is_empty() {
                    format!("{}-{}", prefix, seq)
                } else {
                    format!("{}-{}.{}", prefix, seq, ext)
                }
            };

            let target_path = parent.join(&target_name);

            if target_path.exists() && target_path != *path {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: format!("Collision with existing file: {}", target_name),
                });
                continue;
            }

            if target_set.contains(&target_path) {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: format!("Duplicate target within batch: {}", target_name),
                });
                continue;
            }

            target_set.insert(target_path.clone());
            actions.push(BatchRenameAction {
                source: path.clone(),
                target: target_path,
            });
        }

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        _progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        for action in plan.actions {
            if action.source != action.target {
                if let Err(e) = fs::rename(&action.source, &action.target) {
                    return Err(Error::Internal(format!(
                        "Failed to rename {:?}: {}",
                        action.source, e
                    )));
                }
            }
        }
        Ok(Outcome { data: () })
    }
}
