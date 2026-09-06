//! Joining a moment to a place.
//!
//! Pure over a sorted slice of fixes: no database, no file, no clock. That is
//! deliberate — the arithmetic here is the whole feature, and it should be
//! provable without any of the machinery that feeds it.

use super::TrackPoint;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// How a position was arrived at.
///
/// Travels with every match, because "we have a fix for that second" and "we
/// used one from three hours earlier" are different claims and a table that
/// showed them the same way would be lying by omission.
///
/// **Every position this tool writes was recorded by the phone.** Nothing is
/// computed, averaged or drawn between two points — see [`Mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    /// A fix recorded at that very second.
    Exact,
    /// The closest real fix, used as it stands.
    Nearest,
    /// The last fix recorded *before* the photograph, carried forward.
    CarriedForward,
}

/// Which recorded fix to use when none was recorded at that exact second.
///
/// **There is no mode that computes a position, and that is deliberate.** The
/// tracker this was built for reports when — and only when — its owner moves,
/// so the interval between two fixes is not missing data to be filled in: it is
/// a person who had not moved yet. A line drawn across it passes through
/// streets nobody walked, and the resulting coordinate looks exactly as
/// trustworthy as a real one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// The last fix recorded *before* the photograph, carried forward.
    ///
    /// The right reading of a tracker that reports on movement: between two
    /// fixes, its owner was still at the first of them.
    CarriedForward,
    /// The closest fix in time, from whichever side.
    ///
    /// For a tracker that samples continuously — where the interval really is
    /// just the sampling rate, and the nearer fix really is the better one.
    /// Against a movement-triggered track it answers with where somebody went
    /// next whenever that fix happens to be nearer in time, which is why it is
    /// not the default.
    Nearest,
}

/// The two ceilings, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub mode: Mode,
    /// How old a fix may be and still be used.
    ///
    /// **This is not about the fix going stale.** A tracker that reports on
    /// movement is *right* however long it has been silent — silence is the
    /// evidence that nobody moved, so a six-hour-old fix is as true as a
    /// six-second-old one.
    ///
    /// What it guards is the other case entirely: a photograph from a day this
    /// track does not cover. Without a limit, a frame shot in March against a
    /// September track takes the last fix of September — confidently, silently,
    /// and wrong by a continent. The limit is the line past which the honest
    /// answer is "this track does not know".
    ///
    /// **Zero means no limit**, the convention `max_megapixels` already uses.
    pub max_edge_seconds: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            mode: Mode::CarriedForward,
            // Twelve hours: longer than any silence within a day the tracker
            // was running — a night at home is ten — and short enough that a
            // photograph from a trip this track knows nothing about is refused
            // rather than given the last fix of a different journey.
            //
            // Half an hour, which this was, is the wrong shape of number: it
            // refuses the café afternoon that the whole carry-forward rule
            // exists to answer.
            max_edge_seconds: 12 * 3600,
        }
    }
}

/// A position, and how much to trust it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub point: TrackPoint,
    pub method: Method,
    /// Seconds between the photograph and the nearest *recorded* fix.
    ///
    /// Always a real number here, because the position always came from a real
    /// fix: it is how old the observation is, and the whole measure of how much
    /// to trust the row.
    pub gap_seconds: i64,
}

