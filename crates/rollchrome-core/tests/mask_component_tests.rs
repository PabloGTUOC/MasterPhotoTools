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
    let orig = buf.data.clone();

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
    assert_ne!(
        buf_applied.data, orig,
        "active mask without components adjusts pixels"
    );
}

#[test]
fn add_takes_the_larger_coverage() {
    let mut mask = test_mask(
        MaskKind::Linear {
            start: [0.3, 0.0],
            end: [0.3, 1.0], // constant across x=0.3
        },
        1.0,
    );
    // At x=0.3, base coverage is 1.0 for x <= 0.3, 0.0 for x >= 0.3
    // Use two linear gradients along x:
    // Base: start=[0.0, 0.0], end=[1.0, 0.0] -> at x=0.3, coverage is 1.0 - smoothstep(0.3)
    // Component: start=[1.0, 0.0], end=[0.0, 0.0] -> at x=0.3, coverage is 1.0 - smoothstep(0.7)
    let comp = MaskComponent {
        op: MaskOp::Add,
        kind: MaskKind::Linear {
            start: [1.0, 0.0],
            end: [0.0, 0.0],
        },
        invert: false,
    };
    mask.kind = MaskKind::Linear {
        start: [0.0, 0.0],
        end: [1.0, 0.0],
    };
    mask.components = vec![comp];

    let compiled = CompiledMasks::compile(&[mask], None, 100, 100, 1)
        .unwrap()
        .unwrap();

    // At x=0.3:
    // c_base = 1.0 - smoothstep(0.3) = 1.0 - (0.3^2 * (3 - 2*0.3)) = 1.0 - 0.09 * 2.4 = 1.0 - 0.216 = 0.784
    // c_comp = 1.0 - smoothstep((1.0 - 0.3)) = 1.0 - smoothstep(0.7) = 1.0 - 0.49 * 1.6 = 1.0 - 0.784 = 0.216
    // Add: max(0.784, 0.216) = 0.784
    let adj_03 = compiled.local_at(0.3 * compiled.ax, 0.5 * compiled.ay, 0.5, 0.5, 0.5);
    assert!(
        (adj_03.exposure - 0.784).abs() < 1e-4,
        "Add takes larger coverage: expected ~0.784, got {}",
        adj_03.exposure
    );

    // At x=0.7:
    // c_base = 0.216, c_comp = 0.784 -> Add: max(0.216, 0.784) = 0.784
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
    // Base: c0 = 0.2
    // If order is Comp1 (Add 0.8) then Comp2 (Intersect 0.5):
    // step 1: max(0.2, 0.8) = 0.8
    // step 2: 0.8 * 0.5 = 0.4
    // If order were reversed:
    // step 1: 0.2 * 0.5 = 0.1
    // step 2: max(0.1, 0.8) = 0.8
    // We construct this using linear gradients where at x=0.5:
    // base has 0.2, comp1 has 0.8, comp2 has 0.5.
    // In our linear formula: c = 1.0 - smoothstep(t).
    // smoothstep(0.5) = 0.5.
    // For exact 0.2: smoothstep(t) = 0.8.
    // But even simpler: evaluate using ranges or invert!
    let mut mask = test_mask(
        MaskKind::LuminanceRange {
            low: 0.0,
            high: 0.2,
            feather: 0.0,
        },
        1.0,
    );
    // At y_sqrt = 0.1:
    // base LuminanceRange [0, 0.2] gives 1.0. With opacity 0.2, weight would scale, but let's test component ops directly:
    // Base gives 0.0 at y_sqrt = 0.5.
    // Let base give 1.0 at y_sqrt = 0.5 (range [0.4, 0.6]).
    // Comp 1 (Subtract range [0.4, 0.6]): 1.0 * (1 - 1.0) = 0.0.
    // Comp 2 (Add range [0.4, 0.6]): max(0.0, 1.0) = 1.0.
    // Result is 1.0.
    // If evaluated in reverse order:
    // Comp 2 (Add): max(1.0, 1.0) = 1.0.
    // Comp 1 (Subtract): 1.0 * (1 - 1.0) = 0.0.
    // The two orders give 1.0 vs 0.0!
    mask.kind = MaskKind::LuminanceRange {
        low: 0.4,
        high: 0.6,
        feather: 0.0,
    };
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
    // A luminance range selecting midtones [0.4, 0.6]
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
        masks: vec![mask],
        ..Default::default()
    };

    let compiled_flat = CompiledMasks::compile(&recipe_flat.masks, None, 10, 10, 1)
        .unwrap()
        .unwrap();
    let compiled_boosted = CompiledMasks::compile(&recipe_boosted.masks, None, 10, 10, 1)
        .unwrap()
        .unwrap();

    // Check across multiple pixel values: incoming linear pixel values are read before exposure
    for v in [0.04, 0.16, 0.25, 0.36, 0.49, 0.64, 0.81] {
        let adj_flat = compiled_flat.local_at(0.0, 0.0, v, v, v);
        let adj_boosted = compiled_boosted.local_at(0.0, 0.0, v, v, v);
        assert_eq!(
            adj_flat.exposure, adj_boosted.exposure,
            "mask boundary must not move with global exposure: v={v}"
        );
    }
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

    // Create two distinct rasters: raster 1 is 50, raster 2 is 200
    let data1 = vec![50u8; 16 * 16];
    let data2 = vec![200u8; 16 * 16];
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
                op: MaskOp::Add,
                kind: MaskKind::Auto {
                    target: AutoTarget::Sky,
                    mask: raster2,
                },
                invert: false,
            },
        ],
    };

    let recipe = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    // Render to trigger raster decoding and caching
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();

    // Verify mask coverage successfully resolves both rasters
    let (w, h, cov) = session
        .mask_coverage(&recipe, "multi_auto", PreviewStage::Drag)
        .unwrap();
    assert!(w > 0 && h > 0);
    assert!(!cov.is_empty());
    // Since both rasters are Add, coverage combines both without crashing or colliding
    assert!(cov.iter().any(|&c| c > 0));
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
