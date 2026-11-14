//! DV-9: lens correction from the lensfun database.

#[allow(dead_code)]
mod support;

use std::collections::BTreeSet;

use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::media::film::Grain;
use rollchrome_core::media::lens::model::{
    apply, distortion_at, falloff, magnification, source_pixel, Correction, Distortion, Tca,
};
use rollchrome_core::media::lens::{db, profile_for, LensInfo};
use rollchrome_core::tools::develop::{
    execute_develop, plan_develop, DevelopItem, DevelopStore, NEUTRAL_LOOK,
};
use support::dng::DngBuilder;

fn info(make: &str, model: &str, lens: Option<&str>, focal: f32, f: f32) -> LensInfo {
    LensInfo {
        make: make.to_string(),
        model: model.to_string(),
        lens: lens.map(str::to_string),
        focal_mm: Some(focal),
        f_number: Some(f),
    }
}

// ---------------------------------------------------------------------------
// The database and matching
// ---------------------------------------------------------------------------

#[test]
fn the_shipped_database_parses() {
    let db = db::database().expect("the lens database parses");
    assert!(db.lenses.len() > 1000, "{} lenses", db.lenses.len());
    assert!(db.cameras.len() > 500, "{} cameras", db.cameras.len());
}

#[test]
fn an_olympus_xz1_matches_its_own_lens_by_camera() {
    let p = profile_for(&info("Olympus", "XZ-1", None, 6.0, 2.0), 3648, 2736).unwrap();
    assert!(
        p.lens_name.starts_with("XZ-1 & compatibles"),
        "{}",
        p.lens_name
    );
    assert!(p.correction.distortion.is_some());
    assert!((p.correction.radius_scale - 1.0).abs() < 1e-3);
}

#[test]
fn an_interchangeable_lens_matches_by_name() {
    let p = profile_for(
        &info("Fujifilm", "X-Pro2", Some("XF35mmF1.4 R"), 35.0, 5.6),
        6000,
        4000,
    )
    .unwrap();
    assert_eq!(p.lens_name, "XF35mmF1.4 R");
    assert!(p.correction.distortion.is_some() && p.correction.vignetting.is_some());
}

#[test]
fn an_unknown_camera_or_lens_corrects_nothing_and_says_why() {
    let err = profile_for(&info("Nobody", "Camera 9", None, 35.0, 4.0), 100, 100).unwrap_err();
    assert!(err.contains("not in the lens database"), "{err}");
    let err = profile_for(
        &info(
            "Fujifilm",
            "X-Pro2",
            Some("Imaginary 13mm f/0.7"),
            13.0,
            0.7,
        ),
        6000,
        4000,
    )
    .unwrap_err();
    assert!(err.contains("no lens profile"), "{err}");
}

// ---------------------------------------------------------------------------
// The models
// ---------------------------------------------------------------------------

#[test]
fn ptlens_holds_radius_one_and_follows_its_polynomial() {
    let d = Distortion::Ptlens {
        a: 0.01,
        b: -0.05,
        c: 0.0,
    };
    assert!((d.factor(1.0) - 1.0).abs() < 1e-6);
    let r: f32 = 0.5;
    let want = 0.01 * r.powi(3) - 0.05 * r * r + 1.0 - 0.01 + 0.05;
    assert!((d.factor(r) - want).abs() < 1e-6);
}

#[test]
fn distortion_is_interpolated_between_focal_lengths_and_held_outside() {
    let cals = [
        (
            10.0,
            Distortion::Ptlens {
                a: 0.0,
                b: -0.1,
                c: 0.0,
            },
        ),
        (
            20.0,
            Distortion::Ptlens {
                a: 0.0,
                b: -0.3,
                c: 0.0,
            },
        ),
    ];
    let Distortion::Ptlens { a, b, c } = distortion_at(&cals, 15.0).unwrap() else {
        panic!("interpolating two ptlens calibrations must give ptlens");
    };
    assert!(
        a.abs() < 1e-6 && (b + 0.2).abs() < 1e-6 && c.abs() < 1e-6,
        "{a} {b} {c}"
    );
    assert_eq!(distortion_at(&cals, 5.0).unwrap(), cals[0].1);
    assert_eq!(distortion_at(&cals, 50.0).unwrap(), cals[1].1);
}

#[test]
fn vignetting_is_held_past_the_calibrated_corner() {
    let k = [-0.3, 0.2, -0.1];
    assert_eq!(falloff(k, 1.4), falloff(k, 1.0));
    assert!(falloff(k, 0.5) < 1.0 && falloff(k, 0.0) == 1.0);
}

// ---------------------------------------------------------------------------
// The correction, on pictures made for it
// ---------------------------------------------------------------------------

const W: u32 = 300;
const H: u32 = 200;

fn none() -> Correction {
    Correction {
        distortion: None,
        tca: None,
        vignetting: None,
        radius_scale: 1.0,
        vignetting_scale: 1.0,
    }
}

