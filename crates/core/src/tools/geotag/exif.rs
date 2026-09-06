//! A fix in the form EXIF holds it.
//!
//! **One rendering, used twice.** What the import preview shows and what the
//! writer sends to `exiftool` come out of this same function, so the row you
//! looked at before committing is the row that gets written. A screen that
//! renders a coordinate its own way is a screen that can agree with the file
//! while the file disagrees with the library.
//!
//! What is *stored* is the numeric form (`TrackPoint`); this is computed on
//! demand. Comparing two fixes, measuring the distance between them and
//! deciding whether they are the same reading are all arithmetic, and a
//! degrees-minutes-seconds string has to be parsed back into a number for any
//! of it — coming back not quite the one that went in. One form in the
//! database, and no chance of the two drifting apart.

use super::TrackPoint;
use chrono::DateTime;
use serde::{Deserialize, Serialize};

/// Decimal places kept when a coordinate is rendered.
///
/// A ten-millionth of a degree is about a centimetre — an order of magnitude
/// finer than any consumer fix, and finer than the tolerance that decides
/// whether two readings are the same one, so a value cannot survive a render
/// and come back a different reading.
const COORDINATE_PLACES: usize = 7;

/// One fix, as EXIF's tags.
///
/// Coordinates are unsigned with a separate hemisphere, because that is how the
/// tags are defined: a signed value with a mismatched ref is a coordinate in
/// the wrong half of the world, which is the kind of error that looks fine
/// until a map is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExifPoint {
    pub latitude: String,
    /// `N` or `S`.
    pub latitude_ref: String,
    pub longitude: String,
    /// `E` or `W`.
    pub longitude_ref: String,
    pub altitude: Option<String>,
    /// `0` above sea level, `1` below. Present exactly when `altitude` is.
    pub altitude_ref: Option<u8>,
    /// `YYYY:MM:DD`, UTC.
    pub date_stamp: String,
    /// `HH:MM:SS`, UTC.
    pub time_stamp: String,
}

/// Render a fix into EXIF's form.
///
/// `include_altitude` is the switch behind the tool's "write altitude" option:
/// a phone writes a literal zero when its altitude fix drops out, and somebody
/// who knows that about their own track should be able to leave the tag off
/// rather than have a number invented for them.
pub fn render(point: &TrackPoint, include_altitude: bool) -> ExifPoint {
    let (altitude, altitude_ref) = match point.ele.filter(|_| include_altitude) {
        // Below sea level is `1`, and the tag itself stays unsigned — the same
        // rule as the hemispheres, for the same reason.
        Some(ele) => (Some(format!("{:.3}", ele.abs())), Some(u8::from(ele < 0.0))),
        None => (None, None),
    };

    // GPS timestamps are UTC. The camera's local time is what makes the join
    // hard in the first place, and writing it here would put the lie in the
    // file rather than leaving it in the camera.
    let utc = DateTime::from_timestamp(point.at, 0);

    ExifPoint {
        latitude: format!("{:.*}", COORDINATE_PLACES, point.lat.abs()),
        latitude_ref: if point.lat < 0.0 { "S" } else { "N" }.into(),
        longitude: format!("{:.*}", COORDINATE_PLACES, point.lon.abs()),
        longitude_ref: if point.lon < 0.0 { "W" } else { "E" }.into(),
        altitude,
        altitude_ref,
        date_stamp: utc
            .map(|dt| dt.format("%Y:%m:%d").to_string())
            .unwrap_or_default(),
        time_stamp: utc
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_default(),
    }
}

impl ExifPoint {
    /// The arguments that write this fix, for one `-execute` of the persistent
    /// driver (G4).
    ///
    /// `GPSMapDatum` is stated rather than assumed: readers default to WGS-84,
    /// but a file that says which datum it means can be read in fifty years by
    /// something that does not share the default.
    pub fn args(&self) -> Vec<String> {
        let mut args = vec![
            format!("-GPSLatitude={}", self.latitude),
            format!("-GPSLatitudeRef={}", self.latitude_ref),
            format!("-GPSLongitude={}", self.longitude),
            format!("-GPSLongitudeRef={}", self.longitude_ref),
            format!("-GPSDateStamp={}", self.date_stamp),
            format!("-GPSTimeStamp={}", self.time_stamp),
            "-GPSMapDatum=WGS-84".to_string(),
        ];
        if let (Some(altitude), Some(reference)) = (&self.altitude, self.altitude_ref) {
            args.push(format!("-GPSAltitude={altitude}"));
            args.push(format!("-GPSAltitudeRef={reference}"));
        }
        args
    }

