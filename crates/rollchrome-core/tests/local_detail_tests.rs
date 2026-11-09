//! Integration tests for local detail: clarity, texture, dehaze and sharpness on masks (RP-11b).

use rollchrome_core::media::edit::geometry::{Geometry, NormalizedCrop};
use rollchrome_core::media::edit::masks::{
    CompiledMasks, LocalAdjustments, LocalDetail, Mask, MaskKind,
};
use rollchrome_core::media::edit::pipeline::{
    apply_recipe_with_orientation, AdjustmentRecipe, ImageBuffer, LinearBuffer,
};
use rollchrome_core::media::edit::preview::{PreviewSession, PreviewStage};
use rollchrome_core::media::edit::Detail;

const EXPECTED_PREVIEW_DRAG: [u8; 256] = [
    0, 46, 91, 255, 153, 203, 247, 255, 255, 0, 24, 255, 97, 146, 191, 255, 251, 255, 0, 255, 47,
    95, 139, 255, 203, 249, 255, 255, 5, 53, 96, 255, 166, 215, 255, 255, 255, 0, 46, 255, 111,
    160, 204, 255, 255, 255, 0, 255, 61, 108, 152, 255, 212, 255, 255, 255, 17, 64, 107, 255, 168,
    211, 252, 255, 255, 0, 63, 255, 125, 173, 218, 255, 255, 255, 0, 255, 73, 120, 164, 255, 221,
    255, 0, 255, 29, 75, 117, 255, 177, 220, 255, 255, 0, 30, 72, 255, 140, 188, 232, 255, 255, 0,
    7, 255, 86, 133, 177, 255, 234, 255, 0, 255, 40, 86, 128, 255, 187, 231, 255, 255, 0, 42, 84,
    255, 144, 185, 226, 255, 255, 0, 36, 255, 101, 147, 191, 255, 247, 255, 0, 255, 52, 97, 139,
    255, 198, 242, 255, 255, 8, 53, 95, 255, 154, 195, 236, 255, 255, 12, 60, 255, 116, 163, 206,
    255, 255, 255, 0, 255, 64, 109, 152, 255, 210, 253, 255, 255, 20, 64, 106, 255, 165, 206, 247,
    255, 255, 28, 73, 255, 122, 162, 202, 255, 245, 0, 0, 255, 77, 122, 164, 255, 219, 255, 0, 255,
    31, 75, 117, 255, 175, 217, 255, 255, 0, 33, 73, 255, 133, 173, 213, 255, 238, 0, 41, 255, 91,
    135, 178, 255, 234, 255, 0, 255, 41, 85, 126, 255, 187, 229, 255, 255, 2, 43, 83, 255, 144,
    184, 224, 255, 249, 9, 53, 255, 101, 141, 180, 255,
];

const EXPECTED_EXPORT: [u8; 192] = [
    0, 46, 91, 153, 203, 247, 255, 0, 24, 97, 146, 191, 251, 255, 0, 47, 95, 139, 203, 249, 255, 5,
    53, 96, 166, 215, 255, 255, 0, 46, 111, 160, 204, 255, 255, 0, 61, 108, 152, 212, 255, 255, 17,
    64, 107, 168, 211, 252, 255, 0, 63, 125, 173, 218, 255, 255, 0, 73, 120, 164, 221, 255, 0, 29,
    75, 117, 177, 220, 255, 0, 30, 72, 140, 188, 232, 255, 0, 7, 86, 133, 177, 234, 255, 0, 40, 86,
    128, 187, 231, 255, 0, 42, 84, 144, 185, 226, 255, 0, 36, 101, 147, 191, 247, 255, 0, 52, 97,
    139, 198, 242, 255, 8, 53, 95, 154, 195, 236, 255, 12, 60, 116, 163, 206, 255, 255, 0, 64, 109,
    152, 210, 253, 255, 20, 64, 106, 165, 206, 247, 255, 28, 73, 122, 162, 202, 245, 0, 0, 77, 122,
    164, 219, 255, 0, 31, 75, 117, 175, 217, 255, 0, 33, 73, 133, 173, 213, 238, 0, 41, 91, 135,
    178, 234, 255, 0, 41, 85, 126, 187, 229, 255, 2, 43, 83, 144, 184, 224, 249, 9, 53, 101, 141,
    180,
];

fn test_image(w: u32, h: u32) -> ImageBuffer {
    ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: (0..w * h * 3)
            .map(|i| ((i * 37 + 11) % 256) as u8)
            .collect(),
    }
}

