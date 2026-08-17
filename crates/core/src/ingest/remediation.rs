//! Remediation (F13).
//!
//! What can be done about each failure F12 found, and how to do it to four
//! hundred shots at once.
//!
//! > **Every action must be available as a bulk apply to all shots sharing a
//! > failure.**
//!
//! That is not a UI convenience. A 10 MP ceiling means a 24–45 MP body fails the
//! resolution check on virtually every frame, so **resizing is the normal path,
//! not the exception** (F12). A design that prompts per file is a design that
//! asks someone four hundred questions.
//!
//! Follows the mandatory `plan`/`apply` split (build plan §7): `plan` decides
//! what would happen and touches nothing.

use crate::config::Thresholds;
use crate::error::Error;
use crate::ingest::grouping::Shot;
use crate::ingest::validation::{CardValidation, ClockOffset, FailureClass};
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::{self, image_ops};
use crate::tools::{Plan, Skip, Tool};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What can be done about a failure (F13's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Enter a date by hand.
    EnterDateManually,
    /// Take the batch median as this frame's date.
    DeriveFromBatchMedian,
    /// Fall back to the file's modification time.
    UseFileModificationTime,
    /// Re-date one frame by hand.
    RedateManually,
    /// Shift the whole batch by `now − median`. Reuses F1's `shift` mode.
    BulkShift,
    /// Accept the finding and publish anyway.
    PublishAnyway,
    /// Scale down to the megapixel ceiling.
    Resize,
    /// Step quality down until the byte cap is met.
    ReencodeLower,
    Skip,
}

impl ActionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionKind::EnterDateManually => "enter_date_manually",
            ActionKind::DeriveFromBatchMedian => "derive_from_batch_median",
            ActionKind::UseFileModificationTime => "use_file_modification_time",
            ActionKind::RedateManually => "redate_manually",
            ActionKind::BulkShift => "bulk_shift",
            ActionKind::PublishAnyway => "publish_anyway",
            ActionKind::Resize => "resize",
            ActionKind::ReencodeLower => "reencode_lower",
            ActionKind::Skip => "skip",
        }
    }

    /// True if this action rewrites pixels rather than metadata.
    pub fn rewrites_image(&self) -> bool {
        matches!(self, ActionKind::Resize | ActionKind::ReencodeLower)
    }
}

/// The actions F13 offers for a failure.
///
/// This is the specification's table, in one place, so that a UI cannot offer an
/// action the specification does not sanction and cannot forget one it does.
pub fn actions_for(failure: FailureClass) -> Vec<ActionKind> {
    match failure {
        FailureClass::NoDate => vec![
            ActionKind::EnterDateManually,
            ActionKind::DeriveFromBatchMedian,
            ActionKind::UseFileModificationTime,
            ActionKind::Skip,
        ],
        FailureClass::DateOutOfRangeIsolated => vec![
            ActionKind::RedateManually,
            ActionKind::PublishAnyway,
            ActionKind::Skip,
        ],
        FailureClass::DateOutOfRangeBatch => vec![
            ActionKind::BulkShift,
            ActionKind::PublishAnyway,
            ActionKind::Skip,
        ],
        FailureClass::TooManyPixels => vec![
            ActionKind::Resize,
            ActionKind::PublishAnyway,
            ActionKind::Skip,
        ],
        FailureClass::TooLarge => vec![
            ActionKind::ReencodeLower,
            ActionKind::Resize,
            ActionKind::Skip,
        ],
    }
}

/// What F13 offers by default for a failure.
///
/// F12's consequence: with a 10 MP ceiling and a modern body, resizing is what
/// happens to nearly every frame, so it is the default rather than a choice
/// someone makes four hundred times. Nothing that loses a photograph is ever a
/// default — `Skip` and `PublishAnyway` are always deliberate.
pub fn default_action(failure: FailureClass) -> Option<ActionKind> {
    match failure {
        FailureClass::TooManyPixels => Some(ActionKind::Resize),
        FailureClass::TooLarge => Some(ActionKind::ReencodeLower),
        FailureClass::DateOutOfRangeBatch => Some(ActionKind::BulkShift),
        // A missing or isolated wrong date needs a person: there is no answer
        // the system can infer that is better than the one they know.
        FailureClass::NoDate | FailureClass::DateOutOfRangeIsolated => None,
    }
}

