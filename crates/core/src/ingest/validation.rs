//! Validation (F12).
//!
//! Each candidate is checked against three independent rules — date, resolution
//! and size — and each is independently testable, because a card that fails
//! validation is a card someone has to make four hundred decisions about, and
//! the rules had better be right.
//!
//! The interesting part is not the rules but the **camera clock check**. A body
//! whose clock has reset dates every frame 2019, and checking frames one at a
//! time turns one systematic error into four hundred individual failures. F12
//! requires that this is recognised across the batch and surfaced as a single
//! correction.

use crate::config::Thresholds;
use crate::ingest::grouping::Shot;
use crate::ingest::scanner::ScannedAsset;
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How tightly capture dates must cluster for the batch to count as one shoot.
///
/// F12 names 30 days for the camera-clock check. The same figure decides when a
/// frame is "far from the batch median": reusing one constant is more honest
/// than inventing a second threshold the specification does not give.
pub const BATCH_SPREAD_DAYS: i64 = 30;

/// The outcome of one check on one shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    /// Worth a human's attention, but not a reason to hold the photograph back.
    Warn,
    Fail,
    /// Cannot be decided yet — the candidate does not exist until F14 derives it.
    Pending,
}

impl CheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
            CheckStatus::Pending => "pending",
        }
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, CheckStatus::Fail)
    }
}

/// Which rule produced a finding. The key a bulk action groups by (F13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    NoDate,
    /// One frame out of range while the batch as a whole is fine.
    DateOutOfRangeIsolated,
    /// The whole batch is out of range with a tight spread — a clock reset.
    DateOutOfRangeBatch,
    TooManyPixels,
    TooLarge,
}

impl FailureClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureClass::NoDate => "no_date",
            FailureClass::DateOutOfRangeIsolated => "date_out_of_range",
            FailureClass::DateOutOfRangeBatch => "date_out_of_range_batch",
            FailureClass::TooManyPixels => "too_many_pixels",
            FailureClass::TooLarge => "too_large",
        }
    }
}

/// Which rule a check came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    Date,
    Resolution,
    Size,
}

/// One rule's verdict on one shot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub rule: Rule,
    pub status: CheckStatus,
    /// Set when the status is `Fail`; the key F13's bulk actions group by.
    pub failure: Option<FailureClass>,
    /// A sentence a person can act on.
    pub detail: String,
}

impl Check {
    fn pass(rule: Rule, detail: impl Into<String>) -> Self {
        Self {
            rule,
            status: CheckStatus::Pass,
            failure: None,
            detail: detail.into(),
        }
    }

    fn warn(rule: Rule, detail: impl Into<String>) -> Self {
        Self {
            rule,
            status: CheckStatus::Warn,
            failure: None,
            detail: detail.into(),
        }
    }

    fn fail(rule: Rule, failure: FailureClass, detail: impl Into<String>) -> Self {
        Self {
            rule,
            status: CheckStatus::Fail,
            failure: Some(failure),
            detail: detail.into(),
        }
    }

    fn pending(rule: Rule, detail: impl Into<String>) -> Self {
        Self {
            rule,
            status: CheckStatus::Pending,
            failure: None,
            detail: detail.into(),
        }
    }
}

/// A camera clock that is wrong for the whole card (F12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockOffset {
    /// The median capture date across the card.
    pub median: NaiveDateTime,
    /// Widest gap between any two capture dates, in days.
    pub spread_days: i64,
    /// How far the median is from now, in days. Positive means the past.
    pub median_age_days: i64,
    /// `now − median`, as an F1 `shift` delta.
    pub shift: String,
    /// How many shots the shift would move.
    pub affected: usize,
}

/// Everything the three rules concluded about one shot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShotValidation {
    pub stem: String,
    pub checks: Vec<Check>,
}

impl ShotValidation {
    /// The worst verdict across the three rules.
    pub fn status(&self) -> CheckStatus {
        if self.checks.iter().any(|c| c.status == CheckStatus::Fail) {
            CheckStatus::Fail
        } else if self.checks.iter().any(|c| c.status == CheckStatus::Warn) {
            CheckStatus::Warn
        } else if self.checks.iter().any(|c| c.status == CheckStatus::Pending) {
            CheckStatus::Pending
        } else {
            CheckStatus::Pass
        }
    }