fn full_coverage_mask(detail: LocalDetail) -> Mask {
    Mask {
        id: "full_mask".into(),
        name: "Full Mask".into(),
        kind: MaskKind::Linear {
            start: [0.0, 2.0],
            end: [0.0, 3.0],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail,
        strokes: Vec::new(),
        components: Vec::new(),
    }
}

#[test]
fn a_recipe_without_local_detail_gives_identical_bytes() {
    let buf = test_image(8, 8);
    let session = PreviewSession::new(&buf).unwrap();

    let json_absent = r#"{
        "id": "m1",
        "name": "Mask 1",
        "kind": { "type": "linear", "start": [0.0, 0.0], "end": [1.0, 1.0] },
        "opacity": 0.75,
        "enabled": true,
        "adjustments": {
            "exposure": 0.5,
            "contrast": 10.0,
            "highlights": -5.0,
            "shadows": 5.0,
            "temperature": 10.0,
            "tint": -5.0,
            "saturation": 15.0,
            "vibrance": 5.0
        }
    }"#;

    let json_zeros = r#"{
        "id": "m1",
        "name": "Mask 1",
        "kind": { "type": "linear", "start": [0.0, 0.0], "end": [1.0, 1.0] },
        "opacity": 0.75,
        "enabled": true,
        "adjustments": {
            "exposure": 0.5,
            "contrast": 10.0,
            "highlights": -5.0,
            "shadows": 5.0,
            "temperature": 10.0,
            "tint": -5.0,
            "saturation": 15.0,
            "vibrance": 5.0
        },
        "detail": {
            "clarity": 0.0,
            "texture": 0.0,
            "dehaze": 0.0,
            "sharpness": 0.0
        }
    }"#;

    let mask_absent: Mask = serde_json::from_str(json_absent).unwrap();
    let mask_zeros: Mask = serde_json::from_str(json_zeros).unwrap();

    let detail = Detail {
        clarity: 15.0,
        texture: 10.0,
        sharpen_amount: 25.0,
        dehaze: 5.0,
        ..Default::default()
    };

    let recipe_absent = AdjustmentRecipe {
        exposure: 0.2,
        contrast: 5.0,
        detail: Some(detail.clone()),
        masks: vec![mask_absent],
        ..Default::default()
    };

    let recipe_zeros = AdjustmentRecipe {
        exposure: 0.2,
        contrast: 5.0,
        detail: Some(detail),
        masks: vec![mask_zeros],
        ..Default::default()
    };

    let drag_absent = session
        .render(&recipe_absent, None, PreviewStage::Drag)
        .unwrap();
    let drag_zeros = session
        .render(&recipe_zeros, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(
        drag_absent.bytes, drag_zeros.bytes,
        "absent field and four zeros must match"
    );
    assert_eq!(
        drag_absent.bytes, EXPECTED_PREVIEW_DRAG,
        "must match literal from main"
    );

    let exp_absent = apply_recipe_with_orientation(&buf, &recipe_absent, None, 1).unwrap();
    let exp_zeros = apply_recipe_with_orientation(&buf, &recipe_zeros, None, 1).unwrap();
    assert_eq!(exp_absent.as_rgb8().unwrap(), exp_zeros.as_rgb8().unwrap());
    assert_eq!(exp_absent.as_rgb8().unwrap(), &EXPECTED_EXPORT[..]);
}