/// A bulk instruction: one action, applied to every shot sharing one failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkRequest {
    pub failure: FailureClass,
    pub action: ActionKind,
    /// Required by `EnterDateManually` and `RedateManually`.
    pub date: Option<NaiveDateTime>,
    /// Where rewritten images are written. The card is never written to (G5).
    pub output_dir: PathBuf,
}

/// Everything a bulk apply needs.
pub struct RemediationParams<'a> {
    pub shots: &'a [Shot],
    pub validation: &'a CardValidation,
    pub thresholds: Thresholds,
    pub request: BulkRequest,
}

/// One shot's share of a bulk action, decided but not yet performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAction {
    pub stem: String,
    pub source: PathBuf,
    pub action: ActionKind,
    /// Set for resizes: the dimensions the frame would end up at.
    pub target_dimensions: Option<(u32, u32)>,
    /// Set when the action writes a new file.
    pub destination: Option<PathBuf>,
    /// Set for date actions: the date that would be written.
    pub new_date: Option<NaiveDateTime>,
    /// The byte cap the encoder must meet.
    ///
    /// F13: "The result is encoded as JPEG, stepping quality down 95 → 88 → 82
    /// → 75 until the byte cap is satisfied." That applies to a resize as much
    /// as to a re-encode, so both carry it.
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemediationSummary {
    pub rewritten: Vec<PathBuf>,
    pub redated: Vec<String>,
    pub accepted: Vec<String>,
    pub skipped: Vec<String>,
    pub failures: Vec<Skip>,
    /// Files that are still over the byte cap at the bottom of the quality
    /// ladder. Written, but reported rather than passed off as fixed.
    pub still_too_large: Vec<PathBuf>,
    /// True when every rewritten file carried its EXIF forward.
    ///
    /// F13 marks preserving EXIF mandatory: dropping it destroys the capture
    /// date that was just validated, and Google Photos would then file the
    /// photograph under its upload date.
    pub exif_preserved: bool,
}

pub struct RemediationTool;

impl Tool for RemediationTool {
    type Params = RemediationParams<'static>;
    type Action = PlannedAction;
    type Summary = RemediationSummary;

    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        plan_bulk(p)
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        apply_bulk(plan, progress)
    }
}

/// Decide what a bulk action would do. **Touches no files.**
///
/// Free in the lifetime so a caller can plan over borrowed data without having
/// to satisfy the `'static` the `Tool` trait's associated type implies.
pub fn plan_bulk(p: &RemediationParams<'_>) -> ToolResult<Plan<PlannedAction>> {
    let grouped = p.validation.by_failure();
    let Some(indices) = grouped.get(&p.request.failure) else {
        // Nothing shares this failure. Not an error — a card can be validated
        // twice, and the second time the failure may be gone.
        return Ok(Outcome {
            data: Plan {
                actions: Vec::new(),
                skipped: Vec::new(),
            },
        });
    };

    if !actions_for(p.request.failure).contains(&p.request.action) {
        return Err(Error::Internal(format!(
            "{} is not an action F13 offers for {}",
            p.request.action.as_str(),
            p.request.failure.as_str()
        )));
    }

    let mut actions = Vec::new();
    let mut skipped = Vec::new();

    for &index in indices {
        let Some(shot) = p.shots.get(index) else {
            continue;
        };
        let candidate = shot.candidate();

        match resolve(p, shot) {
            Ok(Some(planned)) => actions.push(planned),
            Ok(None) => {}
            Err(reason) => skipped.push(Skip {
                file: candidate.rel_path.clone(),
                reason,
            }),
        }
    }

    Ok(Outcome {
        data: Plan { actions, skipped },
    })
}