/// Find where somebody was at `at`, or say why that cannot be answered.
///
/// A fix recorded at that second if there is one; otherwise the fix the mode
/// asks for, if it is recent enough. **The answer is always a position the
/// phone recorded** — this function has no arithmetic in it and cannot invent a
/// coordinate.
pub fn match_at(points: &[TrackPoint], at: i64, limits: &Limits) -> Result<Match, String> {
    if points.is_empty() {
        // What to do about it, not just what is wrong. Reading a track is a dry
        // run — the fixes only reach the timeline once the import is committed
        // — so this is the state somebody lands in by previewing a track and
        // then going straight to matching.
        return Err(
            "the track library holds no fixes for this day — read a .gpx and then press              Import into the library"
                .into(),
        );
    }

    let index = points.partition_point(|p| p.at < at);

    if let Some(point) = points.get(index).filter(|p| p.at == at) {
        return Ok(Match {
            point: *point,
            method: Method::Exact,
            gap_seconds: 0,
        });
    }

    let before = index.checked_sub(1).map(|i| points[i]);
    let after = points.get(index).copied();

    // Carrying forward prefers the fix *before*, whatever is on the other side:
    // that is the whole difference between "where you were" and "where you went
    // next". With nothing before it — a photograph taken before the track
    // begins — the first fix is the only evidence there is, and it is offered
    // under the same tolerance.
    let (chosen, method) = match (limits.mode, before, after) {
        (Mode::CarriedForward, Some(before), _) => (before, Method::CarriedForward),
        _ => (
            [before, after]
                .into_iter()
                .flatten()
                .min_by_key(|p| (p.at - at).abs())
                .expect("a non-empty slice has a fix on at least one side"),
            Method::Nearest,
        ),
    };
    let gap = (chosen.at - at).abs();

    if within_edge(gap, limits) {
        return Ok(Match {
            point: chosen,
            method,
            gap_seconds: gap,
        });
    }

    // The refusal names the number that caused it, so a limit can be raised
    // deliberately rather than by trial and error.
    let raise = format!(
        " — this track may not cover it. Raise \"stop trusting a fix after\" above {}, or set it \
         to 0 for no limit",
        duration(limits.max_edge_seconds)
    );

    Err(match (before, after) {
        (Some(b), Some(a)) => format!(
            "the phone recorded nothing for {} around this photograph, and the fix it \
             would take is {} old{raise}",
            duration(a.at - b.at),
            duration(gap)
        ),
        (Some(_), None) => format!("{} after the last fix in the library{raise}", duration(gap)),
        (None, Some(_)) => format!(
            "{} before the first fix in the library{raise}",
            duration(gap)
        ),
        (None, None) => unreachable!("a non-empty slice has a fix on at least one side"),
    })
}

/// Whether a fix this far from the photograph may be used.
///
/// Zero is no limit, matching `max_megapixels` (F12), where the same question —
/// "what if I do not want a ceiling at all?" — has the same answer.
fn within_edge(gap: i64, limits: &Limits) -> bool {
    limits.max_edge_seconds == 0 || gap <= limits.max_edge_seconds
}