/// Picture units (one is half the shorter side, about the centre) to pixel coordinates.
fn to_px((x, y): (f32, f32)) -> (f32, f32) {
    let unit = W.min(H) as f32 * 0.5;
    (
        x * unit + W as f32 * 0.5 - 0.5,
        y * unit + H as f32 * 0.5 - 0.5,
    )
}

/// A picture of small Gaussian dots, each channel's dots at its own positions.
fn dots(positions: &[[(f32, f32); 3]]) -> Vec<[f32; 3]> {
    let mut px = vec![[0.0f32; 3]; (W * H) as usize];
    for set in positions {
        for (ch, &(cx, cy)) in set.iter().enumerate() {
            for y in (cy as i32 - 6).max(0)..(cy as i32 + 7).min(H as i32) {
                for x in (cx as i32 - 6).max(0)..(cx as i32 + 7).min(W as i32) {
                    let d2 = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
                    px[(y as u32 * W + x as u32) as usize][ch] += (-d2 / (2.0 * 1.3 * 1.3)).exp();
                }
            }
        }
    }
    px
}

/// The weighted centre of one channel near a pixel position.
fn centroid(px: &[[f32; 3]], (cx, cy): (f32, f32), ch: usize) -> (f32, f32) {
    let (mut sx, mut sy, mut s) = (0.0, 0.0, 0.0);
    for y in (cy as i32 - 5).max(0)..(cy as i32 + 6).min(H as i32) {
        for x in (cx as i32 - 5).max(0)..(cx as i32 + 6).min(W as i32) {
            let v = px[(y as u32 * W + x as u32) as usize][ch];
            sx += v * x as f32;
            sy += v * y as f32;
            s += v;
        }
    }
    (sx / s, sy / s)
}

