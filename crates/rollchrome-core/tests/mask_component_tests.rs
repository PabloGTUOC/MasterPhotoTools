//! Tests for mask components and range masks (RP-11).

use rollchrome_core::media::edit::color::oklab_to_linear_srgb;
use rollchrome_core::media::edit::geometry::{Geometry, NormalizedCrop};
use rollchrome_core::media::edit::masks::{
    AutoTarget, CompiledMasks, LocalAdjustments, Mask, MaskComponent, MaskKind, MaskOp,
};
use rollchrome_core::media::edit::raster::StoredRaster;
use rollchrome_core::media::edit::{
    apply_recipe, decode_image, AdjustmentRecipe, LinearBuffer, PreviewSession, PreviewStage,
};

fn test_mask(kind: MaskKind, exposure: f32) -> Mask {
    Mask {
        id: "test_mask".into(),
        name: "Test Mask".into(),
        kind,
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
        detail: Default::default(),
    }
}

#[test]
fn a_mask_without_components_renders_exactly_as_before() {
    let mut buf = LinearBuffer::new(8, 8);
    for (i, px) in buf.data.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let v = (i as f32 + 1.0) / 65.0;
        px[0] = v;
        px[1] = v * 0.8;
        px[2] = (1.0 - v).max(0.0);
    }

    let mask = test_mask(
        MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
        },
        1.0,
    );

    let recipe = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    let compiled = CompiledMasks::compile(&recipe.masks, None, 8, 8, 1)
        .unwrap()
        .unwrap();
    assert!(
        !compiled.has_range_masks,
        "a mask without components has no range masks"
    );

    let mut buf_applied = buf.clone();
    buf_applied.apply_adjustments_masked(&recipe, Some(&compiled));

    #[rustfmt::skip]
    const EXPECTED_BYTES: [f32; 192] = [
        0.03053064, 0.024424514, 1.953961, 0.059732646, 0.047786117, 1.8815783, 0.08658792, 0.069270335, 1.7894837, 0.11044364, 0.08835491, 1.6842655, 0.13099891, 0.10479913, 1.5719869, 0.14825867, 0.11860694, 1.4578769, 0.1624704, 0.12997633, 1.3461834, 0.17405705, 0.13924564, 1.2401565,
        0.26879692, 0.21503754, 1.6725141, 0.2886264, 0.23090114, 1.5874451, 0.30372, 0.24297601, 1.4909891, 0.31439736, 0.2515179, 1.3885884, 0.32122713, 0.2569817, 1.2849085, 0.3249408, 0.25995266, 1.183713, 0.32635698, 0.26108557, 1.0878565, 0.32632253, 0.26105803, 0.99936277,
        0.4906649, 0.39253193, 1.3854066, 0.49699637, 0.3975971, 1.2977128, 0.49779585, 0.3982367, 1.2051898, 0.49419558, 0.39535648, 1.11194, 0.48741123, 0.38992903, 1.0212425, 0.47865692, 0.3829255, 0.93555665, 0.46908864, 0.37527093, 0.8565966, 0.45977533, 0.36782026, 0.78544956,
        0.69027275, 0.5522182, 1.1044363, 0.6811943, 0.5449555, 1.0217915, 0.667164, 0.5337312, 0.9389715, 0.6498816, 0.5199053, 0.85877216, 0.6309568, 0.50476545, 0.7832567, 0.61185473, 0.4894838, 0.7138306, 0.5938765, 0.4751012, 0.65134835, 0.57817006, 0.46253607, 0.5962379,
        0.86459285, 0.6916743, 0.8383929, 0.84013253, 0.672106, 0.7660031, 0.81235206, 0.64988166, 0.6963017, 0.78325677, 0.6266054, 0.63095677, 0.75462085, 0.6036967, 0.5710644, 0.72797763, 0.58238214, 0.5172472, 0.7046448, 0.5637158, 0.46976316, 0.68577653, 0.54862124, 0.4286103,
        1.013101, 0.81048083, 0.5930346, 0.97482246, 0.77985805, 0.5338313, 0.93555665, 0.74844533, 0.47865686, 0.89738697, 0.7179096, 0.4282983, 0.8620787, 0.689663, 0.38314608, 0.8311195, 0.66489565, 0.34328845, 0.8057874, 0.64462996, 0.3085994, 0.7872424, 0.62979394, 0.278815,
        1.1372929, 0.90983427, 0.3713609, 1.0878565, 0.8702853, 0.32635695, 1.040153, 0.8321225, 0.2855322, 0.9961799, 0.79694396, 0.24904495, 0.9575942, 0.7660753, 0.21681376, 0.9257983, 0.7406386, 0.18858853, 0.9020486, 0.7216389, 0.16400883, 0.88758415, 0.71006733, 0.14264745,
        1.2401565, 0.9921253, 0.17405704, 1.1829191, 0.9463353, 0.1427661, 1.130281, 0.9042248, 0.114943825, 1.0840689, 0.86725515, 0.09033907, 1.0458091, 0.83664733, 0.06857765, 1.0168548, 0.8134839, 0.049202647, 0.9985322, 0.79882574, 0.03169943, 0.9923099, 0.7938479, 0.015504842,
    ];
    assert_eq!(
        buf_applied.data, EXPECTED_BYTES,
        "a mask without components renders exactly as on main"
    );

    // Also assert that "components": [] and an absent components field deserialize and render identically
    let json_absent = r#"{
        "id": "test_mask",
        "name": "Test Mask",
        "kind": { "type": "linear", "start": [0.0, 0.0], "end": [1.0, 1.0] },
        "invert": false,
        "opacity": 1.0,
        "enabled": true,
        "adjustments": { "exposure": 1.0 }
    }"#;
    let json_empty = r#"{
        "id": "test_mask",
        "name": "Test Mask",
        "kind": { "type": "linear", "start": [0.0, 0.0], "end": [1.0, 1.0] },
        "invert": false,
        "opacity": 1.0,
        "enabled": true,
        "adjustments": { "exposure": 1.0 },
        "components": []
    }"#;
    let mask_absent: Mask = serde_json::from_str(json_absent).unwrap();
    let mask_empty: Mask = serde_json::from_str(json_empty).unwrap();

    let recipe_absent = AdjustmentRecipe {
        masks: vec![mask_absent],
        ..Default::default()
    };
    let compiled_absent = CompiledMasks::compile(&recipe_absent.masks, None, 8, 8, 1)
        .unwrap()
        .unwrap();
    let mut buf_absent = buf.clone();
    buf_absent.apply_adjustments_masked(&recipe_absent, Some(&compiled_absent));
    assert_eq!(buf_absent.data, EXPECTED_BYTES);

    let recipe_empty = AdjustmentRecipe {
        masks: vec![mask_empty],
        ..Default::default()
    };
    let compiled_empty = CompiledMasks::compile(&recipe_empty.masks, None, 8, 8, 1)
        .unwrap()
        .unwrap();
    let mut buf_empty = buf.clone();
    buf_empty.apply_adjustments_masked(&recipe_empty, Some(&compiled_empty));
    assert_eq!(buf_empty.data, EXPECTED_BYTES);
}

