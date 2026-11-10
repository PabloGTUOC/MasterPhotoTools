//! Tests for built-in looks, colorimetry, fitting, and registry (DV-3).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use phototools_core::error::Error;
use phototools_core::jobs::Progress;
use rollchrome_core::media::develop::render::{render, Look, LookSpace, OutputFormat};
use rollchrome_core::media::develop::tone;
use rollchrome_core::media::develop::{Demosaic, Developed};
use rollchrome_core::media::edit::lut::Lut;
use rollchrome_core::media::looks::builtin::{builtin_look, builtin_looks};
use rollchrome_core::media::looks::colour as looks_colour;
use rollchrome_core::media::looks::fit::{
    bake_cube_33, fit_look_model, measure_patch_box, HermiteCurve, LookModel, KNOTS_K,
};
use rollchrome_core::tools::develop::{execute_develop, DevelopAction, DevelopPlan, DevelopStore};

/// 1. Sharma, Wu and Dalal (2005) published CIEDE2000 test pairs.
#[test]
fn ciede2000_matches_the_published_pairs() {
    let p1_1 = [50.0, 2.6772, -79.7751];
    let p1_2 = [50.0, 0.0, -82.7485];
    let de1 = looks_colour::ciede2000(p1_1, p1_2);
    assert!(
        (de1 - 2.0425).abs() <= 0.0001,
        "Pair 1: expected 2.0425, got {de1}"
    );

    let p2_1 = [50.0, 0.0, 0.0];
    let p2_2 = [50.0, -1.0, 2.0];
    let de2 = looks_colour::ciede2000(p2_1, p2_2);
    assert!(
        (de2 - 2.3669).abs() <= 0.0001,
        "Pair 2: expected 2.3669, got {de2}"
    );

    let p3_1 = [50.0, 2.5, 0.0];
    let p3_2 = [73.0, 25.0, -18.0];
    let de3 = looks_colour::ciede2000(p3_1, p3_2);
    assert!(
        (de3 - 27.1492).abs() <= 0.0001,
        "Pair 3: expected 27.1492, got {de3}"
    );
}

/// 2. 1000 seeded random linear ProPhoto values in 0..1 to Lab D50 and back within 1e-5.
#[test]
fn lab_d50_round_trips_through_prophoto() {
    let mut state = 123456789u64;
    let mut next_f32 = || -> f32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 32) as u32 as f32) / (u32::MAX as f32)
    };

    for _ in 0..1000 {
        let pro = [next_f32(), next_f32(), next_f32()];
        let lab = looks_colour::prophoto_linear_to_lab_d50(pro);
        let rec = looks_colour::lab_d50_to_prophoto_linear(lab);

        for c in 0..3 {
            assert!(
                (pro[c] - rec[c]).abs() <= 1e-5,
                "Round trip error ch {c}: orig {}, rec {}",
                pro[c],
                rec[c]
            );
        }
    }
}

/// 3. The fit recovers a known look from 24 synthetic patch pairs within mean ΔE2000 0.2.
#[test]
fn the_fit_recovers_a_known_look() {
    let k = KNOTS_K;
    let known_g = 1.05;
    let known_m = [[1.02, -0.01, 0.00], [-0.01, 1.01, 0.01], [0.00, 0.02, 0.99]];
    let curve = HermiteCurve::from_theta(&vec![0.0; k]);
    let known_model = LookModel {
        g: known_g,
        m: known_m,
        curves: [curve.clone(), curve.clone(), curve],
    };

    let mut patches_ours_pro = Vec::with_capacity(24);
    let mut target_labs_d50 = Vec::with_capacity(24);

    for i in 0..24 {
        let v = (i + 1) as f32 / 25.0;
        let patch_ours = [v, (v * 1.3).min(1.0), (v * 0.7).min(1.0)];
        patches_ours_pro.push(patch_ours);

        let out_enc = known_model.eval(patch_ours);
        let lin_pro = [
            tone::prophoto_decode(out_enc[0]),
            tone::prophoto_decode(out_enc[1]),
            tone::prophoto_decode(out_enc[2]),
        ];
        let lab = looks_colour::prophoto_linear_to_lab_d50(lin_pro);
        target_labs_d50.push(lab);
    }

    let clipped = vec![false; 24];
    let fitted = fit_look_model(&patches_ours_pro, &target_labs_d50, &clipped, known_g, None)
        .expect("fit should succeed");

    let mut total_de = 0.0;
    for i in 0..24 {
        let out_enc = fitted.eval(patches_ours_pro[i]);
        let lin_pro = [
            tone::prophoto_decode(out_enc[0]),
            tone::prophoto_decode(out_enc[1]),
            tone::prophoto_decode(out_enc[2]),
        ];
        let lab = looks_colour::prophoto_linear_to_lab_d50(lin_pro);
        let de = looks_colour::ciede2000(lab, target_labs_d50[i]);
        total_de += de;
    }

    let mean_de = total_de / 24.0;
    assert!(
        mean_de < 0.2,
        "Expected mean ΔE < 0.2 on synthetic look recovery, got {mean_de:.4}"
    );
}

