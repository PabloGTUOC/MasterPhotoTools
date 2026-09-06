//! Reading GPX — the phone's export, as points on a timeline.
//!
//! Hand-written rather than delegated to an XML crate, and deliberately so.
//! GPX is a handful of elements with two attributes that matter, the reader
//! below is the whole of it, and a new dependency would need a justification
//! under G8 that "we needed to find `lat` and `lon`" cannot earn.
//!
//! **A malformed point is reported, not dropped.** A file of ten thousand fixes
//! where one has no timestamp should import the other nine thousand nine
//! hundred and ninety-nine and say what happened to the one — so structural
//! failures (no root element, a tag that never closes) are errors, and
//! individual points that cannot be used are rejections with a reason attached.

use super::{metres_between, same_position, TrackPoint};
use crate::error::Error;
use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// The point elements a track can be made of. `<trkpt>` is what a phone
/// writes; the other two are read because a file holding only waypoints is
/// still a record of where somebody was, and refusing it would be arbitrary.
const POINT_ELEMENTS: [&str; 3] = ["trkpt", "wpt", "rtept"];

/// A point that could not be used, and why.
///
/// `index` counts point elements in file order, so "the 412th `<trkpt>`" can be
/// found by a human looking at the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedPoint {
    pub index: usize,
    pub reason: String,
}

/// One parsed file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedTrack {
    /// The `creator` attribute of the root element, e.g. `OwnTracks`. Kept
    /// because it is the only clue a file carries about which device wrote it,
    /// and that is the first question asked when two files disagree.
    pub creator: Option<String>,
    /// Sorted by time, with points repeated exactly within the file collapsed.
    pub points: Vec<TrackPoint>,
    pub rejected: Vec<RejectedPoint>,
}

impl ParsedTrack {
    pub fn first_fix(&self) -> Option<i64> {
        self.points.first().map(|p| p.at)
    }

    pub fn last_fix(&self) -> Option<i64> {
        self.points.last().map(|p| p.at)
    }

    /// The corners of the box the track sits in, as
    /// `(min_lat, min_lon, max_lat, max_lon)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let first = self.points.first()?;
        let mut b = (first.lat, first.lon, first.lat, first.lon);
        for p in &self.points {
            b.0 = b.0.min(p.lat);
            b.1 = b.1.min(p.lon);
            b.2 = b.2.max(p.lat);
            b.3 = b.3.max(p.lon);
        }
        Some(b)
    }
}

/// Parse a GPX document.
///
/// Fails only where the file is not a track at all or the markup is broken
/// beyond following; everything a *point* gets wrong is a rejection.
pub fn parse(text: &str) -> Result<ParsedTrack, Error> {
    let creator = root_creator(text)?;

    let mut collected: Vec<TrackPoint> = Vec::new();
    let mut rejected: Vec<RejectedPoint> = Vec::new();
    let mut index = 0usize;
    let mut cursor = 0usize;

    while let Some(tag) = next_start_tag(text, cursor)? {
        cursor = tag.end;
        if !POINT_ELEMENTS.contains(&local_name(tag.name)) {
            continue;
        }

        index += 1;
        let body = if tag.self_closing {
            ""
        } else {
            let (body, after) = element_body(text, tag.name, tag.end)?;
            cursor = after;
            body
        };

        match read_point(tag.attrs, body) {
            Ok(point) => collected.push(point),
            Err(reason) => rejected.push(RejectedPoint { index, reason }),
        }
    }

    // Stable, so "the first one wins" below means the first in the file.
    collected.sort_by_key(|p| p.at);

    // A file can disagree with itself: two fixes for one second, from two
    // devices merged into one export. Identical repeats collapse silently
    // because they say nothing new; a genuine disagreement keeps the first and
    // is reported, for the same reason two *files* disagreeing is reported —
    // one instant has one position, and picking silently would hide a fault.
    let mut points: Vec<TrackPoint> = Vec::with_capacity(collected.len());
    for point in collected {
        match points.last() {
            Some(previous) if previous.at == point.at => {
                if !same_position(previous, &point) {
                    rejected.push(RejectedPoint {
                        index: 0,
                        reason: format!(
                            "two different positions are given for {}: kept the first, \
                             which is {:.0} m from the second",
                            stamp(point.at),
                            metres_between(previous, &point)
                        ),
                    });
                }
            }
            _ => points.push(point),
        }
    }

    Ok(ParsedTrack {
        creator,
        points,
        rejected,
    })
}