/// What one shot's share of the bulk action would be.
fn resolve(p: &RemediationParams<'_>, shot: &Shot) -> Result<Option<PlannedAction>, String> {
    let candidate = shot.candidate();
    let action = p.request.action;

    let base = PlannedAction {
        stem: shot.stem.clone(),
        source: candidate.path.clone(),
        action,
        target_dimensions: None,
        destination: None,
        new_date: None,
        max_bytes: None,
    };

    match action {
        ActionKind::Skip => Ok(Some(base)),
        ActionKind::PublishAnyway => Ok(Some(base)),

        ActionKind::EnterDateManually | ActionKind::RedateManually => {
            let date = p
                .request
                .date
                .ok_or_else(|| "No date was supplied for a manual date action".to_string())?;
            Ok(Some(PlannedAction {
                new_date: Some(date),
                ..base
            }))
        }

        ActionKind::DeriveFromBatchMedian => {
            let median = batch_median(p)
                .ok_or_else(|| "The card has no dated frame to take a median from".to_string())?;
            Ok(Some(PlannedAction {
                new_date: Some(median),
                ..base
            }))
        }

        ActionKind::UseFileModificationTime => {
            let modified = std::fs::metadata(&candidate.path)
                .and_then(|m| m.modified())
                .map_err(|e| format!("Could not read the modification time: {e}"))?;
            let seconds = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "The file's modification time predates 1970".to_string())?
                .as_secs() as i64;
            let date = chrono::DateTime::from_timestamp(seconds, 0)
                .ok_or_else(|| "The file's modification time is not a valid date".to_string())?
                .naive_utc();
            Ok(Some(PlannedAction {
                new_date: Some(date),
                ..base
            }))
        }

        ActionKind::BulkShift => {
            let offset: &ClockOffset = p
                .validation
                .clock_offset
                .as_ref()
                .ok_or_else(|| "There is no batch offset to shift by".to_string())?;
            // The shift is applied by F1's `shift` mode; what is planned here is
            // the resulting date, so a dry run can show it.
            let capture = candidate
                .capture
                .ok_or_else(|| "A frame with no date cannot be shifted".to_string())?;
            let delta = chrono::Duration::seconds(
                (chrono::Utc::now().naive_utc() - offset.median).num_seconds(),
            );
            Ok(Some(PlannedAction {
                new_date: Some(capture + delta),
                ..base
            }))
        }

        ActionKind::Resize => {
            if candidate.dimensions_unknown() {
                return Err("No dimensions in metadata, so there is nothing to scale".into());
            }
            let target = image_ops::dimensions_for_megapixels(
                candidate.width,
                candidate.height,
                p.thresholds.max_megapixels,
            );
            let Some((w, h)) = target else {
                // Already within the ceiling. Not a failure — just nothing to do.
                return Ok(None);
            };
            Ok(Some(PlannedAction {
                target_dimensions: Some((w, h)),
                destination: Some(destination_for(p, shot)),
                max_bytes: Some(p.thresholds.max_output_bytes),
                ..base
            }))
        }

        ActionKind::ReencodeLower => Ok(Some(PlannedAction {
            destination: Some(destination_for(p, shot)),
            max_bytes: Some(p.thresholds.max_output_bytes),
            ..base
        })),
    }
}

fn destination_for(p: &RemediationParams<'_>, shot: &Shot) -> PathBuf {
    p.request.output_dir.join(format!("{}.jpg", shot.stem))
}

fn batch_median(p: &RemediationParams<'_>) -> Option<NaiveDateTime> {
    let captures: Vec<NaiveDateTime> = p.shots.iter().filter_map(|s| s.capture()).collect();
    crate::ingest::validation::median_capture(&captures)
}