/// Distance of each point from the straight line through the first and last.
fn bow(points: &[(f32, f32)]) -> f32 {
    let (a, b) = (points[0], points[points.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    points
        .iter()
        .map(|p| ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len)
        .fold(0.0, f32::max)
}

#[test]
fn a_barrel_distorted_line_comes_back_straight() {
    let c = Correction {
        distortion: Some(Distortion::Ptlens {
            a: 0.0,
            b: -0.06,
            c: 0.0,
        }),
        ..none()
    };
    let k = magnification(&c, W, H);
    // A straight scene line near the top edge, as the corrected picture will show it.
    let line: Vec<(f32, f32)> = (0..9).map(|i| (-1.2 + 0.3 * i as f32, -0.75)).collect();
    // Where the lens put each point: the source position of the output pixel showing it.
    let distorted: Vec<[(f32, f32); 3]> = line
        .iter()
        .map(|&u| {
            let o = to_px((u.0 / k, u.1 / k));
            let s = source_pixel(&c, W, H, k, o.0, o.1, 1);
            [s, s, s]
        })
        .collect();
    let source = dots(&distorted);
    let before = bow(&distorted.iter().map(|d| d[1]).collect::<Vec<_>>());
    assert!(
        before > 2.0,
        "the test line must start visibly bent, bowed {before} px"
    );

    let (out, scale) = apply(W, H, &source, &c);
    assert_eq!(scale, k);
    let found: Vec<(f32, f32)> = line
        .iter()
        .map(|&u| centroid(&out, to_px((u.0 / k, u.1 / k)), 1))
        .collect();
    let after = bow(&found);
    assert!(after < 0.5, "corrected line bows {after} px (was {before})");
}

#[test]
fn a_vignetted_flat_field_comes_back_flat() {
    let k = [-0.4, 0.1, -0.05];
    let c = Correction {
        vignetting: Some(k),
        ..none()
    };
    let unit = W.min(H) as f32 * 0.5;
    let half_diag = ((W * W + H * H) as f32).sqrt() * 0.5 / unit;
    let mut source = vec![[0.0f32; 3]; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            let ux = (x as f32 + 0.5) / unit - W as f32 * 0.5 / unit;
            let uy = (y as f32 + 0.5) / unit - H as f32 * 0.5 / unit;
            let r = (ux * ux + uy * uy).sqrt() / half_diag;
            source[(y * W + x) as usize] = [0.5 * falloff(k, r); 3];
        }
    }
    let (out, scale) = apply(W, H, &source, &c);
    assert_eq!(scale, 1.0, "vignetting alone moves nothing");
    let worst = out
        .iter()
        .map(|p| ((p[1] - 0.5) / 0.5).abs())
        .fold(0.0, f32::max);
    assert!(worst < 0.01, "flat field off by {:.2}%", worst * 100.0);
}

#[test]
fn red_and_blue_land_back_on_green() {
    let c = Correction {
        tca: Some(Tca {
            red: [1.01, 0.0, 0.0],
            blue: [0.99, 0.0, 0.0],
        }),
        ..none()
    };
    let k = magnification(&c, W, H);
    let points = [(-1.1, -0.6), (1.1, 0.6), (0.9, -0.7), (-0.5, 0.75)];
    let shifted: Vec<[(f32, f32); 3]> = points
        .iter()
        .map(|&u| {
            let o = to_px((u.0 / k, u.1 / k));
            [
                source_pixel(&c, W, H, k, o.0, o.1, 0),
                source_pixel(&c, W, H, k, o.0, o.1, 1),
                source_pixel(&c, W, H, k, o.0, o.1, 2),
            ]
        })
        .collect();
    let spread_before = shifted
        .iter()
        .map(|s| ((s[0].0 - s[2].0).powi(2) + (s[0].1 - s[2].1).powi(2)).sqrt())
        .fold(0.0, f32::max);
    assert!(
        spread_before > 1.5,
        "the test needs a visible fringe, got {spread_before} px"
    );

    let (out, _) = apply(W, H, &dots(&shifted), &c);
    for &u in &points {
        let at = to_px((u.0 / k, u.1 / k));
        let g = centroid(&out, at, 1);
        for ch in [0, 2] {
            let p = centroid(&out, at, ch);
            let d = ((p.0 - g.0).powi(2) + (p.1 - g.1).powi(2)).sqrt();
            assert!(d < 0.3, "channel {ch} is {d} px off green at {u:?}");
        }
    }
}

#[test]
fn after_the_fill_scale_no_output_pixel_samples_outside_the_source() {
    for c in [
        Correction {
            distortion: Some(Distortion::Ptlens {
                a: 0.0,
                b: -0.08,
                c: 0.0,
            }),
            ..none()
        },
        Correction {
            distortion: Some(Distortion::Ptlens {
                a: 0.0,
                b: 0.06,
                c: 0.0,
            }),
            tca: Some(Tca {
                red: [1.004, 0.0, 0.0],
                blue: [0.996, 0.0, 0.0],
            }),
            ..none()
        },
    ] {
        let k = magnification(&c, W, H);
        let edge = |x: f32, y: f32| {
            (0..3).all(|ch| {
                let (sx, sy) = source_pixel(&c, W, H, k, x, y, ch);
                (-0.51..=W as f32 - 0.49).contains(&sx) && (-0.51..=H as f32 - 0.49).contains(&sy)
            })
        };
        for x in 0..W {
            assert!(
                edge(x as f32, 0.0) && edge(x as f32, (H - 1) as f32),
                "{c:?} at x {x}"
            );
        }
        for y in 0..H {
            assert!(
                edge(0.0, y as f32) && edge((W - 1) as f32, y as f32),
                "{c:?} at y {y}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// In Develop
// ---------------------------------------------------------------------------

struct NoopProgress;
impl phototools_core::jobs::Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

#[derive(Default)]
struct MemStore {
    raw: Vec<u8>,
    written: Vec<String>,
}

impl DevelopStore for MemStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, phototools_core::error::Error> {
        Ok(BTreeSet::new())
    }
    fn read(&self, _name: &str) -> Result<Vec<u8>, phototools_core::error::Error> {
        Ok(self.raw.clone())
    }
    fn look(
        &self,
        look: &str,
    ) -> Result<rollchrome_core::media::develop::Look, phototools_core::error::Error> {
        Err(phototools_core::error::Error::Refused(look.to_string()))
    }
    fn create_new(
        &mut self,
        output: &str,
        _bytes: &[u8],
    ) -> Result<(), phototools_core::error::Error> {
        self.written.push(output.to_string());
        Ok(())
    }
    fn carry_metadata(
        &mut self,
        _s: &str,
        _o: &str,
        _w: u32,
        _h: u32,
        _f: OutputFormat,
    ) -> Result<(), phototools_core::error::Error> {
        Ok(())
    }
}

fn item(lens: bool) -> DevelopItem {
    DevelopItem {
        name: "a.dng".to_string(),
        look: Some(NEUTRAL_LOOK.to_string()),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
        lens_correction: lens,
        capture_sharpening: 0.0,
    }
}

#[test]
fn a_plan_without_lens_correction_serialises_as_before() {
    let plan = plan_develop(&[item(false)], OutputFormat::Tiff, &BTreeSet::new());
    let json = serde_json::to_string(&plan).unwrap();
    assert!(!json.contains("lens_correction"), "{json}");
    let with = plan_develop(&[item(true)], OutputFormat::Tiff, &BTreeSet::new());
    assert!(serde_json::to_string(&with)
        .unwrap()
        .contains("\"lens_correction\":true"));
}

#[test]
fn a_photograph_with_no_profile_develops_uncorrected_and_says_why() {
    let raw = DngBuilder::new_bayer(48, 32).build(&vec![3000u16; 48 * 32]);
    let mut store = MemStore {
        raw,
        ..Default::default()
    };
    let plan = plan_develop(&[item(true)], OutputFormat::Tiff, &BTreeSet::new());
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["a.tif"]);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.lens_skipped.len(), 1);
    assert!(
        summary.lens_skipped[0]
            .reason
            .starts_with("developed without lens correction:"),
        "{}",
        summary.lens_skipped[0].reason
    );
}

#[test]
fn an_item_without_the_field_does_not_correct() {
    let item: DevelopItem = serde_json::from_value(serde_json::json!({
        "name": "a.dng", "look": "neutral", "exposure_ev": 0.0
    }))
    .unwrap();
    assert!(!item.lens_correction);
}