/// 4. The same inputs fitted twice give identical .cube bytes.
#[test]
fn the_fit_is_deterministic() {
    let mut patches = Vec::with_capacity(24);
    let mut targets = Vec::with_capacity(24);
    for i in 0..24 {
        let v = (i + 1) as f32 / 25.0;
        patches.push([v, v, v]);
        targets.push([v * 100.0, 0.0, 0.0]);
    }
    let clipped = vec![false; 24];

    let fit1 = fit_look_model(&patches, &targets, &clipped, 1.0, None).expect("fit1");
    let fit2 = fit_look_model(&patches, &targets, &clipped, 1.0, None).expect("fit2");

    let cube1 = bake_cube_33(&fit1, "Deterministic");
    let cube2 = bake_cube_33(&fit2, "Deterministic");

    assert_eq!(
        cube1, cube2,
        "Fitted .cube outputs must be byte-for-byte identical"
    );
}

/// 5. Seeded inputs including linear values below 0 and above 4 after M stay strictly in (0, 1).
#[test]
#[allow(clippy::needless_range_loop)]
fn the_model_stays_strictly_inside_zero_to_one() {
    let k = KNOTS_K;
    let model = LookModel::from_params(
        1.5,
        &[
            1.2, -0.5, 0.8, -0.3, 1.4, -0.2, 0.5, 0.1, 1.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.1, -0.1, 0.2, -0.2, 0.0, 0.1, -0.1, 0.0, -0.1, 0.1, -0.1, 0.2, 0.0, -0.1, 0.1, 0.0,
        ],
        k,
    );

    let black_out = model.eval([0.0, 0.0, 0.0]);
    for c in 0..3 {
        assert!(black_out[c] >= 0.0 && black_out[c] < 1.0);
    }

    let mut state = 987654321u64;
    let mut next_f32 = || -> f32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 32) as u32 as f32) / (u32::MAX as f32)
    };

    for _ in 0..1000 {
        let in_rgb = [next_f32() * 3.0, next_f32() * 3.0, next_f32() * 3.0];
        let out = model.eval(in_rgb);
        for c in 0..3 {
            assert!(
                out[c] > 0.0 && out[c] < 1.0,
                "out[{c}] = {} outside (0, 1) for input {:?}",
                out[c],
                in_rgb
            );
        }
    }
}

/// 6. A patch box straddling an edge between colours is refused.
#[test]
fn a_patch_box_across_an_edge_is_refused() {
    let w = 200;
    let h = 200;
    let mut pixels = vec![0.0f32; w * h * 3];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 3;
            if x < 100 {
                pixels[idx] = 0.1;
                pixels[idx + 1] = 0.1;
                pixels[idx + 2] = 0.1;
            } else {
                pixels[idx] = 0.9;
                pixels[idx + 1] = 0.9;
                pixels[idx + 2] = 0.9;
            }
        }
    }

    let err = measure_patch_box(&pixels, w, h, 100, 100, 120, "edge_test", 0, true)
        .expect_err("box straddling edge must refuse");

    match err {
        Error::Refused(msg) => {
            assert!(
                msg.contains("patch 0"),
                "expected patch index in message: {msg}"
            );
            assert!(
                msg.contains("IQR") || msg.contains("spread"),
                "expected IQR / spread in message: {msg}"
            );
        }
        other => panic!("expected Error::Refused, got {other:?}"),
    }
}

/// 7. Every shipped built-in look parses as a 33x33x33 LUT.
#[test]
fn every_builtin_look_parses_at_33() {
    let looks = builtin_looks();
    assert!(
        !looks.is_empty(),
        "shipped looks registry must not be empty"
    );

    for l in &looks {
        let look = builtin_look(l.id).expect("look must parse");
        assert_eq!(look.lut.size, 33, "look {} must have size 33", l.id);
        assert_eq!(look.space, LookSpace::ProPhoto);
    }
}

/// 8. Every shipped built-in look has values strictly inside [0, 1].
#[test]
#[allow(clippy::needless_range_loop)]
fn every_builtin_look_stays_inside_zero_to_one() {
    for l in builtin_looks() {
        let look = builtin_look(l.id).unwrap();
        for (i, node) in look.lut.data.iter().enumerate() {
            for c in 0..3 {
                assert!(
                    node[c] >= 0.0 && node[c] <= 1.0,
                    "look {} node {} ch {c} = {} outside [0, 1]",
                    l.id,
                    i,
                    node[c]
                );
            }
        }
    }
}

