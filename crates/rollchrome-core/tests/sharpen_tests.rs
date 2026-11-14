//! DV-10: capture sharpening.

#[allow(dead_code)]
mod support;

use std::collections::BTreeSet;

use rollchrome_core::media::develop::sharpen::{capture_sharpen, sigma_for};
use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::media::film::Grain;
use rollchrome_core::tools::develop::{
    execute_develop, plan_develop, DevelopItem, DevelopStore, NEUTRAL_LOOK,
};
use support::dng::DngBuilder;

const W: u32 = 64;
const H: u32 = 32;

/// A vertical edge from `lo` to `hi`, blurred by a Gaussian of `sigma` across it.
fn blurred_edge(lo: f32, hi: f32, sigma: f32) -> Vec<[f32; 3]> {
    let erf = |x: f32| {
        // Abramowitz–Stegun 7.1.26, ample for a test.
        let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
        let y = 1.0
            - (((((1.061_405_4 * t - 1.453_152_1) * t) + 1.421_413_7) * t - 0.284_496_7) * t
                + 0.254_829_6)
                * t
                * (-x * x).exp();
        if x >= 0.0 {
            y
        } else {
            -y
        }
    };
    let mut px = Vec::with_capacity((W * H) as usize);
    for _ in 0..H {
        for x in 0..W {
            let d = x as f32 + 0.5 - W as f32 / 2.0;
            let v = lo + (hi - lo) * 0.5 * (1.0 + erf(d / (sigma * 2f32.sqrt())));
            px.push([v, v, v]);
        }
    }
    px
}

/// The 10–90 % rise of the middle row, in pixels.
fn rise(px: &[[f32; 3]], lo: f32, hi: f32) -> f32 {
    let row: Vec<f32> = (0..W).map(|x| px[((H / 2) * W + x) as usize][1]).collect();
    let at = |level: f32| {
        let target = lo + (hi - lo) * level;
        let i = row.iter().position(|v| *v >= target).unwrap();
        let (a, b) = (row[i - 1], row[i]);
        (i - 1) as f32 + (target - a) / (b - a)
    };
    at(0.9) - at(0.1)
}

#[test]
fn a_blurred_edge_comes_back_narrower_without_overshoot() {
    let (lo, hi) = (0.1, 0.6);
    let source = blurred_edge(lo, hi, 0.7);
    let out = capture_sharpen(W, H, &source, 100.0, sigma_for(false));

    let before = rise(&source, lo, hi);
    let after = rise(&out, lo, hi);
    println!("10-90% rise: {before:.3} px blurred, {after:.3} px sharpened");
    assert!(
        after <= before * 0.75,
        "10-90% rise {before:.3} px became {after:.3} px, under 25% narrower"
    );
    let (min, max) = out
        .iter()
        .map(|p| p[1])
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    assert!(
        min >= lo - 1e-4 && max <= hi + 1e-4,
        "overshoot: {min}..{max}"
    );
}

#[test]
fn a_flat_noisy_field_is_not_sharpened() {
    // Mild noise on a flat grey, as noise reduction leaves it: a deterministic pseudo-random field.
    let mut seed = 12345u32;
    let source: Vec<[f32; 3]> = (0..W * H)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
            let v = 0.18 + n * 0.0015;
            [v, v, v]
        })
        .collect();
    let std = |px: &[[f32; 3]]| {
        let mean = px.iter().map(|p| p[1]).sum::<f32>() / px.len() as f32;
        (px.iter().map(|p| (p[1] - mean).powi(2)).sum::<f32>() / px.len() as f32).sqrt()
    };
    let out = capture_sharpen(W, H, &source, 100.0, sigma_for(false));
    let (before, after) = (std(&source), std(&out));
    assert!(
        after <= before * 1.05,
        "flat-field noise rose from {before} to {after}"
    );
}

#[test]
fn strength_zero_changes_nothing_and_colour_ratios_keep() {
    let mut source = blurred_edge(0.1, 0.6, 0.7);
    for p in source.iter_mut() {
        *p = [p[0] * 1.3, p[1], p[2] * 0.6];
    }
    assert_eq!(capture_sharpen(W, H, &source, 0.0, 0.7), source);

    let out = capture_sharpen(W, H, &source, 100.0, 0.7);
    for (a, b) in source.iter().zip(&out) {
        if b[1] > 1e-4 {
            assert!((b[0] / b[1] - a[0] / a[1]).abs() < 1e-4);
            assert!((b[2] / b[1] - a[2] / a[1]).abs() < 1e-4);
        }
    }
}

fn item(sharpen: f32) -> DevelopItem {
    DevelopItem {
        name: "a.dng".to_string(),
        look: Some(NEUTRAL_LOOK.to_string()),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
        lens_correction: false,
        capture_sharpening: sharpen,
    }
}

#[test]
fn a_plan_without_sharpening_serialises_as_before_and_a_bad_value_is_skipped() {
    let plan = plan_develop(&[item(0.0)], OutputFormat::Tiff, &BTreeSet::new());
    assert!(!serde_json::to_string(&plan)
        .unwrap()
        .contains("capture_sharpening"));

    let plan = plan_develop(&[item(150.0)], OutputFormat::Tiff, &BTreeSet::new());
    assert!(plan.actions.is_empty());
    assert_eq!(
        plan.skipped[0].reason,
        "capture sharpening must be between 0 and 100"
    );
}

#[derive(Default)]
struct MemStore {
    raw: Vec<u8>,
    written: Vec<Vec<u8>>,
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
        _output: &str,
        bytes: &[u8],
    ) -> Result<(), phototools_core::error::Error> {
        self.written.push(bytes.to_vec());
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

struct NoopProgress;
impl phototools_core::jobs::Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

#[test]
fn develop_sharpens_when_asked() {
    // A frame with structure: stripes, so there is detail for the mask to let through.
    let (w, h) = (48u32, 32u32);
    let pixels: Vec<u16> = (0..w * h)
        .map(|i| if (i % w) / 4 % 2 == 0 { 1200 } else { 9000 })
        .collect();
    let raw = DngBuilder::new_bayer(w, h).build(&pixels);
    let run = |sharpen: f32| {
        let mut store = MemStore {
            raw: raw.clone(),
            ..Default::default()
        };
        let plan = plan_develop(&[item(sharpen)], OutputFormat::Tiff, &BTreeSet::new());
        let summary = execute_develop(plan, &mut store, &NoopProgress);
        assert!(summary.failures.is_empty(), "{:?}", summary.failures);
        store.written.remove(0)
    };
    assert_ne!(run(0.0), run(100.0), "sharpening must change the output");
}
