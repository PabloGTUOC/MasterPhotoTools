//! Geotagging — joining photographs to the places they were taken.
//!
//! **Beyond the specification.** `SPECIFICATION.md` mentions neither GPS nor
//! GPX, so this carries no F-number: inventing one would make the code look as
//! though the specification asked for something it never did. The reasoning,
//! the schema and the steps are in `docs/geotag-plan.md`, and the deviation is
//! recorded in `docs/known-gaps.md` (G9, G11).

pub mod exif;
pub mod gpx;
pub mod join;
pub mod library;
pub mod scan;
pub mod tool;

pub use tool::preview;

use crate::ledger::TrackRow;
use serde::{Deserialize, Serialize};

/// One imported track, as a screen sees it.
///
/// `ledger::TrackRow` is the database's shape and stays there; this is the
/// wire's. The two are separate so a column can be added to one without
/// changing what every client is expecting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackSummary {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub creator: Option<String>,
    pub imported_at: i64,
    pub point_count: i64,
    pub points_added: i64,
    pub points_identical: i64,
    pub points_conflicting: i64,
    pub first_fix: Option<i64>,
    pub last_fix: Option<i64>,
    pub bounds: Option<Bounds>,
}

/// The box a track sits in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

/// One recorded disagreement, as a screen sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedConflict {
    pub at: i64,
    pub kept: TrackPoint,
    pub other: TrackPoint,
    pub metres: f64,
    /// `kept-existing` or `took-new`.
    pub decision: String,
}

impl From<crate::ledger::TrackConflictRecord> for RecordedConflict {
    fn from(row: crate::ledger::TrackConflictRecord) -> Self {
        Self {
            at: row.at,
            kept: row.kept,
            other: row.other,
            metres: row.metres,
            decision: row.decision,
        }
    }
}

impl From<TrackRow> for TrackSummary {
    fn from(row: TrackRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            source_path: row.source_path,
            creator: row.creator,
            imported_at: row.imported_at,
            point_count: row.point_count,
            points_added: row.points_added,
            points_identical: row.points_identical,
            points_conflicting: row.points_conflicting,
            first_fix: row.first_fix,
            last_fix: row.last_fix,
            bounds: row
                .bounds
                .map(|(min_lat, min_lon, max_lat, max_lon)| Bounds {
                    min_lat,
                    min_lon,
                    max_lat,
                    max_lon,
                }),
        }
    }
}

/// One fix: where the phone was, and when.
///
/// The canonical form is numeric — Unix seconds UTC and decimal degrees —
/// because everything done with a fix is arithmetic: how far apart two of them
/// are, which one is nearer in time, whether two readings are the same reading.
/// A degrees-minutes-seconds string has to be parsed back into a number for any
/// of that, and comes back not quite the one that went in. The EXIF rendering
/// is a function of this, computed for display and again for the write, so the
/// two forms cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackPoint {
    /// Unix seconds, UTC. GPX timestamps are UTC by definition; EXIF capture
    /// times are not, which is the whole difficulty of this feature.
    pub at: i64,
    pub lat: f64,
    pub lon: f64,
    /// Metres above sea level. Absent where the point carried no `<ele>` —
    /// which is not the same as zero, and a phone does write a literal zero
    /// when its altitude fix drops out.
    pub ele: Option<f64>,
}

/// How close two readings must be to count as **the same reading**.
///
/// A millionth of a degree is about 11 cm. Two exports of one fix from one
/// phone are usually byte-identical; this tolerance is for an export that
/// rounded differently on its way out. Anything outside it is a disagreement
/// and is put to the user rather than averaged, ignored, or resolved by
/// whichever file was imported last.
pub const SAME_POSITION_DEGREES: f64 = 1e-6;

/// How close two elevations must be to count as the same, in metres.
pub const SAME_ELEVATION_METRES: f64 = 0.5;

/// True where two fixes are the same reading to within the tolerances above.
///
/// An elevation present on one side and absent on the other is a difference:
/// "no altitude was recorded" and "the altitude was 36 m" are different claims,
/// and collapsing them would lose the one that is true.
pub fn same_position(a: &TrackPoint, b: &TrackPoint) -> bool {
    if (a.lat - b.lat).abs() > SAME_POSITION_DEGREES
        || (a.lon - b.lon).abs() > SAME_POSITION_DEGREES
    {
        return false;
    }
    match (a.ele, b.ele) {
        (Some(x), Some(y)) => (x - y).abs() <= SAME_ELEVATION_METRES,
        (None, None) => true,
        _ => false,
    }
}

/// The mean radius of the earth, in metres (IUGG).
const EARTH_RADIUS_METRES: f64 = 6_371_008.8;

/// How far apart two fixes are, over the ground, in metres.
///
/// This number is what makes a conflict readable: three metres is two apps
/// disagreeing about one fix, two kilometres is a different device or an export
/// with the wrong offset. Haversine rather than a flat approximation — the
/// approximation is fine at these distances and wrong by kilometres at the ones
/// that matter most, which are exactly the ones worth telling the truth about.
pub fn metres_between(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let d_lat = lat2 - lat1;
    let d_lon = (b.lon - a.lon).to_radians();

    let h = (d_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_METRES * h.sqrt().clamp(-1.0, 1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(lat: f64, lon: f64, ele: Option<f64>) -> TrackPoint {
        TrackPoint {
            at: 0,
            lat,
            lon,
            ele,
        }
    }

    #[test]
    fn a_reading_repeated_to_the_last_digit_is_the_same_reading() {
        let a = point(52.531549, 13.369192, Some(36.40));
        assert!(same_position(&a, &a));
    }

    #[test]
    fn a_reading_rounded_in_the_seventh_decimal_is_the_same_reading() {
        // 1e-7 of a degree is a centimetre. No two exports of one fix disagree
        // by less than that for any reason worth asking a human about.
        let a = point(52.531549, 13.369192, Some(36.40));
        let b = point(52.5315491, 13.3691921, Some(36.42));
        assert!(same_position(&a, &b));
    }

    #[test]
    fn a_reading_ten_metres_away_is_a_different_reading() {
        let a = point(52.531549, 13.369192, Some(36.40));
        let b = point(52.531639, 13.369192, Some(36.40));
        assert!(!same_position(&a, &b));
    }

    #[test]
    fn an_altitude_recorded_on_one_side_only_is_a_difference() {
        let a = point(52.531549, 13.369192, Some(36.40));
        let b = point(52.531549, 13.369192, None);
        assert!(!same_position(&a, &b));
    }

    #[test]
    fn the_distance_between_two_fixes_is_measured_over_the_ground() {
        // Two of the sample track's points, 26 minutes apart in a Berlin street.
        let a = point(52.531549, 13.369192, None);
        let b = point(52.531469, 13.369442, None);
        let metres = metres_between(&a, &b);
        assert!(
            (18.0..22.0).contains(&metres),
            "expected about 20 m, got {metres}"
        );
    }

    #[test]
    fn a_fix_is_no_distance_from_itself() {
        let a = point(52.531549, 13.369192, None);
        assert_eq!(metres_between(&a, &a), 0.0);
    }

    #[test]
    fn a_degree_of_latitude_is_about_a_hundred_and_eleven_kilometres() {
        let a = point(52.0, 13.0, None);
        let b = point(53.0, 13.0, None);
        let km = metres_between(&a, &b) / 1000.0;
        assert!((111.0..112.0).contains(&km), "expected ~111 km, got {km}");
    }
}