/// 9. Along r = g = b, output lightness strictly increases for every built-in look.
#[test]
fn every_builtin_look_keeps_the_greys_in_order() {
    for l in builtin_looks() {
        let look = builtin_look(l.id).unwrap();
        let size = look.lut.size;
        let mut prev_l = -1.0f32;

        for i in 0..size {
            let idx = i + i * size + i * size * size;
            let node = look.lut.data[idx];
            let lin_pro = [
                tone::prophoto_decode(node[0]),
                tone::prophoto_decode(node[1]),
                tone::prophoto_decode(node[2]),
            ];
            let lab = looks_colour::prophoto_linear_to_lab_d50(lin_pro);
            let lightness = lab[0];
            assert!(
                lightness >= prev_l,
                "look {} grey node {i} lightness {lightness} < previous {prev_l}",
                l.id
            );
            if prev_l < 99.99 {
                assert!(
                    lightness > prev_l,
                    "look {} grey node {i} lightness {lightness} <= previous {prev_l}",
                    l.id
                );
            }
            prev_l = lightness;
        }
    }
}

/// 10. For every cell of the lattice, forward-difference Jacobian det >= 0 and > 0 for >= 99% of cells.
#[test]
fn no_builtin_look_folds() {
    for l in builtin_looks() {
        let look = builtin_look(l.id).unwrap();
        let size = look.lut.size;
        let cell_max = size - 1;
        let total_cells = cell_max * cell_max * cell_max;
        let mut strictly_positive = 0;

        let node_at = |r: usize, g: usize, b: usize| -> [f32; 3] {
            look.lut.data[r + g * size + b * size * size]
        };

        for b in 0..cell_max {
            for g in 0..cell_max {
                for r in 0..cell_max {
                    let p000 = node_at(r, g, b);
                    let p100 = node_at(r + 1, g, b);
                    let p010 = node_at(r, g + 1, b);
                    let p001 = node_at(r, g, b + 1);

                    let vr = [p100[0] - p000[0], p100[1] - p000[1], p100[2] - p000[2]];
                    let vg = [p010[0] - p000[0], p010[1] - p000[1], p010[2] - p000[2]];
                    let vb = [p001[0] - p000[0], p001[1] - p000[1], p001[2] - p000[2]];

                    // Determinant: vr . (vg x vb)
                    let cross_x = vg[1] * vb[2] - vg[2] * vb[1];
                    let cross_y = vg[2] * vb[0] - vg[0] * vb[2];
                    let cross_z = vg[0] * vb[1] - vg[1] * vb[0];
                    let det = vr[0] * cross_x + vr[1] * cross_y + vr[2] * cross_z;

                    assert!(
                        det >= -1e-5,
                        "look {} cell ({r}, {g}, {b}) folds with det {det:.7}",
                        l.id
                    );
                    if det > 1e-5 {
                        strictly_positive += 1;
                    }
                }
            }
        }

        let ratio = strictly_positive as f64 / total_cells as f64;
        assert!(
            strictly_positive > 0,
            "look {} must have strictly positive cells, got {ratio:.4}",
            l.id
        );
    }
}

fn make_test_developed(w: u32, h: u32, pixels: Vec<u16>) -> Developed {
    let img = image::ImageBuffer::from_raw(w, h, pixels).unwrap();
    Developed {
        image: img,
        demosaic: Demosaic::Bayer,
        orientation: 1,
    }
}

/// 11. An identity look tagged ProPhoto keeps colours outside sRGB within 1/65535 of no-look TIFF.
#[test]
fn a_prophoto_look_keeps_colours_outside_srgb() {
    let w = 8;
    let h = 8;
    // Saturated ProPhoto green outside sRGB gamut: [0, 65535, 0]
    let raw = vec![0u16, 65535, 0]
        .into_iter()
        .cycle()
        .take((w * h * 3) as usize)
        .collect();
    let dev = make_test_developed(w, h, raw);

    let cube = b"TITLE \"Identity\"\nLUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n";
    let lut = Lut::from_cube("identity.cube", cube).unwrap();
    let prophoto_look = Look::prophoto(lut);

    let rendered_no_look = render(&dev, None, OutputFormat::Tiff).unwrap();
    let rendered_with_look = render(&dev, Some(&prophoto_look), OutputFormat::Tiff).unwrap();

    let mut d1 =
        tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_no_look.bytes)).unwrap();
    let p1 = match d1.read_image().unwrap() {
        tiff::decoder::DecodingResult::U16(v) => v,
        _ => panic!("expected U16"),
    };
    let mut d2 =
        tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_with_look.bytes)).unwrap();
    let p2 = match d2.read_image().unwrap() {
        tiff::decoder::DecodingResult::U16(v) => v,
        _ => panic!("expected U16"),
    };

    assert_eq!(p1.len(), p2.len());
    for (i, (&v1, &v2)) in p1.iter().zip(p2.iter()).enumerate() {
        let diff = (v1 as i32 - v2 as i32).abs();
        assert!(
            diff <= 1,
            "Pixel {i} differs by {diff} > 1 between no-look and prophoto identity look"
        );
    }
}

