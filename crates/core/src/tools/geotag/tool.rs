//! The matching tool: plan what would be written, then write it.
//!
//! `plan` touches nothing; `apply` writes and then reads each file back before
//! counting it. The read-back is not belt and braces — a tool that reported
//! success having done nothing is the defect this application has found in
//! itself more than once, and it is invisible without it.

use super::exif::{self, ExifPoint};
use super::join::{self, Limits, Match, Method};
use super::scan::{self, GeoStatus};
use super::{same_position, TrackPoint};
use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::ledger::Ledger;
use crate::media::{read_meta, ExifWriter};
use crate::tools::{summarise, Plan, Skip, Tool};
use chrono::NaiveDateTime;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How far either side of the photographs to read the timeline.
///
/// Wide enough that the edge tolerance can never want a fix the window did not
/// fetch, and narrow enough that a library holding years of fixes still answers
/// from an index rather than by being loaded.
const WINDOW_MARGIN_SECONDS: i64 = 6 * 3600;

/// Where the UTC offset used for a photograph came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OffsetSource {
    /// The camera recorded it. Nothing to guess, and nothing overrides it.
    File,
    /// Set on the screen, for the files that carry none.
    Chosen,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeotagParams {
    pub paths: Vec<PathBuf>,
    pub recursive: bool,
    /// Where the timeline lives. A job opens its own connection to it.
    pub database: PathBuf,
    /// The offset to use for files that do not carry one, in minutes east.
    ///
    /// `None` means none was set, and a file with no offset of its own is then
    /// skipped rather than quietly read as UTC — which would move every
    /// photograph by whatever the real offset was.
    pub utc_offset_minutes: Option<i32>,
    /// A camera clock that runs fast or slow, in seconds.
    pub clock_correction_seconds: i64,
    pub limits: Limits,
    /// Write over a position the file already carries.
    pub overwrite_existing: bool,
    pub write_altitude: bool,
}

/// One photograph, and the position it would be given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeotagAction {
    pub path: PathBuf,
    pub name: String,
    /// The camera's local wall clock.
    pub capture: NaiveDateTime,
    /// The instant that was looked up, in UTC.
    pub instant: i64,
    pub offset_minutes: i32,
    pub offset_source: OffsetSource,
    pub point: TrackPoint,
    pub method: Method,
    /// Seconds to the nearest recorded fix — how far this is from an
    /// observation.
    pub gap_seconds: i64,
    /// Exactly what will be written.
    pub exif: ExifPoint,
    /// The position being written over, where there is one.
    pub replaces: Option<ExifPoint>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeotagSummary {
    /// Written **and read back**.
    pub written: usize,
    /// Written without complaint, and not there when the file was read again.
    /// Counted apart from a failure because the two need different answers.
    pub unverified: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, String)>,
    pub skipped: Vec<Skip>,
}

impl GeotagSummary {
    pub fn describe(&self) -> String {
        let mut line = summarise(
            self.written,
            "photographs located",
            self.failures.len(),
            &self.skipped,
            &[],
        );
        if !self.unverified.is_empty() {
            line.push_str(&format!(
                ", {} written but not confirmed on re-reading",
                self.unverified.len()
            ));
        }
        line
    }
}

/// A photograph the plan has to decide about.
struct Candidate {
    path: PathBuf,
    name: String,
    capture: NaiveDateTime,
    offset_minutes: i32,
    offset_source: OffsetSource,
    existing: Option<ExifPoint>,
}

/// What the folder held, sorted into what can be matched and what cannot.
struct Candidates {
    matched: Vec<Candidate>,
    skipped: Vec<Skip>,
    /// Every dated photograph's local capture time, including those skipped.
    captures: Vec<NaiveDateTime>,
}