/// A span of seconds, as a person would say it.
fn duration(seconds: i64) -> String {
    let seconds = seconds.abs();
    match seconds {
        0..=119 => format!("{seconds} s"),
        120..=7199 => format!("{} min", seconds / 60),
        _ => {
            let hours = seconds / 3600;
            let minutes = (seconds % 3600) / 60;
            if minutes == 0 {
                format!("{hours} h")
            } else {
                format!("{hours} h {minutes} min")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Working out the offset
// ---------------------------------------------------------------------------

/// Every UTC offset a clock is actually set to, in minutes east.
///
/// Candidates are real zones rather than a sweep in quarter-hour steps, and
/// that is what makes the answer usable. A track sampled every five minutes
/// cannot tell `+01:45` from `+02:00` — the scores differ by less than its own
/// spacing — so a sweep will sometimes hand back a quarter-hour that no clock
/// on earth is set to. A camera's clock is set to a *zone*, and there are only
/// thirty-eight of them; five are on a quarter or a half hour, and dropping
/// those would move every photograph taken in India, Nepal, Iran, Newfoundland
/// or central Australia.
const ZONE_OFFSETS: [i32; 38] = [
    -720, -660, -600, -570, -540, -480, -420, -360, -300, -240, -210, -180, -120, -60, 0, 60, 120,
    180, 210, 240, 270, 300, 330, 345, 360, 390, 420, 480, 525, 540, 570, 600, 630, 660, 720, 765,
    780, 840,
];

/// Below this many photographs an estimate is arithmetic, not evidence: with
/// one frame, some offset puts it next to a fix no matter where it was taken.
const MINIMUM_SAMPLE: usize = 3;

/// The smallest spacing that counts as a track's resolution, in seconds.
///
/// Guards against a very dense track claiming to separate offsets to the
/// second, which would report every answer as uncertain.
const MINIMUM_RESOLUTION_SECONDS: i64 = 60;

/// A suggestion whose own score is worse than this is not a suggestion: no
/// photograph is near any fix, and some offset still has to come first. Half an
/// hour, matching the default edge tolerance in [`Limits`].
const CONFIDENT_WITHIN_SECONDS: i64 = 30 * 60;

/// An offset worked out from the photographs themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffsetSuggestion {
    /// The best-scoring zone offset, in minutes east.
    pub minutes: i32,
    /// The median distance from a photograph to the nearest fix at this offset.
    pub median_gap_seconds: i64,
    /// The narrowest and widest offsets the evidence cannot separate from the
    /// winner. Where these differ, the honest answer is a range and the screen
    /// says so.
    pub plausible_low_minutes: i32,
    pub plausible_high_minutes: i32,
    /// One candidate survives, and the photographs really are on this track.
    ///
    /// A weak win means the track does not cover these photographs, or covers
    /// them too coarsely to tell two zones apart — and saying that is worth
    /// more than a confident wrong hour.
    pub confident: bool,
    pub sample: usize,
}

/// The offset that best explains where these photographs fall on the track.
///
/// Scored on the **median** distance to the nearest fix rather than the mean:
/// one photograph taken indoors hours from any fix would drag a mean far enough
/// to change the winner, and it is exactly the photograph the answer should not
/// depend on.
///
/// Two offsets whose scores differ by less than the track's own spacing are not
/// distinguishable *by that track*, so both stay in the plausible range. This is
/// the difference between an estimate and a guess dressed as one.
pub fn estimate_offset(
    points: &[TrackPoint],
    captures: &[NaiveDateTime],
) -> Option<OffsetSuggestion> {
    if points.is_empty() || captures.len() < MINIMUM_SAMPLE {
        return None;
    }

    let local: Vec<i64> = captures.iter().map(|c| c.and_utc().timestamp()).collect();

    let mut scored: Vec<(i64, i32)> = ZONE_OFFSETS
        .iter()
        .map(|minutes| {
            let mut gaps: Vec<i64> = local
                .iter()
                .map(|t| nearest_gap(points, t - i64::from(*minutes) * 60))
                .collect();
            gaps.sort_unstable();
            (gaps[gaps.len() / 2], *minutes)
        })
        .collect();

    // Ties go to the offset nearer Greenwich, only so the answer is the same
    // between two runs; a tie means the track cannot tell them apart, which the
    // plausible range below is what actually reports.
    scored.sort_by_key(|(median, minutes)| (*median, minutes.abs()));
    let (best_median, best_minutes) = scored[0];

    let resolution = resolution_seconds(points);
    let plausible: Vec<i32> = scored
        .iter()
        .filter(|(median, _)| *median <= best_median.saturating_add(resolution))
        .map(|(_, minutes)| *minutes)
        .collect();

    Some(OffsetSuggestion {
        minutes: best_minutes,
        median_gap_seconds: best_median,
        plausible_low_minutes: *plausible.iter().min().unwrap_or(&best_minutes),
        plausible_high_minutes: *plausible.iter().max().unwrap_or(&best_minutes),
        confident: plausible.len() == 1 && best_median <= CONFIDENT_WITHIN_SECONDS,
        sample: captures.len(),
    })
}

/// How finely this track can separate two moments: the median spacing between
/// consecutive fixes.
fn resolution_seconds(points: &[TrackPoint]) -> i64 {
    let mut spacings: Vec<i64> = points.windows(2).map(|w| w[1].at - w[0].at).collect();
    if spacings.is_empty() {
        return MINIMUM_RESOLUTION_SECONDS;
    }
    spacings.sort_unstable();
    spacings[spacings.len() / 2].max(MINIMUM_RESOLUTION_SECONDS)
}

/// Seconds from `at` to the nearest recorded fix.
fn nearest_gap(points: &[TrackPoint], at: i64) -> i64 {
    let index = points.partition_point(|p| p.at < at);
    let before = index.checked_sub(1).map(|i| (points[i].at - at).abs());
    let after = points.get(index).map(|p| (p.at - at).abs());
    match (before, after) {
        (Some(b), Some(a)) => b.min(a),
        (Some(b), None) => b,
        (None, Some(a)) => a,
        (None, None) => i64::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fix `minutes` after the hour.
    fn fix(minutes: i64, lat: f64, lon: f64) -> TrackPoint {
        TrackPoint {
            at: minutes * 60,
            lat,
            lon,
            ele: Some(100.0),
        }
    }

    /// Three fixes five minutes apart: the phone reporting while its owner
    /// walked.
    fn track() -> Vec<TrackPoint> {
        vec![
            fix(0, 52.000, 13.000),
            fix(5, 52.005, 13.005),
            fix(10, 52.010, 13.010),
        ]
    }

    /// A morning fix, three hours of silence, and an afternoon fix elsewhere.
    /// The silence is somebody sitting still, which a movement-triggered
    /// tracker records as nothing at all.
    fn cafe() -> Vec<TrackPoint> {
        vec![fix(0, 52.500, 13.300), fix(180, 52.600, 13.400)]
    }

    fn nearest_mode() -> Limits {
        Limits {
            mode: Mode::Nearest,
            ..Limits::default()
        }
    }

    /// A short ceiling, for the tests that are about the ceiling itself.
    fn oldest(minutes: i64) -> Limits {
        Limits {
            max_edge_seconds: minutes * 60,
            ..Limits::default()
        }
    }

    /// The default with no ceiling on how old a fix may be.
    fn without_limit() -> Limits {
        Limits {
            max_edge_seconds: 0,
            ..Limits::default()
        }
    }

    // ---------------------------------------------------------------------
    // Every answer is a position the phone recorded
    // ---------------------------------------------------------------------

    #[test]
    fn every_answer_is_a_fix_the_phone_recorded() {
        // The guarantee the whole design now rests on, asserted over every
        // second of a track rather than at a few chosen moments: no coordinate
        // this returns was computed, averaged, or drawn between two points.
        let points = track();
        let recorded: Vec<(f64, f64)> = points.iter().map(|p| (p.lat, p.lon)).collect();

        for second in -600..1200 {
            let Ok(m) = match_at(&points, second, &without_limit()) else {
                continue;
            };
            assert!(
                recorded.contains(&(m.point.lat, m.point.lon)),
                "at {second}s it answered with {}, {} — which the phone never recorded",
                m.point.lat,
                m.point.lon
            );
        }
    }

    #[test]
    fn a_photograph_taken_on_a_fix_takes_that_fix() {
        let m = match_at(&track(), 5 * 60, &Limits::default()).unwrap();
        assert_eq!(m.method, Method::Exact);
        assert_eq!(m.gap_seconds, 0);
        assert_eq!(m.point.lat, 52.005);
    }

    #[test]
    fn a_photograph_between_two_fixes_takes_the_one_before_it() {
        // Halfway between minute 0 and minute 5. Halfway is not an answer: the
        // tracker reports when its owner moves, so at that moment they had not
        // moved yet.
        let m = match_at(&track(), 150, &Limits::default()).unwrap();
        assert_eq!(m.method, Method::CarriedForward);
        assert_eq!(m.point.lat, 52.000);
        assert_eq!(m.point.lon, 13.000);
        assert_eq!(m.gap_seconds, 150);
    }

    #[test]
    fn the_altitude_is_the_recorded_one_too() {
        let m = match_at(&track(), 150, &Limits::default()).unwrap();
        assert_eq!(m.point.ele, Some(100.0));
    }

    #[test]
    fn a_fix_three_hours_old_beats_one_twenty_nine_minutes_away() {
        // The case as the user put it. With a tracker that reports only on
        // movement, the old fix is where they still were and the next one is
        // where they went afterwards. Nearness in time says nothing about
        // nearness in space.
        let points = vec![fix(0, 52.500, 13.300), fix(209, 52.600, 13.400)];
        let three_hours_in = 180 * 60;

        let m = match_at(&points, three_hours_in, &without_limit()).unwrap();
        assert_eq!(m.point.lat, 52.500, "should still be where it last was");
        assert_eq!(m.method, Method::CarriedForward);
        assert_eq!(m.gap_seconds, 180 * 60);

        // And the contrast that keeps `Nearest` a mode of its own.
        let nearest = match_at(
            &points,
            three_hours_in,
            &Limits {
                max_edge_seconds: 0,
                ..nearest_mode()
            },
        )
        .unwrap();
        assert_eq!(nearest.point.lat, 52.600, "nearest takes the 29-minute one");
    }

    #[test]
    fn the_answer_is_never_a_fix_from_later() {
        // Over every moment of a track with three hours of silence in it.
        let points = cafe();
        for minute in 1..180 {
            let m = match_at(&points, minute * 60, &without_limit()).unwrap();
            assert!(
                m.point.lat <= 52.500 + 1e-9,
                "at minute {minute} it reached forward to {}",
                m.point.lat
            );
        }
    }

    #[test]
    fn a_photograph_before_the_track_begins_has_nothing_to_carry() {
        // The first fix is the only evidence there is, and it is offered as
        // *nearest* rather than dressed up as a carried position.
        let m = match_at(&track(), -600, &Limits::default()).unwrap();
        assert_eq!(m.method, Method::Nearest);
        assert_eq!(m.point.lat, 52.000);
        assert_eq!(m.gap_seconds, 600);
    }

    // ---------------------------------------------------------------------
    // How old is too old
    // ---------------------------------------------------------------------

    #[test]
    fn a_fix_older_than_the_ceiling_is_refused() {
        let error = match_at(&cafe(), 120 * 60, &oldest(30)).unwrap_err();
        assert!(error.contains("recorded nothing for"), "got {error}");
    }

    #[test]
    fn the_default_ceiling_accepts_a_whole_afternoon_of_sitting_still() {
        // The case the carry-forward rule exists for. A ceiling that refuses
        // this is the wrong shape of number, whatever it is set to.
        let m = match_at(&cafe(), 120 * 60, &Limits::default()).unwrap();
        assert_eq!(m.method, Method::CarriedForward);
        assert_eq!(m.point.lat, 52.500);
    }

    #[test]
    fn the_default_ceiling_refuses_a_photograph_from_another_trip() {
        // The case it exists for. A frame from a fortnight away must not take
        // the last fix of this track and look exactly like a real answer.
        let error = match_at(&cafe(), 14 * 86_400, &Limits::default()).unwrap_err();
        assert!(error.contains("may not cover it"), "got {error}");
    }

    #[test]
    fn no_limit_accepts_a_fix_from_the_far_side_of_a_silent_night() {
        // Ten hours after the last fix of the evening.
        let m = match_at(&cafe(), 780 * 60, &without_limit()).unwrap();
        assert_eq!(m.method, Method::CarriedForward);
        assert_eq!(m.point.lat, 52.600);
        // And it says how old the fix is, which is the whole safeguard: an
        // answer carried forward for ten hours has to look like one.
        assert_eq!(m.gap_seconds, 600 * 60);
    }

    #[test]
    fn a_photograph_long_before_the_track_starts_is_refused_by_name() {
        let error = match_at(&track(), -7200, &oldest(30)).unwrap_err();
        assert!(error.contains("before the first fix"), "got {error}");
        assert!(error.contains("2 h"), "got {error}");
    }

    #[test]
    fn a_photograph_long_after_the_track_ends_is_refused_by_name() {
        let error = match_at(&track(), 10 * 60 + 7200, &oldest(30)).unwrap_err();
        assert!(error.contains("after the last fix"), "got {error}");
    }

    #[test]
    fn a_refusal_names_the_setting_that_would_accept_it() {
        // The limit is the user's to raise, so the refusal says which one and
        // how to turn it off rather than leaving them to find it.
        let error = match_at(&track(), -7200, &oldest(30)).unwrap_err();
        assert!(error.contains("stop trusting a fix after"), "got {error}");
        assert!(error.contains("set it to 0"), "got {error}");
    }

    #[test]
    fn an_empty_library_refuses_rather_than_reporting_null_island() {
        let error = match_at(&[], 0, &Limits::default()).unwrap_err();
        assert!(error.contains("no fixes"), "got {error}");
        // And says what to do about it: an empty library is nearly always a
        // track that was read but never imported.
        assert!(error.contains("Import into the library"), "got {error}");
    }

    // ---------------------------------------------------------------------
    // The offset
    //
    // Tested against the real export rather than a synthetic track, because a
    // *uniform* track cannot separate offsets at all: shift everything by one
    // sampling interval and every gap is what it was. What discriminates is
    // the irregular spacing and the gaps a real day leaves in a real track, so
    // a synthetic fixture would have proved the estimator works on the one
    // input it will never see.
    // ---------------------------------------------------------------------

    fn specimen() -> Vec<TrackPoint> {
        super::super::gpx::parse(include_str!("testdata/owntracks-sample.gpx"))
            .unwrap()
            .points
    }

    /// Seven photographs taken while the phone was moving, on the afternoon of
    /// 4 September and the morning of the 5th, wearing a wall clock `offset`
    /// minutes east of UTC.
    fn captures_at(offset_minutes: i64) -> Vec<NaiveDateTime> {
        [
            "2026-09-04T13:12:40Z",
            "2026-09-04T13:28:55Z",
            "2026-09-04T15:15:20Z",
            "2026-09-04T15:26:03Z",
            "2026-09-05T10:21:47Z",
            "2026-09-05T10:52:10Z",
            "2026-09-05T11:06:30Z",
        ]
        .iter()
        .map(|stamp| {
            let instant = chrono::DateTime::parse_from_rfc3339(stamp)
                .unwrap()
                .timestamp();
            chrono::DateTime::from_timestamp(instant + offset_minutes * 60, 0)
                .unwrap()
                .naive_utc()
        })
        .collect()
    }

    #[test]
    fn the_offset_a_camera_was_set_to_can_be_read_off_the_track() {
        // Berlin in September. This is the answer the screen offers when
        // somebody has no idea what their camera was set to.
        let suggestion = estimate_offset(&specimen(), &captures_at(120)).unwrap();
        assert_eq!(suggestion.minutes, 120);
        assert!(suggestion.confident, "{suggestion:?}");
        assert_eq!(suggestion.plausible_low_minutes, 120);
        assert_eq!(suggestion.plausible_high_minutes, 120);
    }

    #[test]
    fn a_camera_five_hours_out_is_placed_five_hours_out() {
        let suggestion = estimate_offset(&specimen(), &captures_at(-300)).unwrap();
        assert_eq!(suggestion.minutes, -300);
        assert!(suggestion.confident);
    }

    #[test]
    fn a_zone_that_is_not_on_a_whole_hour_is_a_candidate_like_any_other() {
        // Adelaide. A search over whole hours would put these photographs half
        // an hour's driving from where they were taken.
        let suggestion = estimate_offset(&specimen(), &captures_at(570)).unwrap();
        assert_eq!(suggestion.minutes, 570);
        assert!(suggestion.confident);
    }

    #[test]
    fn an_offset_the_track_cannot_separate_is_reported_as_a_range() {
        // Kathmandu, +05:45, against a track sampled every five minutes: the
        // scores for +05:30 and +06:00 sit inside its own spacing. The honest
        // answer is the range, and the flag says the winner is not to be taken
        // on its own.
        let suggestion = estimate_offset(&specimen(), &captures_at(345)).unwrap();
        assert!(!suggestion.confident, "{suggestion:?}");
        assert!(
            suggestion.plausible_low_minutes <= 345 && 345 <= suggestion.plausible_high_minutes,
            "the range should contain the truth: {suggestion:?}"
        );
    }

    #[test]
    fn a_wrong_hour_scores_visibly_worse_than_the_right_one() {
        let points = specimen();
        let right = estimate_offset(&points, &captures_at(120)).unwrap();
        // The same photographs, read as though the camera had been an hour out.
        let hour_out: Vec<NaiveDateTime> = captures_at(120)
            .iter()
            .map(|c| *c + chrono::Duration::hours(1))
            .collect();
        let wrong = estimate_offset(&points, &hour_out).unwrap();
        assert_eq!(wrong.minutes, 180, "an hour later is an hour further east");
        assert_eq!(right.median_gap_seconds, wrong.median_gap_seconds);
    }

    #[test]
    fn photographs_the_track_does_not_cover_yield_no_confidence() {
        // A week later. Some offset still comes first; none of them mean anything.
        let elsewhere: Vec<NaiveDateTime> = captures_at(120)
            .iter()
            .map(|c| *c + chrono::Duration::days(7))
            .collect();
        let suggestion = estimate_offset(&specimen(), &elsewhere).unwrap();
        assert!(
            !suggestion.confident,
            "a week away from the track is not a confident answer: {suggestion:?}"
        );
        assert!(suggestion.median_gap_seconds > CONFIDENT_WITHIN_SECONDS);
    }

    #[test]
    fn three_photographs_are_not_enough_to_be_sure() {
        // The estimate is still offered — it is right — but the track cannot
        // rule out its neighbours on three frames, and the flag says so.
        let three = &captures_at(120)[..3];
        let suggestion = estimate_offset(&specimen(), three).unwrap();
        assert_eq!(suggestion.sample, 3);
        assert!(!suggestion.confident, "{suggestion:?}");
    }

    #[test]
    fn too_few_photographs_to_be_evidence_yield_no_suggestion() {
        let two = &captures_at(120)[..2];
        assert!(estimate_offset(&specimen(), two).is_none());
    }

    #[test]
    fn an_empty_library_yields_no_suggestion() {
        assert!(estimate_offset(&[], &captures_at(120)).is_none());
    }

    #[test]
    fn one_photograph_far_from_the_track_does_not_change_the_answer() {
        // The median's whole job: an indoor frame hours from any fix must not
        // drag the estimate the way a mean would.
        let mut captures = captures_at(120);
        captures.push(captures[0] + chrono::Duration::hours(9));
        let suggestion = estimate_offset(&specimen(), &captures).unwrap();
        assert_eq!(suggestion.minutes, 120);
    }
}
