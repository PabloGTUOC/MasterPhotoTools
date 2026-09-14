//! Google's Timeline export, as a phone writes it.
//!
//! **Beyond the specification**, like the rest of `geotag`; the reasoning is in
//! `docs/timeline-plan.md`, where this is Stage B.
//!
//! Since December 2024 a Google account's Timeline lives on the phone, and
//! Takeout hands back settings and an apology. The phone's own export is the
//! data: one JSON array of segments, each with a start, an end, and one of
//! three shapes.
//!
//! | Shape | What it holds |
//! |---|---|
//! | `timelinePath` | positions, each at so many minutes past the segment's start |
//! | `visit` | one place, held for the whole segment |
//! | `activity` | a journey's two endpoints — **not read**, see below |
//!
//! **Every position here is [`PointSource::Inferred`].** These are not fixes a
//! device recorded: they are Google's reconstruction of where somebody was —
//! snapped to roads, smoothed, and carrying its own probability. Calling them
//! recorded would make them indistinguishable from a `.gpx` off a phone, and
//! the whole point of provenance is that they are not the same claim.
//!
//! **`activity` segments are read past on purpose.** Their two endpoints are
//! the same moments a visit or a path already describes, at coordinates
//! rounded differently, so reading them would make one import disagree with
//! itself at hundreds of instants — a fault reported to the user about nothing
//! that is wrong.

use super::gpx::ParsedTrack;
use super::TrackPoint;
use crate::error::Error;
use chrono::DateTime;
use serde::Deserialize;

/// One segment of the export.
///
/// Only the fields a timeline needs are named; `serde` ignores the rest, which
/// is what keeps this tolerant of Google adding more.
#[derive(Debug, Deserialize)]
struct Segment {
    #[serde(rename = "startTime")]
    start_time: Option<String>,
    #[serde(rename = "endTime")]
    end_time: Option<String>,
    visit: Option<Visit>,
    #[serde(rename = "timelinePath")]
    timeline_path: Option<Vec<PathPoint>>,
}

#[derive(Debug, Deserialize)]
struct Visit {
    #[serde(rename = "topCandidate")]
    top_candidate: Option<VisitCandidate>,
}