#[test]
fn add_takes_the_larger_coverage() {
    let mut mask = test_mask(
        MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
        },
        1.0,
    );
    let comp = MaskComponent {
        op: MaskOp::Add,
        kind: MaskKind::Linear {
            start: [1.0, 0.0],
            end: [0.0, 0.0],
        },
        invert: false,
    };
    mask.components = vec![comp];

    let compiled = CompiledMasks::compile(&[mask], None, 100, 100, 1)
        .unwrap()
        .unwrap();

    let adj_03 = compiled.local_at(0.3 * compiled.ax, 0.5 * compiled.ay, 0.5, 0.5, 0.5);
    assert!(
        (adj_03.exposure - 0.784).abs() < 1e-4,
        "Add takes larger coverage: expected ~0.784, got {}",
        adj_03.exposure
    );

    let adj_07 = compiled.local_at(0.7 * compiled.ax, 0.5 * compiled.ay, 0.5, 0.5, 0.5);
    assert!(
        (adj_07.exposure - 0.784).abs() < 1e-4,
        "Add takes larger coverage: expected ~0.784, got {}",
        adj_07.exposure
    );
}

#[test]
fn subtract_removes_coverage() {
    let mut mask = test_mask(
        MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
        },
        1.0,
    );
    // At x=0.0: c_base = 1.0
    // Component at x=0.0: c_comp = 1.0 with Subtract -> c = 1.0 * (1.0 - 1.0) = 0.0
    mask.components = vec![MaskComponent {
        op: MaskOp::Subtract,
        kind: MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 0.0],
        },
        invert: false,
    }];

    let compiled = CompiledMasks::compile(&[mask], None, 100, 100, 1)
        .unwrap()
        .unwrap();

    let adj = compiled.local_at(0.0, 0.5 * compiled.ay, 0.5, 0.5, 0.5);
    assert_eq!(
        adj.exposure, 0.0,
        "Subtract with full coverage removes all coverage"
    );

    // Half coverage subtract: base = 1.0, comp = 0.5 -> 1.0 * (1 - 0.5) = 0.5
    let mut mask2 = test_mask(
        MaskKind::Linear {
            start: [1.0, 0.0],
            end: [2.0, 0.0], // at x <= 1.0, base is 1.0
        },
        1.0,
    );
    mask2.components = vec![MaskComponent {
        op: MaskOp::Subtract,
        kind: MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 0.0], // at x=0.5, comp = 0.5
        },
        invert: false,
    }];
    let compiled2 = CompiledMasks::compile(&[mask2], None, 100, 100, 1)
        .unwrap()
        .unwrap();
    let adj2 = compiled2.local_at(0.5 * compiled2.ax, 0.5 * compiled2.ay, 0.5, 0.5, 0.5);
    assert!(
        (adj2.exposure - 0.5).abs() < 1e-4,
        "Subtract with 0.5 coverage yields 0.5"
    );
}

