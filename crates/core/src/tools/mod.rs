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
pub mod geotag;

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

/// A job's closing line, including when nothing happened.
///
/// The image tools all reported `"{n} written, {m} failed"`, which says nothing
/// when both are zero — and both being zero is the common case when somebody
/// points a tool at a folder whose files are one level down. A run that did
/// nothing has to say why, and the two reasons are different: the inputs held
/// nothing this tool reads, or they held things it declined.
pub fn summarise(
    done: usize,
    noun: &str,
    failed: usize,
    skipped: &[Skip],
    accepted: &[&str],
) -> String {
    if done == 0 && failed == 0 {
        // A tool with no extension list takes anything — F3 renames whatever it
        // is given — so the sentence about what it reads is simply omitted
        // rather than left dangling.
        let reads = if accepted.is_empty() {
            String::new()
        } else {
            format!(
                " This tool reads {}.",
                accepted
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        return match skipped.first() {
            // Something was looked at and declined; the first reason is
            // representative and the count says how widespread it is.
            Some(first) => format!(
                "Nothing to do: {} input(s) skipped, the first because \"{}\".{reads}",
                skipped.len(),
                first.reason
            ),
            // Nothing was even a candidate: an empty folder, or files one level
            // further down than the search went.
            None => format!(
                "Nothing to do: nothing matched.{reads} If the files are inside a subfolder, \
                 tick Include subfolders."
            ),
        };
    }

    let mut line = format!("{done} {noun}, {failed} failed");
    if !skipped.is_empty() {
        line.push_str(&format!(", {} skipped", skipped.len()));
    }
    line
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

#[cfg(test)]
mod summary_tests {
    use super::*;

    /// A run that did nothing has to say why.
    ///
    /// "0 written, 0 failed" is what somebody sees when they point a tool at a
    /// folder whose files are one level further down, and it tells them
    /// nothing at all.
    #[test]
    fn a_run_that_matched_nothing_explains_itself() {
        let line = summarise(0, "pages written", 0, &[], &["tif", "tiff"]);
        assert!(
            line.contains(".tif"),
            "it names what the tool reads: {line}"
        );
        assert!(
            line.contains("Include subfolders"),
            "and the likeliest cause: {line}"
        );
    }

    /// Things looked at and declined are a different answer from nothing found.
    #[test]
    fn a_run_that_declined_everything_says_why_it_declined() {
        let skipped = vec![Skip {
            file: "notes.txt".into(),
            reason: "Not a file type this tool accepts".into(),
        }];
        let line = summarise(0, "pages written", 0, &skipped, &["tif"]);
        assert!(line.contains("1 input(s) skipped"), "{line}");
        assert!(line.contains("Not a file type"), "{line}");
    }

    /// A normal run reads as before, with the skips no longer hidden.
    #[test]
    fn a_run_that_did_something_reports_it_and_its_skips() {
        assert_eq!(
            summarise(4, "pages written", 0, &[], &["tif"]),
            "4 pages written, 0 failed"
        );

        let skipped = vec![Skip {
            file: "x".into(),
            reason: "Not a file type this tool accepts".into(),
        }];
        assert_eq!(
            summarise(4, "pages written", 1, &skipped, &["tif"]),
            "4 pages written, 1 failed, 1 skipped"
        );
    }
}