/// Carry out a planned bulk action.
///
/// Image rewrites go through the EXIF-preserving path, which F13 marks
/// mandatory. Date actions produce the dates; **writing them is F1's job**, and
/// they are returned rather than written here so that one `exiftool` process
/// covers the batch (G4).
pub fn apply_bulk(
    plan: Plan<PlannedAction>,
    progress: &dyn Progress,
) -> ToolResult<RemediationSummary> {
    let total = plan.actions.len() as u64;
    let mut summary = RemediationSummary {
        failures: plan.skipped,
        exif_preserved: true,
        ..Default::default()
    };

    let mut rewrites = 0usize;

    for (index, action) in plan.actions.iter().enumerate() {
        if progress.cancelled() {
            break;
        }

        match action.action {
            ActionKind::Skip => summary.skipped.push(action.stem.clone()),
            ActionKind::PublishAnyway => summary.accepted.push(action.stem.clone()),

            ActionKind::EnterDateManually
            | ActionKind::RedateManually
            | ActionKind::DeriveFromBatchMedian
            | ActionKind::UseFileModificationTime
            | ActionKind::BulkShift => summary.redated.push(action.stem.clone()),

            ActionKind::Resize | ActionKind::ReencodeLower => {
                match rewrite(action) {
                    Ok(result) => {
                        rewrites += 1;
                        if !result.carried {
                            summary.exif_preserved = false;
                        }
                        if let Some(destination) = &action.destination {
                            summary.rewritten.push(destination.clone());
                            if !result.fits {
                                // Written, but the ladder bottomed out before
                                // meeting the cap. Reported rather than passed
                                // off as fixed.
                                summary.still_too_large.push(destination.clone());
                            }
                        }
                    }
                    Err(e) => summary.failures.push(Skip {
                        file: action.source.to_string_lossy().to_string(),
                        reason: e.to_string(),
                    }),
                }
            }
        }

        progress.report(index as u64 + 1, total, action.action.as_str());
    }

    // "Every file kept its EXIF" is only a claim worth making when files were
    // written. With no rewrites the honest answer is that nothing was at risk.
    if rewrites == 0 {
        summary.exif_preserved = true;
    }

    Ok(Outcome { data: summary })
}

/// Rewrite one image, carrying its metadata forward.
fn rewrite(action: &PlannedAction) -> Result<Rewrite, Error> {
    let destination = action
        .destination
        .as_ref()
        .ok_or_else(|| Error::Internal("a rewrite with no destination".into()))?;

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let decoded = media::decode(&action.source)?;

    let image = match action.target_dimensions {
        Some((w, h)) => image_ops::resize(&decoded, w, h)?,
        None => decoded,
    };

    // F13: "resizing must preserve EXIF. The metadata block is carried forward
    // and PixelXDimension / PixelYDimension updated." Dropping it destroys the
    // capture date that was just validated.
    //
    // The cap drives the quality ladder, 95 -> 88 -> 82 -> 75. Without a cap the
    // ladder would stop at its first rung, which is the whole of what
    // `ReencodeLower` is for.
    let (_quality, fits) = image_ops::reencode_preserving_exif_within(
        &action.source,
        &image,
        destination,
        action.max_bytes.unwrap_or(u64::MAX),
    )?;

    // Whether metadata actually made it across, rather than whether we asked.
    let carried = media::read_meta(destination)
        .map(|m| m.capture.is_some() || m.camera.is_some())
        .unwrap_or(false);

    Ok(Rewrite { carried, fits })
}