/// Read the folder, and set aside everything that cannot be matched, with the
/// reason it cannot.
fn candidates(p: &GeotagParams) -> Candidates {
    let files = scan::collect_inputs(&p.paths, p.recursive);
    let rows: Vec<_> = files.par_iter().map(|path| scan::row(path)).collect();

    let mut candidates = Vec::new();
    let mut skipped = Vec::new();
    let mut captures = Vec::new();

    for row in rows {
        // Every dated photograph, whatever happens to it below. The offset
        // estimate is read off these, and the moment it is most needed is the
        // moment *nothing* could be matched — a suggestion computed only from
        // the files that already worked would be silent exactly then.
        if let Some(capture) = row.capture {
            if row.status != GeoStatus::NotSupported {
                captures.push(capture);
            }
        }

        let skip = |reason: &str| Skip {
            file: row.path.to_string_lossy().to_string(),
            reason: reason.to_string(),
        };

        match row.status {
            GeoStatus::NotSupported => {
                skipped.push(skip(
                    "video: a position in a video is a different format, and this tool does not \
                     write one",
                ));
                continue;
            }
            GeoStatus::NoDate | GeoStatus::NoDateOrLocation => {
                skipped.push(skip(
                    "no capture date, so there is no moment to look up. The Dates tab is where \
                     that is repaired",
                ));
                continue;
            }
            GeoStatus::Ok if !p.overwrite_existing => {
                skipped.push(skip(
                    "already carries a position, and overwriting was not asked for",
                ));
                continue;
            }
            GeoStatus::Ok | GeoStatus::NoLocation => {}
        }

        // The camera's own offset wins. Where a file recorded the zone it was
        // in, that is a fact, and a number typed on a screen is a guess about
        // the rest of the folder.
        let (offset_minutes, offset_source) = match (row.utc_offset_minutes, p.utc_offset_minutes) {
            (Some(own), _) => (own, OffsetSource::File),
            (None, Some(chosen)) => (chosen, OffsetSource::Chosen),
            (None, None) => {
                skipped.push(skip(
                    "no UTC offset: the camera recorded none and none was set. Reading the \
                         capture time as UTC would move this photograph by whatever the offset \
                         really was",
                ));
                continue;
            }
        };

        candidates.push(Candidate {
            capture: row.capture.expect("a dated status carries a date"),
            path: row.path,
            name: row.name,
            offset_minutes,
            offset_source,
            existing: row.location,
        });
    }

    Candidates {
        matched: candidates,
        skipped,
        captures,
    }
}

/// Match a set of candidates against a timeline.
///
/// Split from [`GeotagTool::plan`] so the whole decision is testable from a
/// slice of fixes with no database and no photographs behind it.
fn plan_against(
    points: &[TrackPoint],
    candidates: Vec<Candidate>,
    p: &GeotagParams,
) -> Plan<GeotagAction> {
    let mut actions = Vec::new();
    let mut skipped = Vec::new();

    for candidate in candidates {
        let instant = candidate.capture.and_utc().timestamp()
            - i64::from(candidate.offset_minutes) * 60
            + p.clock_correction_seconds;

        match join::match_at(points, instant, &p.limits) {
            Ok(Match {
                point,
                method,
                gap_seconds,
            }) => actions.push(GeotagAction {
                path: candidate.path,
                name: candidate.name,
                capture: candidate.capture,
                instant,
                offset_minutes: candidate.offset_minutes,
                offset_source: candidate.offset_source,
                exif: exif::render(&point, p.write_altitude),
                point,
                method,
                gap_seconds,
                replaces: candidate.existing,
            }),
            Err(reason) => skipped.push(Skip {
                file: candidate.path.to_string_lossy().to_string(),
                reason,
            }),
        }
    }

    Plan { actions, skipped }
}

/// The timeline around these photographs, with the fixes either side of the
/// window included.
///
/// The window is chosen from the photographs, and the fixes bracketing them can
/// be any distance outside it. Without the two neighbours, a photograph one
/// minute past the window's edge looks like a photograph past the end of the
/// track, and the reason it is refused would name the wrong thing.
fn read_window(database: &std::path::Path, from: i64, to: i64) -> Result<Vec<TrackPoint>, Error> {
    let ledger = Ledger::open(database).map_err(Error::Sqlite)?;
    let mut points = ledger.points_between(from, to).map_err(Error::Sqlite)?;
    let (before, after) = ledger.points_around(from, to).map_err(Error::Sqlite)?;

    if let Some(before) = before {
        points.insert(0, before);
    }
    if let Some(after) = after {
        points.push(after);
    }
    Ok(points)
}

/// The window of the timeline these photographs could possibly need.
fn window(candidates: &[Candidate], p: &GeotagParams) -> Option<(i64, i64)> {
    let instants: Vec<i64> = candidates
        .iter()
        .map(|c| {
            c.capture.and_utc().timestamp() - i64::from(c.offset_minutes) * 60
                + p.clock_correction_seconds
        })
        .collect();
    let margin = p.limits.max_edge_seconds.max(WINDOW_MARGIN_SECONDS);
    Some((
        instants.iter().min()?.saturating_sub(margin),
        instants.iter().max()?.saturating_add(margin),
    ))
}