#[test]
fn a_mask_with_only_local_detail_is_active() {
    let mask = Mask {
        id: "det_only".into(),
        name: "Detail Only".into(),
        kind: MaskKind::Linear {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            clarity: 25.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    assert!(
        !mask.is_identity(),
        "mask with only local detail must not be identity"
    );

    let compiled = CompiledMasks::compile(std::slice::from_ref(&mask), None, 64, 64, 1).unwrap();
    assert!(
        compiled.is_some(),
        "mask with only local detail must compile"
    );
    let compiled = compiled.unwrap();
    assert!(
        compiled.has_local_detail(),
        "compiled masks must report has_local_detail"
    );

    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let recipe_empty = AdjustmentRecipe::default();
    let recipe_mask = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    let f_empty = session
        .render(&recipe_empty, None, PreviewStage::Drag)
        .unwrap();
    let f_mask = session
        .render(&recipe_mask, None, PreviewStage::Drag)
        .unwrap();
    assert_ne!(
        f_empty.bytes, f_mask.bytes,
        "mask with local detail must alter render"
    );
}

#[test]
fn local_clarity_acts_only_where_the_mask_covers() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    // Mask covers only x < 32 (left half)
    let mask = Mask {
        id: "half_mask".into(),
        name: "Left Half".into(),
        kind: MaskKind::Linear {
            start: [0.5, 0.0],
            end: [0.50001, 0.0],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            clarity: 45.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    let recipe_nomask = AdjustmentRecipe::default();
    let recipe_mask = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    let frame_no = session
        .render(&recipe_nomask, None, PreviewStage::Drag)
        .unwrap();
    let frame_masked = session
        .render(&recipe_mask, None, PreviewStage::Drag)
        .unwrap();

    let mut changed_covered = 0;
    let mut unchanged_uncovered = 0;
    let mut changed_uncovered = 0;

    for y in 0..64 {
        for x in 0..64 {
            let idx = (y * 64 + x) * 4;
            let p_no = &frame_no.bytes[idx..idx + 3];
            let p_mask = &frame_masked.bytes[idx..idx + 3];
            if x < 30 {
                // Covered area
                if p_no != p_mask {
                    changed_covered += 1;
                }
            } else if x > 34 {
                // Completely uncovered area
                if p_no == p_mask {
                    unchanged_uncovered += 1;
                } else {
                    changed_uncovered += 1;
                }
            }
        }
    }

    assert!(changed_covered > 0, "clarity must change covered pixels");
    assert_eq!(
        changed_uncovered, 0,
        "uncovered pixels must be byte-identical"
    );
    assert!(unchanged_uncovered > 0);
}

#[test]
fn full_coverage_local_clarity_equals_global_clarity() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let recipe_global = AdjustmentRecipe {
        detail: Some(Detail {
            clarity: 40.0,
            ..Default::default()
        }),
        ..Default::default()
    };

    let recipe_local = AdjustmentRecipe {
        masks: vec![full_coverage_mask(LocalDetail {
            clarity: 40.0,
            ..Default::default()
        })],
        ..Default::default()
    };

    // Preview test
    let f_glob = session
        .render(&recipe_global, None, PreviewStage::Drag)
        .unwrap();
    let f_loc = session
        .render(&recipe_local, None, PreviewStage::Drag)
        .unwrap();
    let mut max_diff_prev = 0;
    for (a, b) in f_glob.bytes.iter().zip(f_loc.bytes.iter()) {
        max_diff_prev = max_diff_prev.max((*a as i32 - *b as i32).abs());
    }
    assert!(max_diff_prev <= 1, "preview: full coverage local clarity must equal global clarity within 1 level (got {max_diff_prev})");

    // Export test
    let exp_glob = apply_recipe_with_orientation(&buf, &recipe_global, None, 1).unwrap();
    let exp_loc = apply_recipe_with_orientation(&buf, &recipe_local, None, 1).unwrap();
    let d_glob = exp_glob.as_rgb8().unwrap();
    let d_loc = exp_loc.as_rgb8().unwrap();
    let mut max_diff_exp = 0;
    for (a, b) in d_glob.iter().zip(d_loc.iter()) {
        max_diff_exp = max_diff_exp.max((*a as i32 - *b as i32).abs());
    }
    assert!(max_diff_exp <= 1, "export: full coverage local clarity must equal global clarity within 1 level (got {max_diff_exp})");
}

#[test]
fn full_coverage_local_texture_equals_global_texture() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let recipe_global = AdjustmentRecipe {
        detail: Some(Detail {
            texture: 35.0,
            ..Default::default()
        }),
        ..Default::default()
    };

    let recipe_local = AdjustmentRecipe {
        masks: vec![full_coverage_mask(LocalDetail {
            texture: 35.0,
            ..Default::default()
        })],
        ..Default::default()
    };

    let f_glob = session
        .render(&recipe_global, None, PreviewStage::Drag)
        .unwrap();
    let f_loc = session
        .render(&recipe_local, None, PreviewStage::Drag)
        .unwrap();
    let mut max_diff = 0;
    for (a, b) in f_glob.bytes.iter().zip(f_loc.bytes.iter()) {
        max_diff = max_diff.max((*a as i32 - *b as i32).abs());
    }
    assert!(
        max_diff <= 1,
        "full coverage local texture must equal global texture within 1 level (got {max_diff})"
    );
}

#[test]
fn full_coverage_local_dehaze_equals_global_dehaze() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let recipe_global = AdjustmentRecipe {
        detail: Some(Detail {
            dehaze: 30.0,
            ..Default::default()
        }),
        ..Default::default()
    };

    let recipe_local = AdjustmentRecipe {
        masks: vec![full_coverage_mask(LocalDetail {
            dehaze: 30.0,
            ..Default::default()
        })],
        ..Default::default()
    };

    let f_glob = session
        .render(&recipe_global, None, PreviewStage::Drag)
        .unwrap();
    let f_loc = session
        .render(&recipe_local, None, PreviewStage::Drag)
        .unwrap();
    let mut max_diff = 0;
    for (a, b) in f_glob.bytes.iter().zip(f_loc.bytes.iter()) {
        max_diff = max_diff.max((*a as i32 - *b as i32).abs());
    }
    assert!(
        max_diff <= 1,
        "full coverage local dehaze must equal global dehaze within 1 level (got {max_diff})"
    );
}

