//! F3 — batch rename to a consistent, sortable scheme.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::read_meta;
use crate::tools::{Plan, Skip, Tool};
use chrono::NaiveDateTime;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenameOrder {
    /// By best metadata datetime, falling back to modification time, then name.
    Capture,
    /// By the first integer found in the filename, then name.
    Numeric,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRenameParams {
    pub paths: Vec<PathBuf>,
    pub date: Option<String>,
    pub subject: Option<String>,
    pub camera: Option<String>,
    pub film: Option<String>,
    pub order: RenameOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRenameAction {
    pub source: PathBuf,
    pub target: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchRenameSummary {
    pub renamed: Vec<BatchRenameAction>,
    pub failures: Vec<(PathBuf, String)>,
}

/// The minimum length a sanitised date block must reach to be used.
const MIN_DATE_LENGTH: usize = 6;

/// Sanitise the date block: keep only digits and `-`, and require six characters.
///
/// Accepts `YYYYMM`, `YYYYMMDD` and `YYYY-MM-DD`.
pub fn sanitise_date(raw: &str) -> Option<String> {
    let kept: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    if kept.len() >= MIN_DATE_LENGTH {
        Some(kept)
    } else {
        None
    }
}

/// Sanitise a non-date block: drop spaces, turn `_` into `-`, then strip
/// anything outside `[A-Za-z0-9-]`.
pub fn sanitise_block(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '_' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

/// Assemble `<date>-<subject>-<camera>-<film>`, omitting empty blocks.
pub fn build_prefix(
    date: Option<&str>,
    subject: Option<&str>,
    camera: Option<&str>,
    film: Option<&str>,
) -> String {
    let mut blocks = Vec::new();
    if let Some(d) = date.and_then(sanitise_date) {
        blocks.push(d);
    }
    for raw in [subject, camera, film].into_iter().flatten() {
        let block = sanitise_block(raw);
        if !block.is_empty() {
            blocks.push(block);
        }
    }
    blocks.join("-")
}

/// The first integer in a filename, for `numeric` ordering.
fn leading_number(name: &str) -> Option<u64> {
    let mut digits = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

/// The sort key for one file under a given ordering.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Capture(Option<NaiveDateTime>, String),
    Numeric(Option<u64>, String),
}

fn sort_key(path: &std::path::Path, order: RenameOrder) -> SortKey {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    match order {
        RenameOrder::Numeric => SortKey::Numeric(leading_number(&name), name),
        RenameOrder::Capture => {
            // Best metadata datetime, then modification time, then filename.
            let at = read_meta(path)
                .ok()
                .and_then(|m| m.capture)
                .or_else(|| crate::tools::f1_dates::modified_time(path));
            SortKey::Capture(at, name)
        }
    }
}

pub struct BatchRenamerTool;

/// Whether a folder should contribute this entry.
///
/// A leading dot is the Unix convention for "not part of the visible contents",
/// and on macOS `.DS_Store` is in practically every folder a person has opened.
fn is_hidden(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

impl Tool for BatchRenamerTool {
    type Params = BatchRenameParams;
    type Action = BatchRenameAction;
    type Summary = BatchRenameSummary;

    /// Produce the `(source, new name)` plan. Touches no file.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let mut skipped = Vec::new();

        // Missing inputs are reported but must not consume a sequence number.
        // Nor may a file listed twice: it would be given two sequence numbers,
        // and the second rename would fail with its source already moved.
        let mut present: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // A directory contributes the files inside it; it is never itself
        // renamed. Every other tool that takes "files or folders" expands one
        // (`expand_inputs`), and F3 did not — so a folder passed `exists()`,
        // took a sequence number, and the folder itself was renamed while the
        // photographs inside it kept their names. That is destructive, silent,
        // and the opposite of what the person asked for.
        //
        // Non-recursive, because F3 has no recursion flag to honour: a
        // subdirectory is reported rather than descended into, so a nested
        // batch is a visible decision instead of a surprise.
        let mut inputs: Vec<PathBuf> = Vec::new();
        for path in &p.paths {
            if !path.exists() {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: "File not found".into(),
                });
                continue;
            }

            if path.is_dir() {
                let Ok(entries) = std::fs::read_dir(path) else {
                    skipped.push(Skip {
                        file: path.to_string_lossy().to_string(),
                        reason: "Directory could not be read".into(),
                    });
                    continue;
                };
                for entry in entries.flatten() {
                    let child = entry.path();
                    if child.is_dir() {
                        skipped.push(Skip {
                            file: child.to_string_lossy().to_string(),
                            reason: "Subfolder — list it directly to rename what is inside".into(),
                        });
                    } else if is_hidden(&child) {
                        // F3 renames any file type on purpose — it has no
                        // accepted-extensions list, because a photograph can be
                        // in any format. That makes `.DS_Store` a candidate: it
                        // was taking sequence number 01 and shifting every
                        // photograph by one, as well as being renamed itself.
                        //
                        // A dotfile named explicitly is still honoured; this
                        // only governs what a *folder* contributes, where
                        // nobody asked for it by name.
                        skipped.push(Skip {
                            file: child.to_string_lossy().to_string(),
                            reason: "Hidden file — name it directly to rename it".into(),
                        });
                    } else {
                        inputs.push(child);
                    }
                }
                continue;
            }

            inputs.push(path.clone());
        }

        for path in inputs {
            let identity = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen.insert(identity) {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: "Listed more than once in this batch".into(),
                });
                continue;
            }
            present.push(path);
        }

        // Sort keys are computed in parallel: `capture` ordering reads metadata
        // for every file, which is the expensive part of a large batch.
        let mut keyed: Vec<(SortKey, PathBuf)> = present
            .into_par_iter()
            .map(|path| (sort_key(&path, p.order), path))
            .collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));

        let prefix = build_prefix(
            p.date.as_deref(),
            p.subject.as_deref(),
            p.camera.as_deref(),
            p.film.as_deref(),
        );

        let width = std::cmp::max(2, keyed.len().to_string().len());
        let mut claimed: HashSet<PathBuf> = HashSet::new();
        let mut actions = Vec::new();

        for (index, (_, source)) in keyed.iter().enumerate() {
            let extension = source
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let sequence = format!("{:0width$}", index + 1, width = width);

            let stem = if prefix.is_empty() {
                sequence
            } else {
                format!("{prefix}-{sequence}")
            };
            let file_name = if extension.is_empty() {
                stem
            } else {
                format!("{stem}.{extension}")
            };

            let parent = source.parent().unwrap_or(std::path::Path::new("."));
            let target = parent.join(&file_name);

            // A file already at the target name is never overwritten — unless it
            // is this very file, which is a no-op rename.
            if target != *source && target.exists() {
                skipped.push(Skip {
                    file: source.to_string_lossy().to_string(),
                    reason: format!("Would overwrite an existing file: {file_name}"),
                });
                continue;
            }
            if !claimed.insert(target.clone()) {
                skipped.push(Skip {
                    file: source.to_string_lossy().to_string(),
                    reason: format!("Duplicate target within this batch: {file_name}"),
                });
                continue;
            }

            actions.push(BatchRenameAction {
                source: source.clone(),
                target,
            });
        }

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let total = plan.actions.len() as u64;
        let mut summary = BatchRenameSummary::default();

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            if action.source == action.target {
                continue;
            }

            // Re-check immediately before the write: `fs::rename` replaces its
            // target silently on Unix, and the plan may be minutes old.
            if action.target.exists() {
                summary.failures.push((
                    action.source.clone(),
                    format!(
                        "Target appeared since the plan was made: {}",
                        action.target.display()
                    ),
                ));
                continue;
            }

            match fs::rename(&action.source, &action.target) {
                Ok(()) => summary.renamed.push(action),
                Err(e) => summary
                    .failures
                    .push((action.source.clone(), format!("Rename failed: {e}"))),
            }
        }

        progress.report(total, total, "done");
        Ok(Outcome { data: summary })
    }
}