#[test]
fn intersect_limits_coverage() {
    let mut mask = test_mask(
        MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 0.0], // at x=0.5, c = 0.5
        },
        1.0,
    );
    mask.components = vec![MaskComponent {
        op: MaskOp::Intersect,
        kind: MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 0.0], // at x=0.5, c = 0.5
        },
        invert: false,
    }];

    let compiled = CompiledMasks::compile(&[mask], None, 100, 100, 1)
        .unwrap()
        .unwrap();

    // At x=0.5: c = 0.5 * 0.5 = 0.25
    let adj = compiled.local_at(0.5 * compiled.ax, 0.5 * compiled.ay, 0.5, 0.5, 0.5);
    assert!(
        (adj.exposure - 0.25).abs() < 1e-4,
        "Intersect: 0.5 * 0.5 = 0.25, got {}",
        adj.exposure
    );

    // At x=1.0: c_base = 0.0 -> 0.0 * anything = 0.0
    let adj_end = compiled.local_at(1.0 * compiled.ax, 0.5 * compiled.ay, 0.5, 0.5, 0.5);
    assert_eq!(adj_end.exposure, 0.0);
}

#[test]
fn components_apply_in_order() {
    let mut mask = test_mask(
        MaskKind::LuminanceRange {
            low: 0.4,
            high: 0.6,
            feather: 0.0,
        },
        1.0,
    );
    mask.components = vec![
        MaskComponent {
            op: MaskOp::Subtract,
            kind: MaskKind::LuminanceRange {
                low: 0.4,
                high: 0.6,
                feather: 0.0,
            },
            invert: false,
        },
        MaskComponent {
            op: MaskOp::Add,
            kind: MaskKind::LuminanceRange {
                low: 0.4,
                high: 0.6,
                feather: 0.0,
            },
            invert: false,
        },
    ];

    let compiled = CompiledMasks::compile(&[mask], None, 10, 10, 1)
        .unwrap()
        .unwrap();

    // At pixel with y_sqrt = 0.5:
    // Base = 1.0 -> Subtract 1.0 = 0.0 -> Add 1.0 = 1.0
    let adj = compiled.local_at(0.0, 0.0, 0.25, 0.25, 0.25);
    assert_eq!(
        adj.exposure, 1.0,
        "Components must apply in forward order (Subtract then Add yields 1.0)"
    );
}