#[test]
fn local_and_global_clarity_add() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    // mask 30 + global 20 equals global 50, within 1 level
    let recipe_sum = AdjustmentRecipe {
        detail: Some(Detail {
            clarity: 50.0,
            ..Default::default()
        }),
        ..Default::default()
    };

    let recipe_split = AdjustmentRecipe {
        detail: Some(Detail {
            clarity: 20.0,
            ..Default::default()
        }),
        masks: vec![full_coverage_mask(LocalDetail {
            clarity: 30.0,
            ..Default::default()
        })],
        ..Default::default()
    };

    let f_sum = session
        .render(&recipe_sum, None, PreviewStage::Drag)
        .unwrap();
    let f_split = session
        .render(&recipe_split, None, PreviewStage::Drag)
        .unwrap();
    let mut max_diff = 0;
    for (a, b) in f_sum.bytes.iter().zip(f_split.bytes.iter()) {
        max_diff = max_diff.max((*a as i32 - *b as i32).abs());
    }
    assert!(
        max_diff <= 1,
        "mask 30 + global 20 must equal global 50 within 1 level (got {max_diff})"
    );
}

#[test]
fn local_sharpness_adds_to_the_global_amount() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    // mask 40 + global 30 equals global amount 90 (40 * 1.5 + 30 = 90), within 1 level
    let recipe_sum = AdjustmentRecipe {
        detail: Some(Detail {
            sharpen_amount: 90.0,
            ..Default::default()
        }),
        ..Default::default()
    };

    let recipe_split = AdjustmentRecipe {
        detail: Some(Detail {
            sharpen_amount: 30.0,
            ..Default::default()
        }),
        masks: vec![full_coverage_mask(LocalDetail {
            sharpness: 40.0,
            ..Default::default()
        })],
        ..Default::default()
    };

    let f_sum = session
        .render(&recipe_sum, None, PreviewStage::Drag)
        .unwrap();
    let f_split = session
        .render(&recipe_split, None, PreviewStage::Drag)
        .unwrap();
    let mut max_diff = 0;
    for (a, b) in f_sum.bytes.iter().zip(f_split.bytes.iter()) {
        max_diff = max_diff.max((*a as i32 - *b as i32).abs());
    }
    assert!(
        max_diff <= 1,
        "mask 40 + global 30 must equal global amount 90 within 1 level (got {max_diff})"
    );
}

#[test]
fn half_opacity_gives_half_the_change() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let recipe_base = AdjustmentRecipe::default();
    let mut mask_full = full_coverage_mask(LocalDetail {
        clarity: 40.0,
        ..Default::default()
    });
    mask_full.opacity = 1.0;
    let recipe_full = AdjustmentRecipe {
        masks: vec![mask_full],
        ..Default::default()
    };

    let mut mask_half = full_coverage_mask(LocalDetail {
        clarity: 40.0,
        ..Default::default()
    });
    mask_half.opacity = 0.5;
    let recipe_half = AdjustmentRecipe {
        masks: vec![mask_half],
        ..Default::default()
    };

    let f_base = session
        .render(&recipe_base, None, PreviewStage::Drag)
        .unwrap();
    let f_full = session
        .render(&recipe_full, None, PreviewStage::Drag)
        .unwrap();
    let f_half = session
        .render(&recipe_half, None, PreviewStage::Drag)
        .unwrap();

    let mut max_diff = 0.0f32;
    for i in 0..64 * 64 * 4 {
        if i % 4 == 3 {
            continue; // alpha
        }
        let b = f_base.bytes[i] as f32;
        let full = f_full.bytes[i] as f32;
        let half = f_half.bytes[i] as f32;

        let full_change = full - b;
        let half_change = half - b;
        let expected_half = full_change * 0.5;
        let diff = (half_change - expected_half).abs();
        max_diff = max_diff.max(diff);
    }
    assert!(
        max_diff <= 1.0,
        "half opacity must give half the change within 1 level (got {max_diff})"
    );
}