/// What one rewrite achieved.
struct Rewrite {
    /// Metadata reached the output.
    carried: bool,
    /// The output is within the byte cap.
    fits: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::scanner::{AssetKind, ScannedAsset};
    use crate::ingest::validation::validate;
    use crate::jobs::InMemoryProgress;
    use chrono::{Datelike, NaiveDate};

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2024, 6, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn at(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn shot_with(stem: &str, w: u32, h: u32, bytes: u64, capture: Option<NaiveDateTime>) -> Shot {
        Shot {
            stem: stem.into(),
            assets: vec![ScannedAsset {
                path: PathBuf::from(format!("{stem}.JPG")),
                rel_path: format!("{stem}.JPG"),
                kind: AssetKind::Jpeg,
                bytes,
                sha256: "0".repeat(64),
                width: w,
                height: h,
                capture,
                camera: Some("CANON EOS R6".into()),
            }],
            needs_derivation: false,
        }
    }

    // --------------------------------------------------------- F13's table

    #[test]
    fn every_failure_offers_exactly_the_actions_f13_lists() {
        assert_eq!(
            actions_for(FailureClass::NoDate),
            vec![
                ActionKind::EnterDateManually,
                ActionKind::DeriveFromBatchMedian,
                ActionKind::UseFileModificationTime,
                ActionKind::Skip
            ]
        );
        assert_eq!(
            actions_for(FailureClass::DateOutOfRangeIsolated),
            vec![
                ActionKind::RedateManually,
                ActionKind::PublishAnyway,
                ActionKind::Skip
            ]
        );
        assert_eq!(
            actions_for(FailureClass::DateOutOfRangeBatch),
            vec![
                ActionKind::BulkShift,
                ActionKind::PublishAnyway,
                ActionKind::Skip
            ]
        );
        assert_eq!(
            actions_for(FailureClass::TooManyPixels),
            vec![
                ActionKind::Resize,
                ActionKind::PublishAnyway,
                ActionKind::Skip
            ]
        );
        assert_eq!(
            actions_for(FailureClass::TooLarge),
            vec![
                ActionKind::ReencodeLower,
                ActionKind::Resize,
                ActionKind::Skip
            ]
        );
    }

    #[test]
    fn every_failure_class_has_at_least_a_way_out() {
        for failure in [
            FailureClass::NoDate,
            FailureClass::DateOutOfRangeIsolated,
            FailureClass::DateOutOfRangeBatch,
            FailureClass::TooManyPixels,
            FailureClass::TooLarge,
        ] {
            let actions = actions_for(failure);
            assert!(
                actions.contains(&ActionKind::Skip),
                "{failure:?} must always be skippable"
            );
        }
    }

    #[test]
    fn resizing_is_the_default_and_nothing_lossy_ever_is() {
        // F12's consequence: resizing is the normal path, so it is the default.
        assert_eq!(
            default_action(FailureClass::TooManyPixels),
            Some(ActionKind::Resize)
        );
        // Losing a photograph is never automatic.
        for failure in [
            FailureClass::NoDate,
            FailureClass::DateOutOfRangeIsolated,
            FailureClass::DateOutOfRangeBatch,
            FailureClass::TooManyPixels,
            FailureClass::TooLarge,
        ] {
            assert_ne!(default_action(failure), Some(ActionKind::Skip));
            assert_ne!(default_action(failure), Some(ActionKind::PublishAnyway));
        }
    }

    #[test]
    fn an_action_f13_does_not_offer_is_refused() {
        let shots = vec![shot_with("A", 6000, 4000, 1000, Some(at(2024, 5, 30)))];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                // F13 does not offer re-dating for a resolution failure.
                action: ActionKind::RedateManually,
                date: Some(now()),
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        assert!(plan_bulk(&params).is_err());
    }

    // ------------------------------------------------------------ bulk plan

    #[test]
    fn one_request_covers_every_shot_sharing_a_failure() {
        // F13: "Every action must be available as a bulk apply to all shots
        // sharing a failure." Fifty shots, one request.
        let shots: Vec<Shot> = (0..50)
            .map(|i| {
                shot_with(
                    &format!("IMG_{i:04}"),
                    6000,
                    4000,
                    1000,
                    Some(at(2024, 5, 30)),
                )
            })
            .collect();
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                action: ActionKind::Resize,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        let plan = plan_bulk(&params).unwrap().data;
        assert_eq!(plan.actions.len(), 50, "one operation, fifty shots");
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn a_resize_plan_names_the_dimensions_it_would_produce() {
        let shots = vec![shot_with("A", 6000, 4000, 1000, Some(at(2024, 5, 30)))];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                action: ActionKind::Resize,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        let plan = plan_bulk(&params).unwrap().data;
        let (w, h) = plan.actions[0].target_dimensions.unwrap();

        assert!(
            (w as u64 * h as u64) <= 10_000_000,
            "{w}×{h} must be within the ceiling"
        );
        // Aspect ratio preserved: 6000/4000 is 1.5.
        assert!(((w as f64 / h as f64) - 1.5).abs() < 0.01);
    }

    #[test]
    fn planning_writes_nothing() {
        // Build plan §7: plan never touches disk.
        let out = tempfile::tempdir().unwrap();
        let shots = vec![shot_with("A", 6000, 4000, 1000, Some(at(2024, 5, 30)))];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                action: ActionKind::Resize,
                date: None,
                output_dir: out.path().join("nested"),
            },
        };

        plan_bulk(&params).unwrap();

        assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
        assert!(!out.path().join("nested").exists());
    }

