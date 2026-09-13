//! Points placed by hand, on a map, for the days nothing recorded.
//!
//! **Beyond the specification**, like the rest of `geotag`; the reasoning is in
//! `docs/timeline-plan.md`.
//!
//! A pin is not a fix. It is the photographer saying where they were, which is
//! a claim worth having and not an observation — so it enters the library
//! carrying [`PointSource::Placed`](super::PointSource::Placed), and every
//! screen that shows a position shows that word.
//!
//! It enters by exactly the road a `.gpx` does: the stops are written as GPX,
//! parsed back, and handed to `library::preview_import` and
//! `library::commit_import`. That is not ceremony. It means a pin meets the
//! same conflict rules as a file — an instant the library already holds is put
//! to the user rather than overwritten — it means the text is stored for
//! provenance like any other import, and it means the identity of a placement
//! is the hash of what it claims, so placing the same stop twice is one import
//! rather than two.

use super::gpx::{self, PlacedStop};
use super::library::TrackFile;
use super::PointSource;
use crate::error::Error;

/// Where a placed track says it came from, in the column that holds a path for
/// an imported file. Not a path, and deliberately not shaped like one.
pub const PLACED_SOURCE_PATH: &str = "(placed on the map)";

/// Turn stops into an importable track, or say why they are not one.
///
/// The name is what a person will see in the track library: the places
/// themselves, which is more use than a date they can read off the fixes.
pub fn build(stops: &[PlacedStop]) -> Result<TrackFile, Error> {
    let text = gpx::write_placed(stops)?;

    let mut names: Vec<&str> = stops.iter().map(|s| s.name.trim()).collect();
    names.retain(|n| !n.is_empty());
    names.dedup();
    let name = match names.len() {
        0 => "Placed by hand".to_string(),
        1..=2 => names.join(", "),
        _ => format!("{}, {} and {} more", names[0], names[1], names.len() - 2),
    };

    // The id is the hash of the text, computed by `from_text` as it is for
    // every other track: two placements that claim the same thing are the same
    // import, whatever order the stops arrived in — they are sorted by time on
    // the way into the document.
    TrackFile::from_text(&name, PLACED_SOURCE_PATH, PointSource::Placed, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(name: &str, from: i64, to: i64) -> PlacedStop {
        PlacedStop {
            name: name.into(),
            lat: 52.521918,
            lon: 13.413215,
            from,
            to,
        }
    }

    #[test]
    fn a_span_becomes_two_points_and_never_a_third() {
        let file = build(&[stop("Alexanderplatz", 1_367_416_800, 1_367_431_200)]).unwrap();
        let points = &file.parsed.points;
        assert_eq!(
            points.len(),
            2,
            "a span is its two ends and nothing between"
        );
        assert_eq!(points[0].at, 1_367_416_800);
        assert_eq!(points[1].at, 1_367_431_200);
        assert_eq!(points[0].lat, points[1].lat);
    }

    #[test]
    fn an_instant_becomes_one_point() {
        let file = build(&[stop("Alexanderplatz", 1_367_416_800, 1_367_416_800)]).unwrap();
        assert_eq!(file.parsed.points.len(), 1);
    }

    #[test]
    fn a_placed_track_says_it_was_placed_by_hand() {
        let file = build(&[stop("Alexanderplatz", 10, 20)]).unwrap();
        assert_eq!(file.source, PointSource::Placed);
        assert_eq!(file.parsed.creator.as_deref(), Some(gpx::PLACED_CREATOR));
    }

    #[test]
    fn no_altitude_is_written_because_none_was_known() {
        let file = build(&[stop("Alexanderplatz", 10, 20)]).unwrap();
        assert!(
            file.parsed.points.iter().all(|p| p.ele.is_none()),
            "an unknown altitude is absent, not zero"
        );
    }

    #[test]
    fn the_same_placement_twice_is_one_import() {
        let first = build(&[stop("Alexanderplatz", 10, 20)]).unwrap();
        let second = build(&[stop("Alexanderplatz", 10, 20)]).unwrap();
        assert_eq!(first.id, second.id);
    }

    #[test]
    fn stops_arriving_out_of_order_are_the_same_import() {
        let a = stop("Alexanderplatz", 10, 20);
        let b = stop("Museumsinsel", 100, 200);
        assert_eq!(
            build(&[a.clone(), b.clone()]).unwrap().id,
            build(&[b, a]).unwrap().id
        );
    }

    #[test]
    fn two_stops_claiming_one_instant_are_refused_before_a_file_exists() {
        let mut second = stop("Museumsinsel", 15, 30);
        second.lat = 52.5169;
        let error = build(&[stop("Alexanderplatz", 10, 20), second]).unwrap_err();
        assert!(
            error.to_string().contains("overlap in time"),
            "got: {error}"
        );
    }

    #[test]
    fn a_span_that_ends_before_it_starts_is_refused() {
        let error = build(&[stop("Alexanderplatz", 20, 10)]).unwrap_err();
        assert!(
            error.to_string().contains("ends before it starts"),
            "got: {error}"
        );
    }

    #[test]
    fn a_coordinate_off_the_planet_is_refused() {
        let mut impossible = stop("Nowhere", 10, 20);
        impossible.lat = 91.0;
        let error = build(&[impossible]).unwrap_err();
        assert!(
            error.to_string().contains("not a place on Earth"),
            "got: {error}"
        );
    }

    #[test]
    fn an_ampersand_in_a_place_name_survives_the_round_trip() {
        let mut named = stop("Bar & Grill", 10, 20);
        named.name = "Bar & Grill".into();
        let file = build(&[named]).unwrap();
        assert!(file.gpx.contains("Bar &amp; Grill"));
        assert_eq!(file.parsed.points.len(), 2, "and it still parses");
    }

    #[test]
    fn the_name_lists_the_places_rather_than_a_date() {
        let file = build(&[stop("Alexanderplatz", 10, 20), {
            let mut s = stop("Museumsinsel", 100, 200);
            s.lat = 52.5169;
            s
        }])
        .unwrap();
        assert_eq!(file.name, "Alexanderplatz, Museumsinsel");
    }
}