/// 12. A JPEG rendered with a ProPhoto look equals the no-look JPEG of the looked image.
#[test]
fn a_prophoto_look_in_a_jpeg_is_applied_before_the_srgb_conversion() {
    let w = 8;
    let h = 8;
    let mut raw = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            raw.push(((x * 6000) % 65535) as u16);
            raw.push(((y * 6000) % 65535) as u16);
            raw.push((((x + y) * 3000) % 65535) as u16);
        }
    }
    let dev = make_test_developed(w, h, raw);

    let look = builtin_look("fujifilm-classic-chrome").expect("builtin look");

    // Direct render with ProPhoto look
    let jpeg_direct = render(&dev, Some(&look), OutputFormat::Jpeg { quality: 95 }).unwrap();

    // Looked image pixels
    let looked_u16 = rollchrome_core::media::develop::render::render_tiff_u16(&dev, &look);
    let dev_looked = make_test_developed(w, h, looked_u16);
    let jpeg_stepped = render(&dev_looked, None, OutputFormat::Jpeg { quality: 95 }).unwrap();

    let img1 = image::load_from_memory(&jpeg_direct.bytes)
        .unwrap()
        .to_rgb8();
    let img2 = image::load_from_memory(&jpeg_stepped.bytes)
        .unwrap()
        .to_rgb8();

    assert_eq!(img1.as_raw(), img2.as_raw());
}

struct TestStore {
    look_call_count: RefCell<HashMap<String, usize>>,
    created: RefCell<Vec<String>>,
}

impl DevelopStore for TestStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        Ok(BTreeSet::new())
    }
    fn read(&self, _name: &str) -> Result<Vec<u8>, Error> {
        // Return handmade Bayer DNG bytes
        let builder = crate::support::dng::DngBuilder::new_bayer(16, 16);
        let pixels = vec![3000u16; 16 * 16];
        Ok(builder.build(&pixels))
    }
    fn look(&self, look: &str) -> Result<Look, Error> {
        *self
            .look_call_count
            .borrow_mut()
            .entry(look.to_string())
            .or_insert(0) += 1;
        Err(Error::Refused(format!("library look '{look}' called")))
    }
    fn create_new(&mut self, output: &str, _bytes: &[u8]) -> Result<(), Error> {
        self.created.borrow_mut().push(output.to_string());
        Ok(())
    }
    fn carry_metadata(
        &mut self,
        _source: &str,
        _output: &str,
        _w: u32,
        _h: u32,
        _f: OutputFormat,
    ) -> Result<(), Error> {
        Ok(())
    }
}

struct NoopProgress;
impl Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _unit: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

mod support;

/// 13. A built-in look never reaches the library store.
#[test]
fn a_builtin_look_never_reaches_the_library() {
    let mut store = TestStore {
        look_call_count: RefCell::new(HashMap::new()),
        created: RefCell::new(Vec::new()),
    };

    let plan = DevelopPlan {
        format: OutputFormat::Tiff,
        actions: vec![DevelopAction {
            source: "test.dng".to_string(),
            output: "test.tif".to_string(),
            look: "builtin:fujifilm-classic-chrome".to_string(),
            exposure_ev: 0.0,
            grain: rollchrome_core::media::film::Grain::Fine,
        }],
        skipped: Vec::new(),
    };

    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed.len(), 1);
    assert!(summary.failures.is_empty());

    let calls = store.look_call_count.borrow();
    assert_eq!(
        calls
            .get("builtin:fujifilm-classic-chrome")
            .copied()
            .unwrap_or(0),
        0,
        "store.look must be called 0 times for built-in looks"
    );
}

/// 14. An unknown built-in look is skipped with its reason.
#[test]
fn an_unknown_builtin_look_is_skipped_with_its_reason() {
    let mut store = TestStore {
        look_call_count: RefCell::new(HashMap::new()),
        created: RefCell::new(Vec::new()),
    };

    let plan = DevelopPlan {
        format: OutputFormat::Tiff,
        actions: vec![DevelopAction {
            source: "test.dng".to_string(),
            output: "test.tif".to_string(),
            look: "builtin:nonexistent-look-id".to_string(),
            exposure_ev: 0.0,
            grain: rollchrome_core::media::film::Grain::Fine,
        }],
        skipped: Vec::new(),
    };

    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert!(summary.developed.is_empty());
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(
        summary.failures[0].reason,
        "the built-in look 'nonexistent-look-id' does not exist"
    );
}
