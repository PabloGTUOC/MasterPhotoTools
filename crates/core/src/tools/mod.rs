//! The archive operations (F1–F9)

pub mod bulk_edit;
pub mod edit;
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
pub mod lut;
pub mod lut_library;
pub mod presets;

pub use bulk_edit::{recipe_sha256, BulkEditAction, BulkEditParams, BulkEditSummary, BulkEditTool};
pub use edit::{
    exclusive_create_target, export_edited_image, export_edited_image_with_resolver,
    find_volume_root, is_card_volume, is_card_volume_with_resolver, is_identity, is_sidecar,
    load_recipe, load_recipe_for_image, render_and_write, save_recipe, save_recipe_with_resolver,
    sidecar_path, ExportResult, CURRENT_RECIPE_VERSION, SIDECAR_EXTENSION,
};
pub use lut::{
    sample_frames, BulkLutAction, BulkLutParams, BulkLutSummary, BulkLutTool,
    ACCEPTED as BULK_LUT_ACCEPTED,
};
pub use lut_library::{
    find_lut_by_name_or_sha256, find_lut_by_sha256, import_lut, list_luts, resolve_recipe_lut,
    LutEntry, LutError, LutLibraryList,
};
pub use presets::{
    delete_preset, list_presets, load_preset, rename_preset, save_preset, PresetEntry, PresetError,
    PresetList, PRESET_EXTENSION,
};

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
#[derive(Debug, Clone)]
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
    carry_metadata_with(derived, upright, None)
}

/// Carry metadata across a batch of derivatives, optionally specifying a custom `exiftool` program.
pub fn carry_metadata_with(derived: &[Derived], upright: bool, program: Option<&str>) -> Vec<Skip> {
    if derived.is_empty() {
        return Vec::new();
    }

    let writer_result = match program {
        Some(prog) => {
            crate::media::ExifWriter::start_with(prog, std::time::Duration::from_secs(60))
        }
        None => crate::media::ExifWriter::start(),
    };

    let mut writer = match writer_result {
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

/// Return up to `n` items spread evenly through a slice.
///
/// Picks first, last, and intermediate evenly spaced items rather than the
/// first `n`, because a preview of a batch should show its range, not merely
/// its opening frames.
pub fn sample_evenly<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    let len = items.len();
    if len == 0 || n == 0 {
        return Vec::new();
    }
    if len <= n {
        return items.to_vec();
    }
    if n == 1 {
        return vec![items[0].clone()];
    }

    (0..n)
        .map(|i| {
            let idx = (i * (len - 1)) / (n - 1);
            items[idx].clone()
        })
        .collect()
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
mod tests {
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

    /// sample_evenly returns an empty vec when n is 0 or the input is empty.
    #[test]
    fn sample_evenly_empty_cases() {
        assert!(sample_evenly::<i32>(&[], 3).is_empty());
        assert!(sample_evenly(&[1, 2, 3], 0).is_empty());
    }

    /// sample_evenly returns all items when n >= len.
    #[test]
    fn sample_evenly_short_input() {
        assert_eq!(sample_evenly(&[1, 2, 3], 5), vec![1, 2, 3]);
    }

    /// sample_evenly returns only the first item when n == 1.
    #[test]
    fn sample_evenly_one_item() {
        assert_eq!(sample_evenly(&[10, 20, 30], 1), vec![10]);
    }

    /// sample_evenly spreads evenly: n = 3 of 10 gives indices 0, 4, 9.
    #[test]
    fn sample_evenly_evenly_spaced() {
        let items: Vec<i32> = (0..10).collect();
        assert_eq!(sample_evenly(&items, 3), vec![0, 4, 9]);
    }
}