/// Turn one point element into a fix, or say why it cannot be one.
fn read_point(attrs: &str, body: &str) -> Result<TrackPoint, String> {
    let lat = coordinate(attrs, "lat", 90.0)?;
    let lon = coordinate(attrs, "lon", 180.0)?;

    let raw_time = first_child_text(body, "time")
        .ok_or_else(|| "no <time>, so there is no moment to join a photograph to".to_string())?;
    let at = parse_time(&raw_time)
        .ok_or_else(|| format!("the timestamp {:?} is not one this reads", raw_time.trim()))?;

    // An unreadable elevation is not a reason to throw the position away: the
    // coordinate is what the photograph needs, and altitude is optional in GPX.
    let ele = first_child_text(body, "ele").and_then(|raw| raw.trim().parse::<f64>().ok());

    Ok(TrackPoint { at, lat, lon, ele })
}

fn coordinate(attrs: &str, name: &str, limit: f64) -> Result<f64, String> {
    let raw = attribute(attrs, name).ok_or_else(|| format!("no {name} attribute"))?;
    let value: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("{name}={raw:?} is not a number"))?;
    if !value.is_finite() || value.abs() > limit {
        return Err(format!("{name}={value} is outside ±{limit}"));
    }
    Ok(value)
}

/// Parse a GPX timestamp to Unix seconds.
///
/// GPX times are UTC by definition, so a stamp with no zone is taken as UTC
/// rather than as local time — the opposite of the EXIF rule, and the reason
/// the two cannot be joined without an offset.
fn parse_time(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.timestamp());
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(raw, format) {
            return Some(dt.and_utc().timestamp());
        }
    }
    None
}

/// A UTC instant, for a message a human reads.
fn stamp(at: i64) -> String {
    DateTime::from_timestamp(at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| at.to_string())
}

// ---------------------------------------------------------------------------
// The scanner
// ---------------------------------------------------------------------------

struct StartTag<'a> {
    /// As written, prefix included; compare with [`local_name`].
    name: &'a str,
    attrs: &'a str,
    self_closing: bool,
    /// Just past the closing `>`.
    end: usize,
}

/// The element name without its namespace prefix, so `<gpx:trkpt>` reads as
/// `trkpt`. Files written against a namespaced schema are otherwise invisible
/// to a reader looking for a bare name.
fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// The root element's `creator`, and the check that this is a GPX file at all.
fn root_creator(text: &str) -> Result<Option<String>, Error> {
    let mut cursor = 0usize;
    while let Some(tag) = next_start_tag(text, cursor)? {
        cursor = tag.end;
        if local_name(tag.name) == "gpx" {
            return Ok(attribute(tag.attrs, "creator"));
        }
    }
    Err(Error::Config(
        "Not a GPX file: no <gpx> element. A track is exported from the phone as .gpx".into(),
    ))
}

/// The next start tag at or after `from`, skipping comments, declarations,
/// CDATA and closing tags.
fn next_start_tag(text: &str, from: usize) -> Result<Option<StartTag<'_>>, Error> {
    let mut cursor = from;
    loop {
        let Some(offset) = text[cursor..].find('<') else {
            return Ok(None);
        };
        let open = cursor + offset;
        let rest = &text[open..];

        if rest.starts_with("<!--") {
            cursor = skip_to(text, open, "-->", "a comment")?;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            cursor = skip_to(text, open, "]]>", "a CDATA section")?;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            cursor = tag_end(text, open)? + 1;
            continue;
        }
        if rest.starts_with("</") {
            cursor = tag_end(text, open)? + 1;
            continue;
        }

        let close = tag_end(text, open)?;
        let inner = &text[open + 1..close];
        let self_closing = inner.trim_end().ends_with('/');
        let inner = inner.trim_end().trim_end_matches('/');

        let split = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        return Ok(Some(StartTag {
            name: &inner[..split],
            attrs: &inner[split..],
            self_closing,
            end: close + 1,
        }));
    }
}