#[test]
fn a_range_mask_does_not_move_with_its_own_dehaze() {
    // Synthetic 16x16 buffer with alternating midtone and shadow blocks
    let mut buf = LinearBuffer::new(16, 16);
    for (i, px) in buf.data.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let v = if i % 2 == 0 { 0.25 } else { 0.04 };
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
    let image = ImageBuffer::Rgb8 {
        width: 16,
        height: 16,
        data: buf.data.iter().map(|&v| (v.sqrt() * 255.0) as u8).collect(),
    };
    let session = PreviewSession::new(&image).unwrap();

    // Range mask selecting only midtones ([0.4, 0.6]), with exposure +1.0
    let make_recipe = |dehaze: f32, exposure: f32| AdjustmentRecipe {
        masks: vec![Mask {
            id: "range_m".into(),
            name: "Luminance Range".into(),
            kind: MaskKind::LuminanceRange {
                low: 0.15,
                high: 0.25,
                feather: 0.01,
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure,
                ..Default::default()
            },
            detail: LocalDetail {
                dehaze,
                ..Default::default()
            },
            strokes: Vec::new(),
            components: Vec::new(),
        }],
        ..Default::default()
    };

    let recipe_dehaze100_exp1 = make_recipe(100.0, 1.0);
    let recipe_dehaze100_exp0 = make_recipe(100.0, 0.0);

    let exp1 = apply_recipe_with_orientation(&image, &recipe_dehaze100_exp1, None, 1).unwrap();
    let exp0 = apply_recipe_with_orientation(&image, &recipe_dehaze100_exp0, None, 1).unwrap();
    let d1 = exp1.as_rgb8().unwrap();
    let d0 = exp0.as_rgb8().unwrap();

    let prev1 = session
        .render(&recipe_dehaze100_exp1, None, PreviewStage::Drag)
        .unwrap();
    let prev0 = session
        .render(&recipe_dehaze100_exp0, None, PreviewStage::Drag)
        .unwrap();

    // Shadow pixel (index 1) in base frame has sqrt(0.04) = 0.20, which is covered by [0.15, 0.25].
    // If the mask reads base B, coverage is 1.0 so exposure +1.0 doubles the pixel, making d1 > d0.
    // If the mask read composite C, dehaze 100 crushes the pixel below 0.15, coverage drops to 0,
    // and d1 == d0!
    assert!(
        d1[3] > d0[3],
        "export: shadow pixel with dehaze 100 must still receive exposure adjustment from base coverage"
    );
    assert!(
        prev1.bytes[4] > prev0.bytes[4],
        "preview: shadow pixel with dehaze 100 must still receive exposure adjustment from base coverage"
    );
}