#[test]
fn a_luminance_range_selects_only_its_tones() {
    let mask = test_mask(
        MaskKind::LuminanceRange {
            low: 0.4,
            high: 0.6,
            feather: 0.1,
        },
        1.0,
    );
    let compiled = CompiledMasks::compile(&[mask], None, 10, 10, 1)
        .unwrap()
        .unwrap();

    assert!(compiled.has_range_masks);
    assert!(compiled.needs_luminance);
    assert!(!compiled.needs_colour);

    // y_lin for grey pixel (r=g=b=v) is v, y_sqrt = sqrt(v)
    // 1. Inside range [0.4, 0.6]: v = 0.25 -> y_sqrt = 0.5
    let adj_inside = compiled.local_at(0.0, 0.0, 0.25, 0.25, 0.25);
    assert_eq!(
        adj_inside.exposure, 1.0,
        "inside luminance range gets full coverage 1.0"
    );

    // 2. Far below range: v = 0.04 -> y_sqrt = 0.2 (< 0.4 - 0.1)
    let adj_below = compiled.local_at(0.0, 0.0, 0.04, 0.04, 0.04);
    assert_eq!(
        adj_below.exposure, 0.0,
        "below luminance range feather gets 0.0"
    );

    // 3. Far above range: v = 0.81 -> y_sqrt = 0.9 (> 0.6 + 0.1)
    let adj_above = compiled.local_at(0.0, 0.0, 0.81, 0.81, 0.81);
    assert_eq!(
        adj_above.exposure, 0.0,
        "above luminance range feather gets 0.0"
    );

    // 4. In lower feather zone: v = 0.1225 -> y_sqrt = 0.35 (distance 0.05 from 0.4 with feather 0.1)
    // smoothstep(0.5) = 0.5, so 1.0 - 0.5 = 0.5
    let adj_feather = compiled.local_at(0.0, 0.0, 0.1225, 0.1225, 0.1225);
    assert!(
        (adj_feather.exposure - 0.5).abs() < 1e-4,
        "halfway in feather zone gets 0.5, got {}",
        adj_feather.exposure
    );
}

#[test]
fn a_colour_range_selects_only_its_colour() {
    // Picked OkLCh green: hue 140.0, chroma 0.15, lightness 0.6
    let target_h = 140.0f32;
    let target_c = 0.15f32;
    let target_l = 0.6f32;

    let mask = test_mask(
        MaskKind::ColourRange {
            hue: target_h,
            chroma: target_c,
            lightness: target_l,
            range: 50.0,
        },
        1.0,
    );
    let compiled = CompiledMasks::compile(&[mask], None, 10, 10, 1)
        .unwrap()
        .unwrap();

    assert!(compiled.has_range_masks);
    assert!(compiled.needs_colour);

    // Convert target OkLCh to linear sRGB
    let (sin_h, cos_h) = target_h.to_radians().sin_cos();
    let (target_r, target_g, target_b) =
        oklab_to_linear_srgb(target_l, target_c * cos_h, target_c * sin_h);

    let adj_target = compiled.local_at(0.0, 0.0, target_r, target_g, target_b);
    assert!(
        adj_target.exposure > 0.99,
        "exact target colour gets full weight, got {}",
        adj_target.exposure
    );

    // A distinct red pixel: hue ~29°, chroma 0.2
    let (red_r, red_g, red_b) = oklab_to_linear_srgb(0.6, 0.2, 0.05);
    let adj_red = compiled.local_at(0.0, 0.0, red_r, red_g, red_b);
    assert_eq!(
        adj_red.exposure, 0.0,
        "distinct colour outside envelope gets 0.0"
    );

    // Neutral grey: chroma = 0.0 -> chroma_fade is 0.0
    let adj_grey = compiled.local_at(0.0, 0.0, 0.5, 0.5, 0.5);
    assert_eq!(adj_grey.exposure, 0.0, "neutral grey gets 0.0");
}