/// The text between a start tag and its matching close, and where to carry on.
///
/// Depth is counted, so an element nesting one of its own kind does not end at
/// the inner close. GPX never does that; the cost of being right about it is
/// four lines.
fn element_body<'a>(text: &'a str, name: &str, from: usize) -> Result<(&'a str, usize), Error> {
    let wanted = local_name(name);
    let mut depth = 1usize;
    let mut cursor = from;

    loop {
        let Some(offset) = text[cursor..].find('<') else {
            return Err(unclosed(text, from, wanted));
        };
        let open = cursor + offset;
        let rest = &text[open..];

        if rest.starts_with("<!--") {
            cursor = skip_to(text, open, "-->", "a comment")?;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            cursor = skip_to(text, open, "]]>", "a CDATA section")?;
            continue;
        }

        let close = tag_end(text, open)?;
        let inner = &text[open + 1..close];

        if let Some(closing) = inner.strip_prefix('/') {
            if local_name(closing.trim()) == wanted {
                depth -= 1;
                if depth == 0 {
                    return Ok((&text[from..open], close + 1));
                }
            }
        } else if !inner.trim_end().ends_with('/')
            && !inner.starts_with('?')
            && !inner.starts_with('!')
        {
            let split = inner
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inner.len());
            if local_name(&inner[..split]) == wanted {
                depth += 1;
            }
        }

        cursor = close + 1;
    }
}

/// The text of the first child element with this local name.
fn first_child_text(body: &str, want: &str) -> Option<String> {
    let mut cursor = 0usize;
    while let Ok(Some(tag)) = next_start_tag(body, cursor) {
        cursor = tag.end;
        if local_name(tag.name) != want {
            continue;
        }
        if tag.self_closing {
            return Some(String::new());
        }
        let (text, _) = element_body(body, tag.name, tag.end).ok()?;
        return Some(decode_entities(text));
    }
    None
}

/// The index of the `>` closing the tag that opens at `open`.
///
/// Quotes are tracked because an attribute value may legally contain `>`, and
/// a reader that stops at the first one truncates the tag and loses the
/// attributes after it.
fn tag_end(text: &str, open: usize) -> Result<usize, Error> {
    let mut quote: Option<char> = None;
    for (offset, ch) in text[open..].char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(ch),
            (None, '>') => return Ok(open + offset),
            (None, _) => {}
        }
    }
    Err(malformed(
        text,
        open,
        "a tag that is never closed with '>'".into(),
    ))
}

fn skip_to(text: &str, open: usize, terminator: &str, what: &str) -> Result<usize, Error> {
    text[open..]
        .find(terminator)
        .map(|offset| open + offset + terminator.len())
        .ok_or_else(|| malformed(text, open, format!("{what} that is never closed")))
}

/// One attribute's value, with the five predefined entities decoded.
///
/// The name must be whole: a reader that took `lat` from `xlat` would read a
/// vendor extension as a coordinate.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let mut cursor = 0usize;
    while let Some(offset) = attrs[cursor..].find(name) {
        let at = cursor + offset;
        cursor = at + name.len();

        let before_is_boundary = attrs[..at]
            .chars()
            .next_back()
            .map(|c| c.is_whitespace())
            .unwrap_or(true);
        if !before_is_boundary {
            continue;
        }

        let after = attrs[cursor..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &after[quote.len_utf8()..];
        let end = value.find(quote)?;
        return Some(decode_entities(&value[..end]));
    }
    None
}

fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    raw.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Where in the file something went wrong, in the terms a person looking at it
/// has: a line number, not a byte offset.
fn malformed(text: &str, at: usize, what: String) -> Error {
    let line = text[..at.min(text.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1;
    Error::Config(format!("Malformed GPX at line {line}: {what}"))
}

fn unclosed(text: &str, at: usize, name: &str) -> Error {
    malformed(text, at, format!("<{name}> is never closed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape OwnTracks writes, which is the shape the sample track is in.
    fn document(points: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<gpx xmlns="http://www.topografix.com/GPX/1/1" version="1.1" creator="OwnTracks">
<trk><trkseg>
{points}
</trkseg></trk></gpx>"#
        )
    }

    /// A real export, frozen as a specimen.
    ///
    /// A copy rather than a reach into `docs/GPS/`: the crate's tests must not
    /// depend on a file outside it that somebody is free to move, and what is
    /// wanted here is *an* OwnTracks export exactly as the phone wrote it, not
    /// whichever one happens to be in the working tree today.
    #[test]
    fn the_sample_track_reads_as_fifty_fixes() {
        let text = include_str!("testdata/owntracks-sample.gpx");
        let track = parse(text).expect("the sample track parses");

        assert_eq!(track.points.len(), 50);
        assert_eq!(track.rejected, vec![]);
        assert_eq!(track.creator.as_deref(), Some("OwnTracks"));

        // 2026-09-02T19:40:44Z and 2026-09-05T16:33:09Z, the file's first and last.
        assert_eq!(track.first_fix(), Some(1_788_378_044));
        assert_eq!(track.last_fix(), Some(1_788_625_989));

        let first = track.points[0];
        assert_eq!(first.lat, 52.509998);
        assert_eq!(first.lon, 13.419901);
        assert_eq!(first.ele, Some(66.71));
    }

    #[test]
    fn attributes_are_read_in_either_order() {
        let track = parse(&document(
            r#"<trkpt lon="13.4" lat="52.5"><time>2026-09-02T19:40:44Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points[0].lat, 52.5);
        assert_eq!(track.points[0].lon, 13.4);
    }

    #[test]
    fn a_namespaced_document_reads_like_a_bare_one() {
        let text = r#"<gpx:gpx xmlns:gpx="http://www.topografix.com/GPX/1/1" creator="Phone">
            <gpx:trk><gpx:trkseg>
            <gpx:trkpt lat="52.5" lon="13.4">
                <gpx:ele>36.4</gpx:ele>
                <gpx:time>2026-09-02T19:40:44Z</gpx:time>
            </gpx:trkpt>
            </gpx:trkseg></gpx:trk></gpx:gpx>"#;
        let track = parse(text).unwrap();
        assert_eq!(track.points.len(), 1);
        assert_eq!(track.points[0].ele, Some(36.4));
        assert_eq!(track.creator.as_deref(), Some("Phone"));
    }

    #[test]
    fn a_point_with_no_elevation_is_a_point_with_no_elevation() {
        let track = parse(&document(
            r#"<trkpt lat="52.5" lon="13.4"><time>2026-09-02T19:40:44Z</time></trkpt>"#,
        ))
        .unwrap();
        // None, not zero: a phone writes a literal 0.00 when its altitude fix
        // drops out, and the two must stay distinguishable.
        assert_eq!(track.points[0].ele, None);
    }

    #[test]
    fn a_point_with_no_time_is_rejected_with_a_reason() {
        let track = parse(&document(
            r#"<trkpt lat="52.5" lon="13.4"><ele>36.4</ele></trkpt>"#,
        ))
        .unwrap();
        assert!(track.points.is_empty());
        assert_eq!(track.rejected.len(), 1);
        assert_eq!(track.rejected[0].index, 1);
        assert!(
            track.rejected[0].reason.contains("<time>"),
            "the reason should name what was missing, got {:?}",
            track.rejected[0].reason
        );
    }

    #[test]
    fn a_point_with_no_coordinate_is_rejected_rather_than_dropped() {
        let track = parse(&document(
            r#"<trkpt lat="52.5"><time>2026-09-02T19:40:44Z</time></trkpt>"#,
        ))
        .unwrap();
        assert!(track.points.is_empty());
        assert!(track.rejected[0].reason.contains("lon"));
    }

    #[test]
    fn a_coordinate_outside_the_globe_is_rejected() {
        let track = parse(&document(
            r#"<trkpt lat="152.5" lon="13.4"><time>2026-09-02T19:40:44Z</time></trkpt>"#,
        ))
        .unwrap();
        assert!(track.points.is_empty());
        assert!(track.rejected[0].reason.contains("outside"));
    }

    #[test]
    fn several_segments_and_tracks_become_one_timeline() {
        let text = r#"<gpx creator="Phone">
            <trk><trkseg>
              <trkpt lat="52.1" lon="13.1"><time>2026-09-02T10:00:00Z</time></trkpt>
            </trkseg><trkseg>
              <trkpt lat="52.2" lon="13.2"><time>2026-09-02T11:00:00Z</time></trkpt>
            </trkseg></trk>
            <trk><trkseg>
              <trkpt lat="52.3" lon="13.3"><time>2026-09-02T12:00:00Z</time></trkpt>
            </trkseg></trk>
            <wpt lat="52.4" lon="13.4"><time>2026-09-02T13:00:00Z</time></wpt>
        </gpx>"#;
        let track = parse(text).unwrap();
        assert_eq!(track.points.len(), 4);
    }

    #[test]
    fn points_out_of_order_come_back_in_order() {
        let track = parse(&document(
            r#"<trkpt lat="52.2" lon="13.2"><time>2026-09-02T11:00:00Z</time></trkpt>
               <trkpt lat="52.1" lon="13.1"><time>2026-09-02T10:00:00Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points[0].lat, 52.1);
        assert_eq!(track.points[1].lat, 52.2);
    }

    #[test]
    fn one_fix_written_twice_is_recorded_once() {
        let track = parse(&document(
            r#"<trkpt lat="52.1" lon="13.1"><time>2026-09-02T10:00:00Z</time></trkpt>
               <trkpt lat="52.1" lon="13.1"><time>2026-09-02T10:00:00Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points.len(), 1);
        assert_eq!(track.rejected, vec![]);
    }

    #[test]
    fn a_file_that_disagrees_with_itself_keeps_the_first_and_says_so() {
        let track = parse(&document(
            r#"<trkpt lat="52.1" lon="13.1"><time>2026-09-02T10:00:00Z</time></trkpt>
               <trkpt lat="52.2" lon="13.2"><time>2026-09-02T10:00:00Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points.len(), 1);
        assert_eq!(track.points[0].lat, 52.1);
        assert_eq!(track.rejected.len(), 1);
        assert!(
            track.rejected[0].reason.contains("2026-09-02 10:00:00 UTC"),
            "the reason should name the instant, got {:?}",
            track.rejected[0].reason
        );
    }

    #[test]
    fn carriage_returns_do_not_change_what_is_read() {
        let unix = document(
            r#"<trkpt lat="52.5" lon="13.4"><ele>36.4</ele><time>2026-09-02T19:40:44Z</time></trkpt>"#,
        );
        let dos = unix.replace('\n', "\r\n");
        assert_eq!(parse(&unix).unwrap().points, parse(&dos).unwrap().points);
    }

    #[test]
    fn comments_and_declarations_are_stepped_over() {
        let text = r#"<?xml version="1.0"?>
            <!-- exported 2026-09-06, <trkpt lat="0" lon="0"> is not a point -->
            <gpx creator="Phone">
              <trkpt lat="52.5" lon="13.4"><time>2026-09-02T19:40:44Z</time></trkpt>
            </gpx>"#;
        let track = parse(text).unwrap();
        assert_eq!(track.points.len(), 1);
        assert_eq!(track.points[0].lat, 52.5);
    }

    #[test]
    fn a_self_closing_point_is_rejected_rather_than_read_as_the_next_one() {
        let track = parse(&document(
            r#"<trkpt lat="52.1" lon="13.1" />
               <trkpt lat="52.2" lon="13.2"><time>2026-09-02T10:00:00Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points.len(), 1);
        assert_eq!(track.points[0].lat, 52.2);
        assert_eq!(track.rejected.len(), 1);
    }

    #[test]
    fn a_timestamp_with_an_offset_is_converted_to_utc() {
        let track = parse(&document(
            r#"<trkpt lat="52.5" lon="13.4"><time>2026-09-02T21:40:44+02:00</time></trkpt>"#,
        ))
        .unwrap();
        // The same instant as 19:40:44Z, which is the sample track's first fix.
        assert_eq!(track.points[0].at, 1_788_378_044);
    }

    #[test]
    fn a_timestamp_with_no_zone_is_taken_as_utc() {
        // GPX times are UTC by definition — the opposite of the EXIF rule.
        let track = parse(&document(
            r#"<trkpt lat="52.5" lon="13.4"><time>2026-09-02T19:40:44</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points[0].at, 1_788_378_044);
    }

    #[test]
    fn sub_second_precision_is_accepted_and_truncated() {
        let track = parse(&document(
            r#"<trkpt lat="52.5" lon="13.4"><time>2026-09-02T19:40:44.750Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points[0].at, 1_788_378_044);
    }

    #[test]
    fn a_timestamp_in_a_form_this_does_not_read_is_rejected_by_name() {
        let track = parse(&document(
            r#"<trkpt lat="52.5" lon="13.4"><time>Wed, 02 Sep 2026</time></trkpt>"#,
        ))
        .unwrap();
        assert!(track.points.is_empty());
        assert!(track.rejected[0].reason.contains("Wed, 02 Sep 2026"));
    }

    #[test]
    fn a_file_that_is_not_gpx_says_so_rather_than_reading_as_empty() {
        let error = parse("<html><body>not a track</body></html>").unwrap_err();
        assert!(error.to_string().contains("Not a GPX file"), "got {error}");
    }

    #[test]
    fn an_empty_file_says_so_rather_than_reading_as_empty() {
        assert!(parse("").is_err());
    }

    #[test]
    fn a_truncated_document_reports_the_line_it_gave_up_on() {
        let text = "<gpx creator=\"Phone\">\n<trk><trkseg>\n<trkpt lat=\"52.5\" lon=\"13.4\"";
        let error = parse(text).unwrap_err().to_string();
        assert!(error.contains("line 3"), "got {error}");
    }

    #[test]
    fn a_point_that_is_never_closed_reports_the_line_it_opened_on() {
        let text = "<gpx creator=\"Phone\">\n<trkpt lat=\"52.5\" lon=\"13.4\">\n<ele>1</ele>\n";
        let error = parse(text).unwrap_err().to_string();
        assert!(error.contains("<trkpt> is never closed"), "got {error}");
    }

    #[test]
    fn the_bounds_are_the_corners_of_the_box_the_track_sits_in() {
        let track = parse(&document(
            r#"<trkpt lat="52.1" lon="13.4"><time>2026-09-02T10:00:00Z</time></trkpt>
               <trkpt lat="52.3" lon="13.2"><time>2026-09-02T11:00:00Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.bounds(), Some((52.1, 13.2, 52.3, 13.4)));
    }

    #[test]
    fn an_attribute_whose_name_ends_in_lat_is_not_a_latitude() {
        let track = parse(&document(
            r#"<trkpt xlat="1" lat="52.5" lon="13.4"><time>2026-09-02T19:40:44Z</time></trkpt>"#,
        ))
        .unwrap();
        assert_eq!(track.points[0].lat, 52.5);
    }
}