#[test]
fn local_detail_follows_a_crop_and_a_rotation() {
    let buf = test_image(80, 80);
    let session = PreviewSession::new(&buf).unwrap();

    // Mask at [0.25, 0.75] which falls into the crop under 90 deg rotation
    let mask = Mask {
        id: "quadrant_mask".into(),
        name: "Quadrant Mask".into(),
        kind: MaskKind::Radial {
            center: [0.25, 0.75],
            radius_x: 0.2,
            radius_y: 0.2,
            feather: 0.1,
            angle: 0.0,
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            clarity: 50.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    // Crop top-left quadrant: [0.0, 0.0, 0.5, 0.5] and rotate 90 deg clockwise
    let geom = Geometry {
        crop: Some(NormalizedCrop {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        }),
        rotate: 90,
        ..Default::default()
    };

    let recipe_plain = AdjustmentRecipe {
        geometry: Some(geom.clone()),
        ..Default::default()
    };

    let recipe_masked = AdjustmentRecipe {
        geometry: Some(geom),
        masks: vec![mask],
        ..Default::default()
    };

    let f_plain = session
        .render(&recipe_plain, None, PreviewStage::Drag)
        .unwrap();
    let f_masked = session
        .render(&recipe_masked, None, PreviewStage::Drag)
        .unwrap();

    assert_eq!(f_plain.width, f_masked.width);
    assert_eq!(f_plain.height, f_masked.height);
    assert_ne!(
        f_plain.bytes, f_masked.bytes,
        "masked render must alter pixels in cropped/rotated frame"
    );
}

#[test]
fn export_and_preview_agree_on_local_detail() {
    let buf = test_image(120, 80);
    let session = PreviewSession::new(&buf).unwrap();

    let mask = Mask {
        id: "det_mask".into(),
        name: "Detail Mask".into(),
        kind: MaskKind::Linear {
            start: [0.2, 0.2],
            end: [0.8, 0.8],
        },
        invert: false,
        opacity: 0.9,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: 0.3,
            contrast: 5.0,
            ..Default::default()
        },
        detail: LocalDetail {
            clarity: 25.0,
            texture: 20.0,
            dehaze: 15.0,
            sharpness: 30.0,
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    let recipe = AdjustmentRecipe {
        masks: vec![mask],
        ..Default::default()
    };

    let f_full = session.render(&recipe, None, PreviewStage::Full).unwrap();
    let exp = apply_recipe_with_orientation(&buf, &recipe, None, 1).unwrap();
    let d_exp = exp.as_rgb8().unwrap();

    let mut sum_diff = 0.0;
    let mut max_diff = 0;
    let n_pixels = (f_full.width * f_full.height) as usize;

    for i in 0..n_pixels {
        for c in 0..3 {
            let p = f_full.bytes[i * 4 + c];
            let e = d_exp[i * 3 + c];
            let diff = (p as i32 - e as i32).abs();
            sum_diff += diff as f64;
            max_diff = max_diff.max(diff);
        }
    }

    let mean_diff = sum_diff / (n_pixels * 3) as f64;
    assert!(
        mean_diff <= 1.0,
        "mean difference between preview and export must be <= 1.0 (got {mean_diff:.3})"
    );
    assert!(
        max_diff <= 2,
        "max difference between preview and export must be <= 2 (got {max_diff})"
    );
}

#[test]
fn moving_exposure_recomputes_no_local_detail_and_one_mask_recomputes_alone() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let mask1 = Mask {
        id: "m1".into(),
        name: "Mask 1".into(),
        kind: MaskKind::Linear {
            start: [0.0, 0.0],
            end: [0.5, 0.5],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            clarity: 20.0,
            dehaze: 10.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    let mask2 = Mask {
        id: "m2".into(),
        name: "Mask 2".into(),
        kind: MaskKind::Linear {
            start: [0.5, 0.5],
            end: [1.0, 1.0],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            clarity: 30.0,
            texture: 15.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    let mut recipe = AdjustmentRecipe {
        masks: vec![mask1.clone(), mask2.clone()],
        ..Default::default()
    };

    // First render: both masks compute their local detail frames
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let c0 = session.detail_computes();
    assert_eq!(c0, 2, "initial render computes 2 local detail frames");

    // Moving exposure recomputes no local detail
    recipe.exposure = 0.5;
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.detail_computes(),
        c0,
        "moving exposure must not recompute any local detail frames"
    );

    // Moving Mask 1's Clarity recomputes only that mask alone
    recipe.masks[0].detail.clarity = 40.0;
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.detail_computes(),
        c0 + 1,
        "moving mask 1's clarity must recompute only that mask alone (+1 computation)"
    );
}

#[test]
fn a_deleted_mask_releases_its_local_detail_frames() {
    let buf = test_image(64, 64);
    let session = PreviewSession::new(&buf).unwrap();

    let mask1 = Mask {
        id: "m1".into(),
        name: "Mask 1".into(),
        kind: MaskKind::Linear {
            start: [0.0, 0.0],
            end: [0.5, 0.5],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            clarity: 20.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    let mask2 = Mask {
        id: "m2".into(),
        name: "Mask 2".into(),
        kind: MaskKind::Linear {
            start: [0.5, 0.5],
            end: [1.0, 1.0],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments::default(),
        detail: LocalDetail {
            texture: 30.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
    };

    let mut recipe = AdjustmentRecipe {
        masks: vec![mask1, mask2],
        ..Default::default()
    };

    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(
        session.cached_local_detail_count(),
        4,
        "2 masks across 2 stages must cache 4 entries"
    );

    // Delete mask 2
    recipe.masks.truncate(1);
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(
        session.cached_local_detail_count(),
        2,
        "deleted mask must release its cached local detail frames (only mask 1 left across 2 stages = 2)"
    );
}