    pub fn failures(&self) -> impl Iterator<Item = FailureClass> + '_ {
        self.checks.iter().filter_map(|c| c.failure)
    }

    pub fn passes(&self) -> bool {
        self.status() == CheckStatus::Pass
    }
}

/// The card's verdict (F12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardValidation {
    pub shots: Vec<ShotValidation>,
    /// Present when the whole batch shares a clock error.
    pub clock_offset: Option<ClockOffset>,
}

impl CardValidation {
    /// Shot indices grouped by the failure they share.
    ///
    /// **This is what makes bulk remediation possible** (F13): every action must
    /// be available across all shots sharing a failure, and this is the grouping
    /// that "sharing a failure" means.
    pub fn by_failure(&self) -> BTreeMap<FailureClass, Vec<usize>> {
        let mut grouped: BTreeMap<FailureClass, Vec<usize>> = BTreeMap::new();
        for (index, shot) in self.shots.iter().enumerate() {
            for failure in shot.failures() {
                grouped.entry(failure).or_default().push(index);
            }
        }
        grouped
    }

    pub fn failing(&self) -> usize {
        self.shots.iter().filter(|s| s.status().is_fail()).count()
    }

    pub fn passing(&self) -> usize {
        self.shots.iter().filter(|s| s.passes()).count()
    }
}

// ---------------------------------------------------------------------------
// The three rules, each usable on its own
// ---------------------------------------------------------------------------

/// The date rule (F12).
///
/// `batch` is the card's clock verdict, which decides whether an out-of-range
/// date is one frame's problem or the whole card's — the same date is a
/// different finding depending on the company it keeps.
pub fn check_date(
    asset: &ScannedAsset,
    now: NaiveDateTime,
    thresholds: &Thresholds,
    median: Option<NaiveDateTime>,
    batch: Option<&ClockOffset>,
) -> Check {
    let Some(capture) = asset.capture else {
        return Check::fail(Rule::Date, FailureClass::NoDate, "No capture date");
    };

    let age_days = (now - capture).num_days().abs();

    if age_days > thresholds.max_age_days {
        // F12: a whole batch out of range with a tight spread is a clock reset,
        // not four hundred old photographs.
        let (class, detail) = if batch.is_some() {
            (
                FailureClass::DateOutOfRangeBatch,
                format!("{capture} — the whole card shares this offset"),
            )
        } else {
            (
                FailureClass::DateOutOfRangeIsolated,
                format!(
                    "{capture} is {age_days} days old, over the {} day limit",
                    thresholds.max_age_days
                ),
            )
        };
        return Check::fail(Rule::Date, class, detail);
    }

    // F12: "Within range but far from the batch median → WARN, not FAIL. Frames
    // left over from an earlier shoot on the same card are legitimate."
    if let Some(median) = median {
        let from_median = (capture - median).num_days().abs();
        if from_median > BATCH_SPREAD_DAYS {
            return Check::warn(
                Rule::Date,
                format!("{capture} is {from_median} days from the rest of the card"),
            );
        }
    }

    Check::pass(Rule::Date, format!("{capture}"))
}

/// The resolution rule (F12): `width × height ≤ max_megapixels × 10⁶`.
pub fn check_resolution(asset: &ScannedAsset, thresholds: &Thresholds) -> Check {
    // Zero means there is no resolution ceiling, so the rule has nothing to
    // decide. It passes with the measurement rather than a verdict: the number
    // is still worth showing on the review screen, and calling it a "pass"
    // against a limit that does not exist would be a small lie repeated on
    // every row.
    if thresholds.max_megapixels == 0 {
        if asset.dimensions_unknown() {
            return Check::pass(Rule::Resolution, "No ceiling set");
        }
        return Check::pass(
            Rule::Resolution,
            format!(
                "{}×{}, {:.1} MP — no ceiling set",
                asset.width,
                asset.height,
                asset.megapixels()
            ),
        );
    }

    // F11 forbids decoding to find out, so "the card does not say" is its own
    // outcome. Not a pass — there is no evidence it is under the ceiling — and
    // not a fail, because there is none that it is over.
    if asset.dimensions_unknown() {
        return Check::warn(Rule::Resolution, "No dimensions in metadata");
    }

    let pixels = asset.width as u64 * asset.height as u64;
    let ceiling = thresholds.max_megapixels as u64 * 1_000_000;

    if pixels <= ceiling {
        Check::pass(
            Rule::Resolution,
            format!(
                "{}×{}, {:.1} MP",
                asset.width,
                asset.height,
                asset.megapixels()
            ),
        )
    } else {
        Check::fail(
            Rule::Resolution,
            FailureClass::TooManyPixels,
            format!(
                "{:.1} MP, over the {} MP ceiling",
                asset.megapixels(),
                thresholds.max_megapixels
            ),
        )
    }
}