#[test]
fn a_range_mask_reads_the_pixel_before_the_adjustments() {
    // 1. Export pipeline: through apply_adjustments_masked
    // Create an 8x8 buffer with two alternating pixel tones:
    // Even pixels: midtones (v = 0.25 -> y_sqrt = 0.5, covered by [0.4, 0.6])
    // Odd pixels: shadows (v = 0.04 -> y_sqrt = 0.2, outside [0.4, 0.6])
    let mut buf = LinearBuffer::new(8, 8);
    for (i, px) in buf.data.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let v = if i % 2 == 0 { 0.25 } else { 0.04 };
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }

    let mask = test_mask(
        MaskKind::LuminanceRange {
            low: 0.4,
            high: 0.6,
            feather: 0.05,
        },
        1.0,
    );

    let recipe_flat = AdjustmentRecipe {
        exposure: 0.0,
        masks: vec![mask.clone()],
        ..Default::default()
    };
    let recipe_boosted = AdjustmentRecipe {
        exposure: 2.0, // +2 EV global exposure shift
        masks: vec![mask.clone()],
        ..Default::default()
    };

    let compiled_flat = CompiledMasks::compile(&recipe_flat.masks, None, 8, 8, 1)
        .unwrap()
        .unwrap();
    let compiled_boosted = CompiledMasks::compile(&recipe_boosted.masks, None, 8, 8, 1)
        .unwrap()
        .unwrap();

    let mut buf_flat = buf.clone();
    buf_flat.apply_adjustments_masked(&recipe_flat, Some(&compiled_flat));

    let mut buf_boosted = buf.clone();
    buf_boosted.apply_adjustments_masked(&recipe_boosted, Some(&compiled_boosted));

    // Assert that the exact same pixels receive the local adjustment in both flat and boosted renders.
    // If the pipeline incorrectly evaluated the mask on post-exposure pixels, midtones at +2 EV
    // would be pushed to v = 1.0 (y_sqrt = 1.0 > 0.6), losing their mask coverage, while shadows
    // at +2 EV would become v = 0.16 (y_sqrt = 0.4), gaining spurious mask coverage.
    for i in 0..(8 * 8) {
        let initial = if i % 2 == 0 { 0.25 } else { 0.04 };
        let flat_ratio = buf_flat.data[i * 3] / (initial * 1.0);
        let boosted_ratio = buf_boosted.data[i * 3] / (initial * 4.0);

        assert!(
            (flat_ratio - boosted_ratio).abs() < 1e-4,
            "Pixel {i}: apply_adjustments_masked mask selection must not move with global exposure"
        );
        if i % 2 == 0 {
            assert!(
                flat_ratio > 1.9,
                "midtones must receive +1.0 EV local adjustment"
            );
        } else {
            assert!(
                (flat_ratio - 1.0).abs() < 1e-4,
                "shadows must receive no local adjustment"
            );
        }
    }

    // 2. Preview pipeline: through PreviewSession::render
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preview_range_test.jpg");
    let img = image::RgbImage::from_fn(32, 32, |x, _y| {
        if x < 16 {
            // Midtones (sRGB 128 -> linear ~0.21 -> y_sqrt ~0.46, inside [0.4, 0.6])
            image::Rgb([128, 128, 128])
        } else {
            // Dark shadows (sRGB 10 -> linear ~0.003 -> y_sqrt ~0.05, outside [0.4, 0.6])
            image::Rgb([10, 10, 10])
        }
    });
    img.save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();

    let recipe_flat_nomask = AdjustmentRecipe {
        exposure: 0.0,
        ..Default::default()
    };
    let recipe_boosted_nomask = AdjustmentRecipe {
        exposure: 2.0,
        ..Default::default()
    };

    let frame_flat = session
        .render(&recipe_flat, None, PreviewStage::Drag)
        .unwrap();
    let frame_flat_nomask = session
        .render(&recipe_flat_nomask, None, PreviewStage::Drag)
        .unwrap();
    let frame_boosted = session
        .render(&recipe_boosted, None, PreviewStage::Drag)
        .unwrap();
    let frame_boosted_nomask = session
        .render(&recipe_boosted_nomask, None, PreviewStage::Drag)
        .unwrap();

    let mut flat_mask_count = 0;
    let mut boosted_mask_count = 0;
    for i in 0..(frame_flat.width * frame_flat.height) as usize {
        let idx = i * 4;
        let flat_affected = frame_flat.bytes[idx] != frame_flat_nomask.bytes[idx];
        let boosted_affected = frame_boosted.bytes[idx] != frame_boosted_nomask.bytes[idx];
        assert_eq!(
            flat_affected, boosted_affected,
            "Pixel {i}: preview mask coverage must be identical between 0 EV and +2 EV"
        );
        if flat_affected {
            flat_mask_count += 1;
        }
        if boosted_affected {
            boosted_mask_count += 1;
        }
    }
    assert!(
        flat_mask_count > 0,
        "midtones must be covered by mask in flat render"
    );
    assert_eq!(flat_mask_count, boosted_mask_count);
}