/// Convenience for callers that only want the plan's names.
pub fn plan_names(plan: &Plan<BatchRenameAction>) -> Vec<String> {
    plan.actions
        .iter()
        .map(|a| {
            a.target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect()
}

impl BatchRenamerTool {
    /// Surface a plan error as an [`Error`] for callers that want one.
    pub fn check(plan: &Plan<BatchRenameAction>) -> Result<(), Error> {
        if plan.actions.is_empty() && !plan.skipped.is_empty() {
            return Err(Error::Config(
                "Every file in the batch was skipped; nothing to rename".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_block_accepts_the_documented_forms() {
        assert_eq!(sanitise_date("202405").as_deref(), Some("202405"));
        assert_eq!(sanitise_date("20240501").as_deref(), Some("20240501"));
        assert_eq!(sanitise_date("2024-05-01").as_deref(), Some("2024-05-01"));
        // Non-digit, non-hyphen characters are stripped.
        assert_eq!(sanitise_date("2024/05/01").as_deref(), Some("20240501"));
    }

    #[test]
    fn a_date_block_shorter_than_six_characters_is_dropped() {
        assert_eq!(sanitise_date("2024"), None);
        assert_eq!(sanitise_date("May"), None);
        assert_eq!(sanitise_date(""), None);
    }

    #[test]
    fn other_blocks_lose_spaces_and_convert_underscores() {
        assert_eq!(sanitise_block("Lisboa"), "Lisboa");
        assert_eq!(sanitise_block("San Sebastian"), "SanSebastian");
        assert_eq!(sanitise_block("PORTRA_400"), "PORTRA-400");
        assert_eq!(sanitise_block("Kodak/Gold!"), "KodakGold");
        assert_eq!(sanitise_block("café"), "caf");
    }

    #[test]
    fn the_prefix_matches_the_specification_example() {
        let prefix = build_prefix(
            Some("2024-05-01"),
            Some("Lisboa"),
            Some("PENTAX17"),
            Some("PORTRA400"),
        );
        assert_eq!(prefix, "2024-05-01-Lisboa-PENTAX17-PORTRA400");
    }

    #[test]
    fn empty_blocks_are_omitted_rather_than_leaving_double_hyphens() {
        assert_eq!(
            build_prefix(Some("202405"), None, Some("PENTAX17"), None),
            "202405-PENTAX17"
        );
        assert_eq!(build_prefix(None, Some("Lisboa"), None, None), "Lisboa");
        assert_eq!(build_prefix(None, None, None, None), "");
        // A block that sanitises to nothing is also omitted.
        assert_eq!(build_prefix(None, Some("!!!"), Some("CAM"), None), "CAM");
    }

    #[test]
    fn numeric_ordering_reads_the_first_integer() {
        assert_eq!(leading_number("IMG_1234.JPG"), Some(1234));
        assert_eq!(leading_number("scan-007-b.tif"), Some(7));
        assert_eq!(leading_number("no-digits.jpg"), None);
        // The *first* run of digits, not the last.
        assert_eq!(leading_number("12_of_34.jpg"), Some(12));
    }

    /// A folder passed to F3 used to be renamed itself, leaving the
    /// photographs inside it untouched — destructive, silent, and the opposite
    /// of the request.
    #[test]
    fn a_folder_contributes_the_files_inside_it_and_is_never_itself_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Holiday");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("a.jpg"), b"x").unwrap();
        std::fs::write(folder.join("b.jpg"), b"y").unwrap();

        let plan = BatchRenamerTool
            .plan(&BatchRenameParams {
                paths: vec![folder.clone()],
                date: None,
                subject: Some("Trip".into()),
                camera: None,
                film: None,
                order: RenameOrder::Numeric,
            })
            .unwrap()
            .data;

        assert_eq!(plan.actions.len(), 2, "both files, and only the files");
        for action in &plan.actions {
            assert_ne!(
                action.source, folder,
                "the folder itself must never be a rename source"
            );
            assert_eq!(action.source.parent().unwrap(), folder);
        }

        let targets: Vec<String> = plan
            .actions
            .iter()
            .map(|a| a.target.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(targets, vec!["Trip-01.jpg", "Trip-02.jpg"]);
    }

    /// Non-recursive: F3 has no recursion flag, so a nested folder is reported
    /// rather than quietly walked.
    #[test]
    fn a_subfolder_is_reported_rather_than_descended_into() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Roll");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        std::fs::write(folder.join("top.jpg"), b"x").unwrap();
        std::fs::write(folder.join("nested/deep.jpg"), b"y").unwrap();

        let plan = BatchRenamerTool
            .plan(&BatchRenameParams {
                paths: vec![folder],
                date: None,
                subject: None,
                camera: None,
                film: None,
                order: RenameOrder::Numeric,
            })
            .unwrap()
            .data;

        assert_eq!(plan.actions.len(), 1, "only the file at the top level");
        assert!(
            plan.skipped.iter().any(|s| s.file.contains("nested")),
            "the subfolder must be reported, not silently ignored: {:?}",
            plan.skipped
        );
    }

    /// A folder does not contribute its hidden files.
    ///
    /// `.DS_Store` is in practically every folder on a Mac, and F3 renames any
    /// file type by design — so it was renamed *and* it took sequence number
    /// 01, shifting every photograph by one.
    #[test]
    fn a_folder_does_not_contribute_its_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("roll");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(".DS_Store"), b"x").unwrap();
        std::fs::write(folder.join("a.jpg"), b"y").unwrap();
        std::fs::write(folder.join("b.jpg"), b"z").unwrap();

        let plan = BatchRenamerTool
            .plan(&BatchRenameParams {
                paths: vec![folder],
                date: None,
                subject: Some("Roll".into()),
                camera: None,
                film: None,
                order: RenameOrder::Numeric,
            })
            .unwrap()
            .data;

        assert_eq!(
            plan.actions.len(),
            2,
            "the two photographs, and nothing else"
        );
        assert!(
            plan.actions
                .iter()
                .all(|a| !a.source.ends_with(".DS_Store")),
            "a hidden file must never be a rename source: {:?}",
            plan.actions
        );
        assert!(
            plan.skipped.iter().any(|s| s.file.ends_with(".DS_Store")),
            "and it is reported rather than silently dropped: {:?}",
            plan.skipped
        );

        // The sequence starts at the first photograph, not at the dotfile.
        let targets: Vec<String> = plan
            .actions
            .iter()
            .map(|a| a.target.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(targets, vec!["Roll-01.jpg", "Roll-02.jpg"]);
    }

    /// A hidden file named directly is still renamed: the person asked for it.
    #[test]
    fn a_hidden_file_named_explicitly_is_still_honoured() {
        let dir = tempfile::tempdir().unwrap();
        let hidden = dir.path().join(".hidden.jpg");
        std::fs::write(&hidden, b"x").unwrap();

        let plan = BatchRenamerTool
            .plan(&BatchRenameParams {
                paths: vec![hidden.clone()],
                date: None,
                subject: Some("Named".into()),
                camera: None,
                film: None,
                order: RenameOrder::Numeric,
            })
            .unwrap()
            .data;

        assert_eq!(plan.actions.len(), 1, "naming it is asking for it");
        assert_eq!(plan.actions[0].source, hidden);
    }
}