    /// The coordinate as one line, for a table a person reads.
    pub fn coordinate(&self) -> String {
        format!(
            "{} {} {} {}",
            self.latitude, self.latitude_ref, self.longitude, self.longitude_ref
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(lat: f64, lon: f64, ele: Option<f64>) -> TrackPoint {
        TrackPoint {
            // 2026-09-04T15:33:37Z, one of the sample track's fixes.
            at: 1_788_536_017,
            lat,
            lon,
            ele,
        }
    }

    #[test]
    fn a_berlin_fix_renders_north_and_east() {
        let rendered = render(&fix(52.531549, 13.369192, Some(36.40)), true);
        assert_eq!(rendered.latitude, "52.5315490");
        assert_eq!(rendered.latitude_ref, "N");
        assert_eq!(rendered.longitude, "13.3691920");
        assert_eq!(rendered.longitude_ref, "E");
    }

    #[test]
    fn a_southern_western_fix_renders_unsigned_with_the_other_hemispheres() {
        let rendered = render(&fix(-33.8688, -151.2093, None), true);
        assert_eq!(rendered.latitude, "33.8688000");
        assert_eq!(rendered.latitude_ref, "S");
        assert_eq!(rendered.longitude, "151.2093000");
        assert_eq!(rendered.longitude_ref, "W");
    }

    #[test]
    fn a_coordinate_survives_the_rendering_to_within_a_centimetre() {
        // The claim the whole design rests on: what goes into the file is what
        // came out of the track, to a precision finer than the tolerance that
        // decides whether two readings are the same reading.
        for (lat, lon) in [
            (52.531549, 13.369192),
            (-33.868800, 151.209300),
            (0.000001, -0.000001),
            (52.5315491234, 13.3691921234),
        ] {
            let rendered = render(&fix(lat, lon, None), true);
            let back_lat: f64 = rendered.latitude.parse().unwrap();
            let back_lon: f64 = rendered.longitude.parse().unwrap();
            let signed_lat = if rendered.latitude_ref == "S" {
                -back_lat
            } else {
                back_lat
            };
            let signed_lon = if rendered.longitude_ref == "W" {
                -back_lon
            } else {
                back_lon
            };
            assert!((signed_lat - lat).abs() < super::super::SAME_POSITION_DEGREES);
            assert!((signed_lon - lon).abs() < super::super::SAME_POSITION_DEGREES);
        }
    }

    #[test]
    fn an_altitude_above_sea_level_takes_reference_zero() {
        let rendered = render(&fix(52.5, 13.4, Some(36.40)), true);
        assert_eq!(rendered.altitude.as_deref(), Some("36.400"));
        assert_eq!(rendered.altitude_ref, Some(0));
    }

    #[test]
    fn an_altitude_below_sea_level_stays_unsigned_and_takes_reference_one() {
        let rendered = render(&fix(31.5, 35.5, Some(-420.0)), true);
        assert_eq!(rendered.altitude.as_deref(), Some("420.000"));
        assert_eq!(rendered.altitude_ref, Some(1));
    }

    #[test]
    fn a_recorded_zero_altitude_is_written_as_a_zero_altitude() {
        // The sample track holds one of these. It is a dropout, and it is also
        // what the phone said; correcting it here would be inventing data.
        let rendered = render(&fix(52.5, 13.4, Some(0.0)), true);
        assert_eq!(rendered.altitude.as_deref(), Some("0.000"));
        assert_eq!(rendered.altitude_ref, Some(0));
    }

    #[test]
    fn an_absent_altitude_writes_no_altitude_tag() {
        let rendered = render(&fix(52.5, 13.4, None), true);
        assert_eq!(rendered.altitude, None);
        assert!(!rendered.args().iter().any(|a| a.contains("GPSAltitude")));
    }

    #[test]
    fn altitude_can_be_left_out_of_a_point_that_has_one() {
        let rendered = render(&fix(52.5, 13.4, Some(36.40)), false);
        assert_eq!(rendered.altitude, None);
        assert!(!rendered.args().iter().any(|a| a.contains("GPSAltitude")));
    }

    #[test]
    fn the_stamps_are_the_fixs_utc_not_a_local_time() {
        let rendered = render(&fix(52.5, 13.4, None), true);
        assert_eq!(rendered.date_stamp, "2026:09:04");
        assert_eq!(rendered.time_stamp, "15:33:37");
    }

    #[test]
    fn the_arguments_carry_every_tag_the_fix_has() {
        let args = render(&fix(52.531549, 13.369192, Some(36.40)), true).args();
        for expected in [
            "-GPSLatitude=52.5315490",
            "-GPSLatitudeRef=N",
            "-GPSLongitude=13.3691920",
            "-GPSLongitudeRef=E",
            "-GPSDateStamp=2026:09:04",
            "-GPSTimeStamp=15:33:37",
            "-GPSMapDatum=WGS-84",
            "-GPSAltitude=36.400",
            "-GPSAltitudeRef=0",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "expected {expected:?} among {args:?}"
            );
        }
    }
}