#[test]
fn a_range_mask_follows_a_crop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("split_image.jpg");

    // Left half bright (y_sqrt = 0.8, v = 0.64), right half dark (y_sqrt = 0.2, v = 0.04)
    let img = image::RgbImage::from_fn(100, 100, |x, _y| {
        if x < 50 {
            image::Rgb([200, 200, 200])
        } else {
            image::Rgb([10, 10, 10])
        }
    });
    img.save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();

    // Mask selects only bright pixels (y_sqrt >= 0.5)
    let mask = test_mask(
        MaskKind::LuminanceRange {
            low: 0.5,
            high: 1.0,
            feather: 0.0,
        },
        1.0,
    );

    // Crop to right half (dark)
    let recipe_right = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            ..Default::default()
        }),
        masks: vec![mask.clone()],
        ..Default::default()
    };

    let (w_r, h_r, cov_r) = session
        .mask_coverage(&recipe_right, &mask.id, PreviewStage::Settle)
        .unwrap();
    assert!(w_r > 0 && h_r > 0);
    assert!(
        cov_r.iter().all(|&c| c == 0),
        "cropped dark region should have 0 coverage"
    );

    // Crop to left half (bright)
    let recipe_left = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            ..Default::default()
        }),
        masks: vec![mask.clone()],
        ..Default::default()
    };

    let (w_l, h_l, cov_l) = session
        .mask_coverage(&recipe_left, &mask.id, PreviewStage::Settle)
        .unwrap();
    assert!(w_l > 0 && h_l > 0);
    assert!(
        cov_l.iter().all(|&c| c > 0),
        "cropped bright region should have active coverage"
    );
}

#[test]
fn export_and_preview_agree_on_component_masks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_components.jpg");

    let img = image::RgbImage::from_fn(128, 128, |x, y| {
        let r = (x * 2) as u8;
        let g = (y * 2) as u8;
        let b = 100u8;
        image::Rgb([r, g, b])
    });
    img.save(&path).unwrap();

    let mut mask = test_mask(
        MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
        },
        0.8,
    );
    mask.components = vec![
        MaskComponent {
            op: MaskOp::Subtract,
            kind: MaskKind::Radial {
                center: [0.5, 0.5],
                radius_x: 0.3,
                radius_y: 0.3,
                angle: 0.0,
                feather: 0.2,
            },
            invert: false,
        },
        MaskComponent {
            op: MaskOp::Add,
            kind: MaskKind::LuminanceRange {
                low: 0.3,
                high: 0.7,
                feather: 0.1,
            },
            invert: false,
        },
    ];

    let recipe = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    let session = PreviewSession::open(&path).unwrap();
    let settle_frame = session.render(&recipe, None, PreviewStage::Settle).unwrap();

    let decoded = decode_image(&path).unwrap();
    let exported = apply_recipe(&decoded, &recipe, None).unwrap();
    let exp_bytes = exported.as_rgb8().unwrap();

    // Check sampled points across the image
    for y_step in (8..120).step_by(16) {
        for x_step in (8..120).step_by(16) {
            let u = x_step as f32 / 128.0;
            let v = y_step as f32 / 128.0;

            let sx =
                ((u * settle_frame.width as f32) as usize).min(settle_frame.width as usize - 1);
            let sy =
                ((v * settle_frame.height as f32) as usize).min(settle_frame.height as usize - 1);
            let fx = ((u * exported.width() as f32) as usize).min(exported.width() as usize - 1);
            let fy = ((v * exported.height() as f32) as usize).min(exported.height() as usize - 1);

            let s_idx = (sy * settle_frame.width as usize + sx) * 4;
            let f_idx = (fy * exported.width() as usize + fx) * 3;

            for c in 0..3 {
                let s_val = settle_frame.bytes[s_idx + c] as i32;
                let f_val = exp_bytes[f_idx + c] as i32;
                assert!(
                    (s_val - f_val).abs() <= 2,
                    "Pixel mismatch at ({u}, {v}) ch {c}: settle={s_val}, export={f_val}"
                );
            }
        }
    }
}