/// The size rule (F12): the published file must be within `max_output_bytes`.
pub fn check_size(asset: &ScannedAsset, thresholds: &Thresholds) -> Check {
    if asset.bytes <= thresholds.max_output_bytes {
        Check::pass(Rule::Size, format!("{:.1} MB", megabytes(asset.bytes)))
    } else {
        Check::fail(
            Rule::Size,
            FailureClass::TooLarge,
            format!(
                "{:.1} MB, over the {:.0} MB limit",
                megabytes(asset.bytes),
                megabytes(thresholds.max_output_bytes)
            ),
        )
    }
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

// ---------------------------------------------------------------------------
// The camera clock check
// ---------------------------------------------------------------------------

/// The median capture date across a set of shots.
pub fn median_capture(captures: &[NaiveDateTime]) -> Option<NaiveDateTime> {
    if captures.is_empty() {
        return None;
    }
    let mut sorted = captures.to_vec();
    sorted.sort();
    Some(sorted[sorted.len() / 2])
}

/// Detect a camera clock that is wrong for the whole card (F12).
///
/// > If the median is out of range but the spread is tight (under 30 days), the
/// > camera clock is offset rather than the photographs being old. Surface a
/// > **single bulk correction of `now − median`**.
///
/// Spread is the widest gap between any two capture dates, which is the plain
/// reading of the word. That makes it sensitive: one frame left over from an
/// earlier shoot widens the spread and suppresses the suggestion, even when the
/// other 399 plainly share one offset. The alternative — a robust measure like
/// the median absolute deviation — is not what F12 says, so it is not what this
/// does. See the phase report.
pub fn detect_clock_offset(
    captures: &[NaiveDateTime],
    now: NaiveDateTime,
    thresholds: &Thresholds,
) -> Option<ClockOffset> {
    let median = median_capture(captures)?;

    let median_age_days = (now - median).num_days().abs();
    if median_age_days <= thresholds.max_age_days {
        // The batch as a whole is fine; any failure is an individual frame's.
        return None;
    }

    let earliest = captures.iter().min()?;
    let latest = captures.iter().max()?;
    let spread_days = (*latest - *earliest).num_days().abs();

    if spread_days >= BATCH_SPREAD_DAYS {
        // Spread out over months: these are old photographs, not a wrong clock.
        return None;
    }

    Some(ClockOffset {
        median,
        spread_days,
        median_age_days,
        shift: format_shift(now - median),
        affected: captures.len(),
    })
}

/// Render a duration as an F1 `shift` delta: `+Y:M:D h:m:s`.
///
/// Whole days are carried as days rather than converted to months, because a
/// month is not a fixed length and `exiftool` would resolve it against each
/// file's own date. Days are unambiguous.
fn format_shift(delta: Duration) -> String {
    let sign = if delta < Duration::zero() { '-' } else { '+' };
    let total = delta.num_seconds().abs();

    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;
    let seconds = total % 60;

    format!("{sign}0:0:{days} {hours}:{minutes}:{seconds}")
}

// ---------------------------------------------------------------------------
// Validating a whole card
// ---------------------------------------------------------------------------

/// Run all three rules over every shot on a card (F12).
pub fn validate(shots: &[Shot], now: NaiveDateTime, thresholds: &Thresholds) -> CardValidation {
    let captures: Vec<NaiveDateTime> = shots.iter().filter_map(|s| s.capture()).collect();
    let median = median_capture(&captures);
    let clock_offset = detect_clock_offset(&captures, now, thresholds);

    let validated = shots
        .iter()
        .map(|shot| {
            let candidate = shot.candidate();
            let mut checks = vec![check_date(
                candidate,
                now,
                thresholds,
                median,
                clock_offset.as_ref(),
            )];

            // A RAW-only shot's candidate does not exist yet: F14 derives it,
            // and its size is a property of that derivation, not of the RAW.
            // The RAW's dimensions are real, so resolution is still checked.
            checks.push(check_resolution(candidate, thresholds));
            checks.push(if shot.needs_derivation {
                Check::pending(Rule::Size, "Awaiting the JPEG F14 will derive")
            } else {
                check_size(candidate, thresholds)
            });

            ShotValidation {
                stem: shot.stem.clone(),
                checks,
            }
        })
        .collect();

    CardValidation {
        shots: validated,
        clock_offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::scanner::AssetKind;
    use chrono::NaiveDate;
    use std::path::PathBuf;

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

    fn asset(width: u32, height: u32, bytes: u64, capture: Option<NaiveDateTime>) -> ScannedAsset {
        ScannedAsset {
            path: PathBuf::from("IMG_0001.JPG"),
            rel_path: "IMG_0001.JPG".into(),
            kind: AssetKind::Jpeg,
            bytes,
            sha256: "0".repeat(64),
            width,
            height,
            capture,
            camera: Some("CANON EOS R6".into()),
        }
    }

    fn shot(stem: &str, asset: ScannedAsset) -> Shot {
        Shot {
            stem: stem.into(),
            assets: vec![asset],
            needs_derivation: false,
        }
    }

    // ------------------------------------------------------------ resolution

    /// Thresholds with §F12's 10 MP ceiling set.
    ///
    /// The default is now zero — no ceiling — so a test of the resolution rule
    /// has to say which ceiling it is testing. The rule itself is unchanged;
    /// these assertions are the same ones, against a stated limit rather than
    /// an inherited one.
    fn with_ceiling(max_megapixels: u32) -> Thresholds {
        Thresholds {
            max_megapixels,
            ..Thresholds::default()
        }
    }

    #[test]
    fn ten_megapixels_exactly_passes_and_a_fraction_over_fails() {
        // The acceptance boundary. 4000×2500 is 10,000,000 pixels exactly.
        let t = with_ceiling(10);

        assert_eq!(
            check_resolution(&asset(4000, 2500, 1000, None), &t).status,
            CheckStatus::Pass,
            "the rule is ≤, so exactly at the ceiling passes"
        );
        assert_eq!(
            check_resolution(&asset(4040, 2500, 1000, None), &t).status,
            CheckStatus::Fail,
            "10.1 MP is over"
        );
    }

    #[test]
    fn a_frame_over_the_ceiling_reports_which_class_it_failed() {
        let check = check_resolution(&asset(6000, 4000, 1000, None), &with_ceiling(10));
        assert_eq!(check.failure, Some(FailureClass::TooManyPixels));
        assert!(check.detail.contains("24.0 MP"), "{}", check.detail);
    }

    #[test]
    fn unknown_dimensions_warn_rather_than_fail() {
        // F11 forbids decoding to find out, so this is genuinely unknown. It is
        // not evidence of being over the ceiling.
        let check = check_resolution(&asset(0, 0, 1000, None), &with_ceiling(10));
        assert_eq!(check.status, CheckStatus::Warn);
        assert_eq!(check.failure, None);
    }

    #[test]
    fn no_ceiling_passes_any_resolution_and_says_so() {
        // Zero means the rule has nothing to decide. It still reports the
        // measurement, because the number is worth seeing on the review screen.
        let t = with_ceiling(0);
        assert_eq!(t.max_megapixels, 0, "this is the default");

        let check = check_resolution(&asset(11648, 8736, 1000, None), &t);
        assert_eq!(
            check.status,
            CheckStatus::Pass,
            "102 MP with no ceiling set"
        );
        assert_eq!(check.failure, None);
        assert!(check.detail.contains("no ceiling"), "{}", check.detail);
        assert!(
            check.detail.contains("MP"),
            "the size is still shown: {}",
            check.detail
        );

        // And unknown dimensions are no longer a warning: there is nothing they
        // could be over.
        let unknown = check_resolution(&asset(0, 0, 1000, None), &t);
        assert_eq!(unknown.status, CheckStatus::Pass);
    }

    // ------------------------------------------------------------------ size

    #[test]
    fn the_size_boundary_is_inclusive() {
        let t = Thresholds::default();
        let cap = t.max_output_bytes;

        assert_eq!(
            check_size(&asset(100, 100, cap, None), &t).status,
            CheckStatus::Pass
        );
        assert_eq!(
            check_size(&asset(100, 100, cap + 1, None), &t).status,
            CheckStatus::Fail
        );
    }

    #[test]
    fn an_oversized_file_reports_too_large() {
        let t = Thresholds::default();
        let check = check_size(&asset(100, 100, 20 * 1024 * 1024, None), &t);
        assert_eq!(check.failure, Some(FailureClass::TooLarge));
    }

    // ------------------------------------------------------------------ date

    #[test]
    fn a_missing_capture_date_fails_as_no_date() {
        let check = check_date(
            &asset(100, 100, 1, None),
            now(),
            &Thresholds::default(),
            None,
            None,
        );
        assert_eq!(check.failure, Some(FailureClass::NoDate));
    }

    #[test]
    fn a_recent_date_passes() {
        let recent = Some(at(2024, 5, 20));
        let check = check_date(
            &asset(100, 100, 1, recent),
            now(),
            &Thresholds::default(),
            recent,
            None,
        );
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn an_isolated_old_frame_fails_as_isolated_not_as_a_batch() {
        let old = Some(at(2019, 1, 1));
        let check = check_date(
            &asset(100, 100, 1, old),
            now(),
            &Thresholds::default(),
            Some(at(2024, 5, 20)),
            None,
        );
        assert_eq!(check.failure, Some(FailureClass::DateOutOfRangeIsolated));
    }

    #[test]
    fn an_in_range_frame_far_from_the_median_warns_rather_than_failing() {
        // F12: "Frames left over from an earlier shoot on the same card are
        // legitimate." 60 days old, inside the 90-day limit, but nowhere near
        // the rest of the card.
        let stray = Some(at(2024, 4, 1));
        let check = check_date(
            &asset(100, 100, 1, stray),
            now(),
            &Thresholds::default(),
            Some(at(2024, 5, 30)),
            None,
        );
        assert_eq!(check.status, CheckStatus::Warn);
        assert_eq!(check.failure, None, "a warning is not a failure class");
    }

    // ---------------------------------------------------------- clock offset

    #[test]
    fn a_card_dated_2019_with_a_tight_spread_is_a_clock_offset() {
        let captures: Vec<NaiveDateTime> = (0..400)
            .map(|i| at(2019, 3, 1) + Duration::minutes(i))
            .collect();

        let offset = detect_clock_offset(&captures, now(), &Thresholds::default())
            .expect("a whole card in 2019 with minutes of spread is a clock reset");

        assert_eq!(offset.affected, 400);
        assert_eq!(offset.spread_days, 0);
        assert!(offset.shift.starts_with('+'), "{}", offset.shift);
    }

    #[test]
    fn photographs_genuinely_spread_over_months_are_not_a_clock_offset() {
        // Old *and* spread out: these really are old photographs.
        let captures = vec![at(2019, 1, 1), at(2019, 6, 1), at(2019, 12, 1)];

        assert_eq!(
            detect_clock_offset(&captures, now(), &Thresholds::default()),
            None
        );
    }

    #[test]
    fn a_card_shot_this_week_has_no_clock_offset() {
        let captures = vec![at(2024, 5, 28), at(2024, 5, 29), at(2024, 5, 30)];

        assert_eq!(
            detect_clock_offset(&captures, now(), &Thresholds::default()),
            None
        );
    }

    #[test]
    fn an_empty_card_has_no_median_and_no_offset() {
        assert_eq!(median_capture(&[]), None);
        assert_eq!(
            detect_clock_offset(&[], now(), &Thresholds::default()),
            None
        );
    }

    #[test]
    fn the_shift_is_expressed_as_days_not_months() {
        // exiftool resolves a month against each file's own date, so a month is
        // not a fixed length. Days are unambiguous.
        let captures = vec![at(2024, 1, 1)];
        let offset = detect_clock_offset(&captures, now(), &Thresholds::default()).unwrap();

        assert_eq!(offset.shift, "+0:0:152 0:0:0");
    }

    #[test]
    fn a_clock_running_ahead_produces_a_negative_shift() {
        let captures = vec![at(2025, 1, 1)];
        let offset = detect_clock_offset(&captures, now(), &Thresholds::default()).unwrap();

        assert!(offset.shift.starts_with('-'), "{}", offset.shift);
    }

    // -------------------------------------------------------- the whole card

    #[test]
    fn a_clock_reset_card_produces_one_suggestion_not_four_hundred_failures() {
        // The acceptance criterion, stated as the plan states it.
        let shots: Vec<Shot> = (0..400)
            .map(|i| {
                shot(
                    &format!("IMG_{i:04}"),
                    asset(
                        4000,
                        2500,
                        1_000_000,
                        Some(at(2019, 3, 1) + Duration::minutes(i)),
                    ),
                )
            })
            .collect();

        let result = validate(&shots, now(), &Thresholds::default());

        let offset = result.clock_offset.as_ref().expect("one bulk correction");
        assert_eq!(offset.affected, 400);

        // Every frame is grouped under one failure class, so one action fixes
        // the card.
        let grouped = result.by_failure();
        assert_eq!(grouped.len(), 1, "one class, not four hundred decisions");
        assert_eq!(grouped[&FailureClass::DateOutOfRangeBatch].len(), 400);
    }

    #[test]
    fn shots_sharing_a_failure_are_grouped_for_one_bulk_action() {
        // F13: "Every action must be available as a bulk apply to all shots
        // sharing a failure." This is what sharing means.
        let shots = vec![
            shot("A", asset(6000, 4000, 1_000, Some(at(2024, 5, 30)))),
            shot("B", asset(6000, 4000, 1_000, Some(at(2024, 5, 30)))),
            shot("C", asset(4000, 2500, 1_000, None)),
        ];

        let grouped = validate(&shots, now(), &with_ceiling(10)).by_failure();

        assert_eq!(grouped[&FailureClass::TooManyPixels], vec![0, 1]);
        assert_eq!(grouped[&FailureClass::NoDate], vec![2]);
    }

    #[test]
    fn a_raw_only_shot_defers_its_size_check_to_f14() {
        let mut raw = shot(
            "IMG_0001",
            asset(6000, 4000, 30_000_000, Some(at(2024, 5, 30))),
        );
        raw.needs_derivation = true;
        raw.assets[0].kind = AssetKind::Raw;

        let result = validate(&[raw], now(), &with_ceiling(10));
        let checks = &result.shots[0].checks;

        let size = checks.iter().find(|c| c.rule == Rule::Size).unwrap();
        assert_eq!(
            size.status,
            CheckStatus::Pending,
            "a 30 MB RAW says nothing about the JPEG F14 will produce"
        );

        // The RAW's own dimensions are real, so resolution is still decided.
        let resolution = checks.iter().find(|c| c.rule == Rule::Resolution).unwrap();
        assert_eq!(resolution.status, CheckStatus::Fail);
    }

    #[test]
    fn a_clean_card_passes_every_rule() {
        let shots = vec![
            shot("A", asset(4000, 2500, 1_000_000, Some(at(2024, 5, 30)))),
            shot("B", asset(3000, 2000, 900_000, Some(at(2024, 5, 31)))),
        ];

        let result = validate(&shots, now(), &Thresholds::default());

        assert_eq!(result.passing(), 2);
        assert_eq!(result.failing(), 0);
        assert!(result.clock_offset.is_none());
        assert!(result.by_failure().is_empty());
    }

    #[test]
    fn the_worst_check_decides_a_shots_status() {
        let shots = vec![shot("A", asset(6000, 4000, 1_000, Some(at(2024, 5, 30))))];

        let result = validate(&shots, now(), &with_ceiling(10));
        assert_eq!(result.shots[0].status(), CheckStatus::Fail);
        assert!(!result.shots[0].passes());
    }
}