/// The plan, with the counts and the offset the photographs themselves suggest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeotagPreview {
    pub plan: Plan<GeotagAction>,
    pub matched: usize,
    pub unmatched: usize,
    /// What the photographs say the camera's offset was, where they say
    /// anything. `None` when there are too few of them, or no timeline to
    /// compare against.
    pub suggestion: Option<join::OffsetSuggestion>,
}

/// The dry run a screen shows: what would be written, and what to set the
/// offset to if none of it worked.
///
/// The suggestion travels with the plan rather than behind a button of its own.
/// The moment somebody needs it is the moment they are looking at a table of
/// refusals, and a number they have to go and ask for separately is a number
/// they will not ask for.
pub fn preview(p: &GeotagParams) -> Result<GeotagPreview, Error> {
    let found = candidates(p);

    // Wide enough for the estimator, which sweeps offsets from -12 to +14
    // hours: the instant a photograph might belong to is a day either side of
    // the wall clock it carries. One indexed range read.
    let points = match estimator_window(&found.captures) {
        Some((from, to)) => read_window(&p.database, from, to)?,
        None => Vec::new(),
    };

    let suggestion = join::estimate_offset(&points, &found.captures);
    let plan = assemble(points, found, p);

    Ok(GeotagPreview {
        matched: plan.actions.len(),
        unmatched: plan.skipped.len(),
        plan,
        suggestion,
    })
}

/// Match the candidates and put the two sets of skips back in one list.
///
/// The files that never reached the matcher — a video, an undated frame — and
/// the ones it declined belong in the same table: from the outside they are all
/// "this photograph did not get a position", and the reason is what differs.
fn assemble(points: Vec<TrackPoint>, found: Candidates, p: &GeotagParams) -> Plan<GeotagAction> {
    let Candidates {
        matched,
        mut skipped,
        ..
    } = found;
    let mut plan = plan_against(&points, matched, p);
    skipped.append(&mut plan.skipped);
    plan.skipped = skipped;
    plan
}

/// The span of timeline any offset could put these photographs in.
fn estimator_window(captures: &[NaiveDateTime]) -> Option<(i64, i64)> {
    let instants: Vec<i64> = captures.iter().map(|c| c.and_utc().timestamp()).collect();
    // Fourteen hours east, twelve west, and the margin on top.
    const SWEEP: i64 = 15 * 3600;
    Some((
        instants
            .iter()
            .min()?
            .saturating_sub(SWEEP + WINDOW_MARGIN_SECONDS),
        instants
            .iter()
            .max()?
            .saturating_add(SWEEP + WINDOW_MARGIN_SECONDS),
    ))
}

pub struct GeotagTool;

impl Tool for GeotagTool {
    type Params = GeotagParams;
    type Action = GeotagAction;
    type Summary = GeotagSummary;

    /// Dry run. Reads the timeline and the photographs; writes to neither.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let found = candidates(p);
        let points = match window(&found.matched, p) {
            Some((from, to)) => read_window(&p.database, from, to)?,
            None => Vec::new(),
        };
        Ok(Outcome {
            data: assemble(points, found, p),
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let total = plan.actions.len() as u64;
        let mut summary = GeotagSummary {
            skipped: plan.skipped,
            ..Default::default()
        };
        if total == 0 {
            return Ok(Outcome { data: summary });
        }

        // One writer for the whole batch (G4).
        let mut writer = ExifWriter::start()?;

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.path.to_string_lossy());

            if let Err(e) = writer.set_tags(&action.path, &action.exif.args()) {
                summary.failures.push((action.path.clone(), e.to_string()));
                continue;
            }

            if verify(&action) {
                summary.written += 1;
            } else {
                summary.unverified.push(action.path.clone());
            }
        }

        progress.report(total, total, "done");
        writer.close()?;
        Ok(Outcome { data: summary })
    }
}