#[test]
fn two_auto_components_in_one_mask_keep_their_own_rasters() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dummy.jpg");
    let img = image::RgbImage::from_pixel(64, 64, image::Rgb([128, 128, 128]));
    img.save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();

    // Create two distinct rasters: raster 1 is 200, raster 2 is 50
    let data1 = vec![200u8; 16 * 16];
    let data2 = vec![50u8; 16 * 16];
    let raster1 = StoredRaster::encode(16, 16, &data1, "model_sha_1").unwrap();
    let raster2 = StoredRaster::encode(16, 16, &data2, "model_sha_2").unwrap();

    let mask = Mask {
        id: "multi_auto".into(),
        name: "Multi Auto".into(),
        kind: MaskKind::Brush,
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: 1.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: vec![
            MaskComponent {
                op: MaskOp::Add,
                kind: MaskKind::Auto {
                    target: AutoTarget::Subject,
                    mask: raster1,
                },
                invert: false,
            },
            MaskComponent {
                op: MaskOp::Intersect,
                kind: MaskKind::Auto {
                    target: AutoTarget::Sky,
                    mask: raster2,
                },
                invert: false,
            },
        ],
        detail: Default::default(),
    };

    let recipe = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    // Render twice to trigger raster decoding and caching
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let _ = session.render(&recipe, None, PreviewStage::Settle).unwrap();

    // Assert the session holds two cached rasters after two renders
    assert_eq!(
        session.cached_raster_count(),
        2,
        "session must hold two cached rasters for two auto components"
    );

    // Verify mask coverage: Add at 200 then Intersect at 50 gives 200/255 * 50/255 * 255 = ~39.2
    let (w, h, cov) = session
        .mask_coverage(&recipe, "multi_auto", PreviewStage::Drag)
        .unwrap();
    assert!(w > 0 && h > 0);
    assert!(!cov.is_empty());
    assert!(
        (cov[0] as i32 - 39).abs() <= 1,
        "combined coverage should be about 39, got {}",
        cov[0]
    );
}

#[test]
fn mask_coverage_reflects_the_components() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cov_test.jpg");
    let img = image::RgbImage::from_pixel(100, 100, image::Rgb([128, 128, 128]));
    img.save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();

    // Base mask covers the full image
    let mut mask = test_mask(
        MaskKind::Radial {
            center: [0.5, 0.5],
            radius_x: 2.0,
            radius_y: 2.0,
            angle: 0.0,
            feather: 0.0,
        },
        1.0,
    );
    // Subtract a hard radial mask in the center
    mask.components = vec![MaskComponent {
        op: MaskOp::Subtract,
        kind: MaskKind::Radial {
            center: [0.5, 0.5],
            radius_x: 0.25,
            radius_y: 0.25,
            angle: 0.0,
            feather: 0.0,
        },
        invert: false,
    }];

    let recipe = AdjustmentRecipe {
        masks: vec![mask.clone()],
        ..Default::default()
    };

    let (w, h, cov) = session
        .mask_coverage(&recipe, &mask.id, PreviewStage::Settle)
        .unwrap();

    // Center pixel should be subtracted -> 0
    let center_idx = ((h / 2) * w + (w / 2)) as usize;
    assert_eq!(
        cov[center_idx], 0,
        "center inside subtracted radial must have 0 coverage"
    );

    // Corner pixel should remain covered -> > 0
    let corner_idx = (h / 10 * w + w / 10) as usize;
    assert!(
        cov[corner_idx] > 200,
        "corner outside subtracted radial must have high coverage"
    );
}

#[test]
fn a_mask_written_before_components_still_loads() {
    let json_legacy = r#"{
        "id": "legacy_m",
        "name": "Legacy Mask",
        "kind": {
            "type": "linear",
            "start": [0.2, 0.2],
            "end": [0.8, 0.8]
        },
        "invert": false,
        "opacity": 0.9,
        "enabled": true,
        "adjustments": {
            "exposure": 0.5,
            "contrast": 10.0
        }
    }"#;

    let mask: Mask = serde_json::from_str(json_legacy).expect("legacy mask loads without error");
    assert_eq!(mask.id, "legacy_m");
    assert_eq!(mask.opacity, 0.9);
    assert_eq!(mask.adjustments.exposure, 0.5);
    assert!(
        mask.components.is_empty(),
        "deserialized legacy mask has empty components"
    );

    // Re-serializing must omit components field
    let reserialized = serde_json::to_string(&mask).unwrap();
    assert!(
        !reserialized.contains("\"components\""),
        "empty components are skipped in serialized JSON"
    );

    // Legacy recipe with legacy mask loads with empty components
    let mut base_recipe = AdjustmentRecipe::default();
    base_recipe.masks.push(mask);
    let recipe_json = serde_json::to_string(&base_recipe).unwrap();
    assert!(
        !recipe_json.contains("\"components\""),
        "recipe serialization omits empty components"
    );
    let loaded_recipe: AdjustmentRecipe = serde_json::from_str(&recipe_json).unwrap();
    assert_eq!(loaded_recipe.masks.len(), 1);
    assert!(loaded_recipe.masks[0].components.is_empty());
}
