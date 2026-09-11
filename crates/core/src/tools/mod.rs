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

/// One output, and the photograph it was made from.
pub struct Derived {
    pub source: std::path::PathBuf,
    pub output: std::path::PathBuf,
    /// What was actually written, for the pixel-dimension tags.
    pub width: u32,
    pub height: u32,
}

/// Carry each output's metadata over from the photograph it came from.
///
/// **Every tool that writes a new image calls this**, because a derivative with
/// no metadata is a photograph that has lost its date, its camera and its
/// position — and the position is the one nobody notices until it is gone. A
/// geotagged frame that goes through the border tool used to arrive at Google
/// Photos with no location and no date, filed under the day it was uploaded.
///
/// `upright` says whether the tool decoded with the EXIF orientation applied. A
/// tool that did has already rotated the pixels, so the tag must be reset or
/// viewers rotate them again; a tool that did not must keep the tag it was
/// given. `f4`, `f6` and `f7` decode oriented; `f8` reads TIFF pages as they
/// are.
///
/// **One `exiftool` for the whole batch** (G4), started once here rather than
/// per file. A failure to copy is reported as a [`Skip`] against that output
/// and does not fail the run: the image was written and is usable, and losing
/// it because its metadata could not be carried would be the worse outcome.
/// Returning the skips rather than swallowing them is what lets a summary say
/// so (G10).
pub fn carry_metadata(derived: &[Derived], upright: bool) -> Vec<Skip> {
    if derived.is_empty() {
        return Vec::new();
    }

    let mut writer = match crate::media::ExifWriter::start() {
        Ok(writer) => writer,
        Err(e) => {
            // Everything was written; only the metadata is missing. Say so once
            // per output rather than pretending it succeeded.
            return derived
                .iter()
                .map(|d| Skip {
                    file: d.output.to_string_lossy().to_string(),
                    reason: format!("metadata could not be carried over: {e}"),
                })
                .collect();
        }
    };

    let mut skipped = Vec::new();
    for item in derived {
        if let Err(e) = writer.copy_metadata_to_derivative(
            &item.source,
            &item.output,
            item.width,
            item.height,
            upright,
        ) {
            skipped.push(Skip {
                file: item.output.to_string_lossy().to_string(),
                reason: format!("metadata could not be carried over: {e}"),
            });
        }
    }

    let _ = writer.close();
    skipped
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