/// Read the file back and confirm it holds the position it was given.
///
/// Compared with the same tolerance that decides whether two readings are the
/// same reading, because EXIF stores coordinates as rationals and a round trip
/// is not expected to be bit-exact.
fn verify(action: &GeotagAction) -> bool {
    let Ok(meta) = read_meta(&action.path) else {
        return false;
    };
    let Some(fix) = meta.gps else {
        return false;
    };
    same_position(
        &action.point,
        &TrackPoint {
            at: action.point.at,
            lat: fix.lat,
            lon: fix.lon,
            // Altitude is not compared: it is optional, may have been left out
            // deliberately, and is not what makes the position right.
            ele: action.point.ele,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn params() -> GeotagParams {
        GeotagParams {
            paths: vec![],
            recursive: false,
            database: PathBuf::from("unused"),
            utc_offset_minutes: Some(120),
            clock_correction_seconds: 0,
            limits: Limits::default(),
            overwrite_existing: false,
            write_altitude: true,
        }
    }

    fn local(stamp: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(stamp, "%Y:%m:%d %H:%M:%S").unwrap()
    }

    fn candidate(name: &str, capture: &str, offset: i32, source: OffsetSource) -> Candidate {
        Candidate {
            path: PathBuf::from(format!("/photos/{name}")),
            name: name.into(),
            capture: local(capture),
            offset_minutes: offset,
            offset_source: source,
            existing: None,
        }
    }

    /// Two fixes ten minutes apart, at 15:30 and 15:40 UTC on 4 September.
    fn points() -> Vec<TrackPoint> {
        vec![
            TrackPoint {
                at: 1_788_535_800,
                lat: 52.500,
                lon: 13.300,
                ele: Some(30.0),
            },
            TrackPoint {
                at: 1_788_536_400,
                lat: 52.510,
                lon: 13.310,
                ele: Some(40.0),
            },
        ]
    }

    #[test]
    fn a_local_wall_clock_is_converted_by_the_offset_before_it_is_looked_up() {
        // 17:35 in Berlin is 15:35 UTC, which falls between the two fixes: the
        // photograph belongs at the earlier one, where its subject still was.
        let plan = plan_against(
            &points(),
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:35:00",
                120,
                OffsetSource::Chosen,
            )],
            &params(),
        );

        assert_eq!(plan.skipped, vec![]);
        let action = &plan.actions[0];
        assert_eq!(action.instant, 1_788_536_100);
        assert_eq!(action.method, Method::CarriedForward);
        assert_eq!(
            action.point.lat, 52.500,
            "a fix the phone recorded, verbatim"
        );
        assert_eq!(action.exif.latitude_ref, "N");
    }

    #[test]
    fn the_same_photograph_read_at_the_wrong_offset_lands_somewhere_else_entirely() {
        // The failure this whole design is built around: an hour out is a
        // different place, and nothing about the file looks wrong afterwards.
        //
        // Note what it is *not* — a refusal. Both readings produce a confident
        // position; they simply produce different ones. Only the offset itself
        // can tell them apart, which is why the estimator exists and why an
        // unset offset is never quietly read as UTC.
        let right = plan_against(
            &points(),
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:35:00",
                120,
                OffsetSource::Chosen,
            )],
            &params(),
        );
        let mut hour_out = params();
        hour_out.utc_offset_minutes = Some(60);
        let wrong = plan_against(
            &points(),
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:35:00",
                60,
                OffsetSource::Chosen,
            )],
            &hour_out,
        );

        assert_eq!(right.actions.len(), 1);
        assert_eq!(wrong.actions.len(), 1);
        assert_ne!(
            right.actions[0].point.lat, wrong.actions[0].point.lat,
            "an hour of offset should move the photograph"
        );
        // And the wrong one is visibly further from an observation, which is
        // the only clue on the row itself.
        assert!(wrong.actions[0].gap_seconds > right.actions[0].gap_seconds);
    }

    #[test]
    fn a_camera_clock_that_runs_fast_is_corrected() {
        let mut p = params();
        p.clock_correction_seconds = -300; // the clock was five minutes ahead
        let plan = plan_against(
            &points(),
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:40:00",
                120,
                OffsetSource::Chosen,
            )],
            &p,
        );
        assert_eq!(plan.actions[0].instant, 1_788_536_100);
    }

    #[test]
    fn the_row_says_where_its_offset_came_from() {
        // A folder can hold a phone's photographs, which carry their own
        // offset, beside a camera's, which do not. The table has to be able to
        // show that the two were treated differently.
        let plan = plan_against(
            &points(),
            vec![
                candidate("phone.jpg", "2026:09:04 17:35:00", 120, OffsetSource::File),
                candidate(
                    "camera.jpg",
                    "2026:09:04 17:35:00",
                    120,
                    OffsetSource::Chosen,
                ),
            ],
            &params(),
        );
        assert_eq!(plan.actions[0].offset_source, OffsetSource::File);
        assert_eq!(plan.actions[1].offset_source, OffsetSource::Chosen);
    }

    #[test]
    fn a_photograph_the_timeline_cannot_place_is_skipped_with_the_reason_on_it() {
        // Five hours past the last fix, against a ceiling of half an hour: the
        // track does not cover this photograph and says so.
        let mut p = params();
        p.limits.max_edge_seconds = 30 * 60;
        let plan = plan_against(
            &points(),
            vec![candidate(
                "far.jpg",
                "2026:09:04 23:00:00",
                120,
                OffsetSource::Chosen,
            )],
            &p,
        );
        assert_eq!(plan.actions, vec![]);
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].reason.contains("after the last fix"));
    }

    #[test]
    fn nearest_mode_takes_the_closer_fix_from_either_side() {
        let mut p = params();
        p.limits.mode = join::Mode::Nearest;
        let plan = plan_against(
            &points(),
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:36:00",
                120,
                OffsetSource::Chosen,
            )],
            &p,
        );
        assert_eq!(plan.actions[0].method, Method::Nearest);
        assert_eq!(plan.actions[0].point.lat, 52.510);
    }

    #[test]
    fn leaving_altitude_out_leaves_it_out_of_what_will_be_written() {
        let mut p = params();
        p.write_altitude = false;
        let plan = plan_against(
            &points(),
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:35:00",
                120,
                OffsetSource::Chosen,
            )],
            &p,
        );
        assert_eq!(plan.actions[0].exif.altitude, None);
        assert!(!plan.actions[0]
            .exif
            .args()
            .iter()
            .any(|a| a.contains("Altitude")));
    }

    #[test]
    fn an_empty_timeline_skips_every_photograph_rather_than_placing_it_nowhere() {
        let plan = plan_against(
            &[],
            vec![candidate(
                "a.jpg",
                "2026:09:04 17:35:00",
                120,
                OffsetSource::Chosen,
            )],
            &params(),
        );
        assert_eq!(plan.actions, vec![]);
        assert!(plan.skipped[0].reason.contains("no fixes"));
    }

    #[test]
    fn the_window_covers_every_photograph_and_the_tolerance_around_them() {
        let p = params();
        let (from, to) = window(
            &[
                candidate("a.jpg", "2026:09:04 17:35:00", 120, OffsetSource::Chosen),
                candidate("b.jpg", "2026:09:04 18:35:00", 120, OffsetSource::Chosen),
            ],
            &p,
        )
        .unwrap();

        assert!(from <= 1_788_536_100 - p.limits.max_edge_seconds);
        assert!(to >= 1_788_539_700 + p.limits.max_edge_seconds);
    }

    #[test]
    fn the_two_kinds_of_skip_end_up_in_one_table() {
        // A video and a photograph the track could not place are different
        // problems, and from the outside they are the same sentence: this file
        // did not get a position, and here is why.
        let found = Candidates {
            matched: vec![candidate(
                "far.jpg",
                "2026:09:04 23:00:00",
                120,
                OffsetSource::Chosen,
            )],
            skipped: vec![Skip {
                file: "/photos/clip.mov".into(),
                reason: "video".into(),
            }],
            captures: vec![],
        };
        let mut p = params();
        p.limits.max_edge_seconds = 30 * 60;
        let plan = assemble(points(), found, &p);

        assert_eq!(plan.actions, vec![]);
        assert_eq!(plan.skipped.len(), 2);
        assert_eq!(plan.skipped[0].file, "/photos/clip.mov");
        assert!(plan.skipped[1].reason.contains("after the last fix"));
    }

    #[test]
    fn a_run_that_did_nothing_says_why_rather_than_reporting_zero_of_zero() {
        let summary = GeotagSummary {
            skipped: vec![Skip {
                file: "/photos/a.jpg".into(),
                reason: "already carries a position".into(),
            }],
            ..Default::default()
        };
        let line = summary.describe();
        assert!(line.contains("Nothing to do"), "got {line}");
        assert!(line.contains("already carries a position"), "got {line}");
    }

    #[test]
    fn a_write_that_could_not_be_confirmed_is_counted_apart_from_a_success() {
        let summary = GeotagSummary {
            written: 3,
            unverified: vec![PathBuf::from("/photos/b.jpg")],
            ..Default::default()
        };
        let line = summary.describe();
        assert!(line.contains('3'), "got {line}");
        assert!(line.contains("not confirmed"), "got {line}");
    }
}