#[derive(Debug, Deserialize)]
struct VisitCandidate {
    #[serde(rename = "placeLocation")]
    place_location: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PathPoint {
    point: Option<String>,
    /// Minutes past the segment's `startTime`, as a string.
    #[serde(rename = "durationMinutesOffsetFromStartTime")]
    offset_minutes: Option<String>,
}

/// True where this text is a Google Timeline export rather than GPX.
///
/// By the first character that is not whitespace: GPX is XML and this is a JSON
/// array. Cheap, and it cannot mistake one for the other.
pub fn looks_like_google(text: &str) -> bool {
    matches!(text.trim_start().chars().next(), Some('[') | Some('{'))
}

/// Read `geo:52.521918,13.413215`.
fn geo(value: &str) -> Option<(f64, f64)> {
    let rest = value.trim().strip_prefix("geo:")?;
    let (lat, lon) = rest.split_once(',')?;
    let lat: f64 = lat.trim().parse().ok()?;
    let lon: f64 = lon.trim().parse().ok()?;
    (-90.0..=90.0).contains(&lat).then_some(())?;
    (-180.0..=180.0).contains(&lon).then_some(())?;
    Some((lat, lon))
}

/// Unix seconds from one of the export's timestamps.
///
/// **They carry their real offset** — `2026-09-13T18:37:24.615+02:00` — which
/// is worth more than it looks: everywhere else in this module a local day has
/// to be configured or guessed, and here the file says.
fn instant(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

/// Parse an export into the same shape a `.gpx` parses into.
///
/// Points land in time order with repeats at one instant collapsed, exactly as
/// `gpx::parse` leaves them, because everything downstream — the diff, the
/// import, the conflict rules — is written against that shape.
pub fn parse(text: &str) -> Result<ParsedTrack, Error> {
    let segments: Vec<Segment> = serde_json::from_str(text)
        .map_err(|e| Error::Config(format!("That is not a Google Timeline export: {e}")))?;

    let mut points: Vec<TrackPoint> = Vec::new();
    let mut rejected = Vec::new();

    for (index, segment) in segments.iter().enumerate() {
        let start = segment.start_time.as_deref().and_then(instant);

        if let Some(path) = &segment.timeline_path {
            let Some(start) = start else {
                rejected.push(super::gpx::RejectedPoint {
                    index: index + 1,
                    reason: "a path segment with no readable startTime".into(),
                });
                continue;
            };
            for step in path {
                let Some((lat, lon)) = step.point.as_deref().and_then(geo) else {
                    continue;
                };
                let minutes: i64 = step
                    .offset_minutes
                    .as_deref()
                    .and_then(|m| m.trim().parse().ok())
                    .unwrap_or(0);
                points.push(TrackPoint {
                    at: start + minutes * 60,
                    lat,
                    lon,
                    // Google's export carries no altitude, and absent stays
                    // absent: "no altitude" is not the claim "zero metres".
                    ele: None,
                });
            }
            continue;
        }

        if let Some(visit) = &segment.visit {
            let Some((lat, lon)) = visit
                .top_candidate
                .as_ref()
                .and_then(|c| c.place_location.as_deref())
                .and_then(geo)
            else {
                rejected.push(super::gpx::RejectedPoint {
                    index: index + 1,
                    reason: "a visit with no readable place".into(),
                });
                continue;
            };

            // Both ends, which is what a stay *is*: there at the start, there
            // at the end, and the matcher carries the position forward across
            // the middle rather than this inventing points through it.
            for edge in [start, segment.end_time.as_deref().and_then(instant)]
                .into_iter()
                .flatten()
            {
                points.push(TrackPoint {
                    at: edge,
                    lat,
                    lon,
                    ele: None,
                });
            }
        }
    }

    if points.is_empty() {
        return Err(Error::Config(
            "That export holds no positions this reads: no timelinePath and no visit with a place"
                .into(),
        ));
    }

    points.sort_by_key(|p| p.at);

    // One instant, one position — the rule the timeline itself keeps. Within a
    // single export the repeats are a path point and a visit's edge describing
    // the same minute, so the first is kept and the rest counted rather than
    // reported one by one: a thousand lines about a file agreeing with itself
    // is not a report.
    let mut kept: Vec<TrackPoint> = Vec::with_capacity(points.len());
    let mut collapsed = 0usize;
    for point in points {
        match kept.last() {
            Some(previous) if previous.at == point.at => collapsed += 1,
            _ => kept.push(point),
        }
    }
    if collapsed > 0 {
        rejected.push(super::gpx::RejectedPoint {
            index: 0,
            reason: format!(
                "{collapsed} position(s) fell on an instant the export already \
                 described; the first of each was kept"
            ),
        });
    }

    Ok(ParsedTrack {
        // What wrote it, in the field a `.gpx` keeps the same answer in.
        creator: Some("Google Timeline".to_string()),
        points: kept,
        rejected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shapes as a phone actually writes them.
    const EXPORT: &str = r#"[
      {
        "startTime": "2026-04-01T13:24:41.400+02:00",
        "endTime": "2026-04-01T14:08:09.741+02:00",
        "visit": {
          "hierarchyLevel": "0",
          "probability": "0.698345",
          "topCandidate": {
            "probability": "0.92",
            "semanticType": "Home",
            "placeID": "ChIJexample",
            "placeLocation": "geo:52.521918,13.413215"
          }
        }
      },
      {
        "startTime": "2026-04-01T15:00:00.000+02:00",
        "endTime": "2026-04-01T15:30:00.000+02:00",
        "timelinePath": [
          { "point": "geo:48.858370,2.294481", "durationMinutesOffsetFromStartTime": "0" },
          { "point": "geo:48.860000,2.300000", "durationMinutesOffsetFromStartTime": "13" }
        ]
      },
      {
        "startTime": "2026-04-01T16:00:00.000+02:00",
        "endTime": "2026-04-01T16:40:00.000+02:00",
        "activity": {
          "start": "geo:1.000000,1.000000",
          "end": "geo:2.000000,2.000000",
          "distanceMeters": "1200.0",
          "probability": "0.9",
          "topCandidate": { "type": "walking", "probability": "0.8" }
        }
      }
    ]"#;

    #[test]
    fn a_visit_becomes_its_two_ends_and_a_path_becomes_its_points() {
        let parsed = parse(EXPORT).unwrap();

        // Two visit edges and two path points; the activity is read past.
        assert_eq!(parsed.points.len(), 4, "got {:?}", parsed.points);
        assert_eq!(parsed.creator.as_deref(), Some("Google Timeline"));

        // 13:24:41+02:00 is 11:24:41Z.
        assert_eq!(parsed.points[0].at, 1_775_042_681);
        assert_eq!(parsed.points[0].lat, 52.521918);
        assert!(parsed.points.iter().all(|p| p.ele.is_none()));
    }

    #[test]
    fn a_path_points_offset_is_minutes_past_the_segment_start() {
        let parsed = parse(EXPORT).unwrap();
        let path: Vec<i64> = parsed.points[2..].iter().map(|p| p.at).collect();
        assert_eq!(path[1] - path[0], 13 * 60, "thirteen minutes later");
    }

    #[test]
    fn the_offset_in_the_file_is_believed() {
        // The same wall-clock time in two offsets is two different instants,
        // and the file says which. Nothing here needs a configured timezone.
        let winter = parse(&EXPORT.replace("+02:00", "+01:00")).unwrap();
        let summer = parse(EXPORT).unwrap();
        assert_eq!(winter.points[0].at - summer.points[0].at, 3_600);
    }

    #[test]
    fn an_activity_segment_contributes_nothing() {
        // Its endpoints are moments a visit or a path already describes, at
        // coordinates rounded differently: reading them would make one import
        // disagree with itself.
        let parsed = parse(EXPORT).unwrap();
        assert!(
            !parsed.points.iter().any(|p| p.lat == 1.0 || p.lat == 2.0),
            "the activity's endpoints are not in the timeline"
        );
    }

    #[test]
    fn two_positions_for_one_instant_keep_the_first_and_are_counted() {
        let doubled = EXPORT.replace(
            r#""durationMinutesOffsetFromStartTime": "13""#,
            r#""durationMinutesOffsetFromStartTime": "0""#,
        );
        let parsed = parse(&doubled).unwrap();

        assert_eq!(parsed.points.len(), 3, "the repeat collapsed");
        assert!(
            parsed
                .rejected
                .iter()
                .any(|r| r.reason.contains("already described")),
            "and it is counted rather than silent: {:?}",
            parsed.rejected
        );
    }

    #[test]
    fn a_gpx_file_is_not_mistaken_for_an_export() {
        assert!(!looks_like_google("<?xml version=\"1.0\"?><gpx/>"));
        assert!(!looks_like_google("  <gpx creator=\"OwnTracks\"/>"));
        assert!(looks_like_google("  [ {} ]"));
        assert!(looks_like_google("{\"semanticSegments\": []}"));
    }

    #[test]
    fn something_that_is_not_an_export_is_refused_by_name() {
        let error = parse("{\"hello\": 1}").unwrap_err().to_string();
        assert!(
            error.contains("not a Google Timeline export"),
            "got: {error}"
        );
    }

    #[test]
    fn an_export_with_no_positions_at_all_is_refused() {
        let error = parse("[]").unwrap_err().to_string();
        assert!(error.contains("holds no positions"), "got: {error}");
    }
}