    #[test]
    fn a_bulk_shift_plans_the_date_each_frame_would_end_up_with() {
        let shots: Vec<Shot> = (0..5)
            .map(|i| {
                shot_with(
                    &format!("IMG_{i:04}"),
                    4000,
                    2500,
                    1000,
                    Some(at(2019, 3, 1) + chrono::Duration::minutes(i)),
                )
            })
            .collect();
        let validation = validate(&shots, now(), &Thresholds::default());
        assert!(validation.clock_offset.is_some(), "a clock reset");

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::DateOutOfRangeBatch,
                action: ActionKind::BulkShift,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        let plan = plan_bulk(&params).unwrap().data;
        assert_eq!(plan.actions.len(), 5);
        for action in &plan.actions {
            let date = action.new_date.expect("a shifted date");
            assert!(date.year() >= 2024, "shifted forward, got {date}");
        }
    }

    #[test]
    fn a_manual_date_action_without_a_date_is_reported_not_guessed() {
        let shots = vec![shot_with("A", 4000, 2500, 1000, None)];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::NoDate,
                action: ActionKind::EnterDateManually,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        let plan = plan_bulk(&params).unwrap().data;
        assert!(plan.actions.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("No date"));
    }

    #[test]
    fn deriving_from_the_batch_median_uses_the_cards_own_dates() {
        let shots = vec![
            shot_with("A", 4000, 2500, 1000, None),
            shot_with("B", 4000, 2500, 1000, Some(at(2024, 5, 29))),
            shot_with("C", 4000, 2500, 1000, Some(at(2024, 5, 30))),
            shot_with("D", 4000, 2500, 1000, Some(at(2024, 5, 31))),
        ];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::NoDate,
                action: ActionKind::DeriveFromBatchMedian,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        let plan = plan_bulk(&params).unwrap().data;
        assert_eq!(plan.actions.len(), 1, "only the undated frame");
        assert_eq!(plan.actions[0].new_date, Some(at(2024, 5, 30)));
    }

    #[test]
    fn a_frame_already_within_the_ceiling_plans_no_work() {
        let shots = vec![
            shot_with("BIG", 6000, 4000, 1000, Some(at(2024, 5, 30))),
            shot_with("OK", 4000, 2500, 1000, Some(at(2024, 5, 30))),
        ];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                action: ActionKind::Resize,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        let plan = plan_bulk(&params).unwrap().data;
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].stem, "BIG");
    }

    #[test]
    fn a_failure_nothing_shares_plans_nothing_rather_than_erroring() {
        let shots = vec![shot_with("A", 4000, 2500, 1000, Some(at(2024, 5, 30)))];
        let validation = validate(&shots, now(), &Thresholds::default());

        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                action: ActionKind::Resize,
                date: None,
                output_dir: PathBuf::from("/tmp/out"),
            },
        };

        assert!(plan_bulk(&params).unwrap().data.actions.is_empty());
    }

    #[test]
    fn skipping_and_publishing_anyway_are_recorded_separately() {
        let shots: Vec<Shot> = (0..3)
            .map(|i| shot_with(&format!("IMG_{i}"), 6000, 4000, 1000, Some(at(2024, 5, 30))))
            .collect();
        let validation = validate(&shots, now(), &Thresholds::default());

        for (action, expected) in [(ActionKind::Skip, 3), (ActionKind::PublishAnyway, 3)] {
            let params = RemediationParams {
                shots: &shots,
                validation: &validation,
                thresholds: Thresholds::default(),
                request: BulkRequest {
                    failure: FailureClass::TooManyPixels,
                    action,
                    date: None,
                    output_dir: PathBuf::from("/tmp/out"),
                },
            };
            let plan = plan_bulk(&params).unwrap().data;
            let summary = apply_bulk(plan, &InMemoryProgress::new()).unwrap().data;

            if action == ActionKind::Skip {
                assert_eq!(summary.skipped.len(), expected);
                assert!(summary.accepted.is_empty());
            } else {
                assert_eq!(summary.accepted.len(), expected);
                assert!(summary.skipped.is_empty());
            }
            assert!(summary.rewritten.is_empty(), "neither writes a file");
        }
    }
}
