//! Editing engine acceptance tests moved from `phototools-core` in RC-1.

#[path = "../../core/tests/fixtures/mod.rs"]
mod fixtures;

use fixtures::Fixtures;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// ED-4 — Preview renderer and session in core
// ---------------------------------------------------------------------------

#[test]
fn decode_tables_match_the_exact_conversion_for_every_code() {
    use rollchrome_core::media::edit::pipeline::{srgb_to_linear, u16_to_linear, u8_to_linear};

    for code in 0..=255u8 {
        let expected = srgb_to_linear(code as f32 / 255.0);
        let table_val = u8_to_linear(code);
        assert_eq!(
            table_val.to_bits(),
            expected.to_bits(),
            "u8 table mismatch at code {code}: expected {expected}, got {table_val}"
        );
    }

    for code in 0..=65535u16 {
        let expected = srgb_to_linear(code as f32 / 65535.0);
        let table_val = u16_to_linear(code);
        assert_eq!(
            table_val.to_bits(),
            expected.to_bits(),
            "u16 table mismatch at code {code}: expected {expected}, got {table_val}"
        );
    }
}

#[test]
fn rendering_never_changes_the_cached_proxies() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, Lut, LutRef, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 1600,
        height: 1200,
        data: (0..1600 * 1200 * 3).map(|i| (i % 256) as u8).collect(),
    };

    let session = PreviewSession::new(&buf).unwrap();

    // Snapshot the linear proxy buffers bit-for-bit before rendering
    let drag_snapshot = session.drag_linear().data.clone();
    let settle_snapshot = session.settle_linear().data.clone();

    let cube_content = "\
TITLE \"Test\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    let lut = Lut::from_cube("test.cube", cube_content.as_bytes()).unwrap();
    let lut_ref = LutRef {
        name: "test.cube".to_string(),
        sha256: lut.sha256.clone(),
    };

    let recipes_with_lut = [
        AdjustmentRecipe {
            exposure: 1.5,
            contrast: 25.0,
            highlights: -30.0,
            shadows: 20.0,
            saturation: 15.0,
            lut: Some(lut_ref.clone()),
            ..Default::default()
        },
        AdjustmentRecipe {
            exposure: -2.0,
            contrast: -15.0,
            temperature: 2000.0,
            tint: 15.0,
            vibrance: -40.0,
            lut: Some(lut_ref),
            ..Default::default()
        },
    ];

    for recipe in &recipes_with_lut {
        let drag_frame = session
            .render(recipe, Some(&lut), PreviewStage::Drag)
            .unwrap();
        assert!(drag_frame.width > 0);
        let settle_frame = session
            .render(recipe, Some(&lut), PreviewStage::Settle)
            .unwrap();
        assert!(settle_frame.width > 0);
    }

    let recipe_no_lut = AdjustmentRecipe {
        exposure: 0.8,
        contrast: 10.0,
        saturation: 5.0,
        ..Default::default()
    };
    let drag_frame = session
        .render(&recipe_no_lut, None, PreviewStage::Drag)
        .unwrap();
    assert!(drag_frame.width > 0);
    let settle_frame = session
        .render(&recipe_no_lut, None, PreviewStage::Settle)
        .unwrap();
    assert!(settle_frame.width > 0);

    // Verify bit-for-bit invariance of both cached linear proxy buffers
    assert_eq!(
        session.drag_linear().data,
        drag_snapshot,
        "rendering must never mutate the cached drag linear proxy in place"
    );
    assert_eq!(
        session.settle_linear().data,
        settle_snapshot,
        "rendering must never mutate the cached settle linear proxy in place"
    );
}

#[test]
fn the_preview_matches_the_export_renderer_within_one_code() {
    use rollchrome_core::media::edit::{
        apply_recipe, render_rgba_frame, AdjustmentRecipe, ImageBuffer, LinearBuffer, Lut, LutRef,
    };

    let w = 64;
    let h = 64;
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let r = ((x * 4) % 256) as u8;
            let g = ((y * 4) % 256) as u8;
            let b = (((x + y) * 2) % 256) as u8;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    let proxy_img = ImageBuffer::Rgb8 {
        width: w as u32,
        height: h as u32,
        data,
    };

    let cube_content = "\
TITLE \"Test Cube\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.1 0.05
0.05 0.9 0.1
0.95 0.95 0.2
0.1 0.05 0.8
0.85 0.2 0.85
0.2 0.85 0.9
1.0 1.0 1.0
";
    let lut = Lut::from_cube("test.cube", cube_content.as_bytes()).unwrap();
    let recipe = AdjustmentRecipe {
        exposure: 0.4,
        highlights: -20.0,
        shadows: 15.0,
        whites: 10.0,
        blacks: -10.0,
        brightness: 5.0,
        contrast: 12.0,
        saturation: 10.0,
        vibrance: -8.0,
        temperature: 15.0,
        tint: -10.0,
        hue: 12.0,
        curves: Some(rollchrome_core::media::edit::ToneCurves {
            luma: vec![
                rollchrome_core::media::edit::CurvePoint::new(0.0, 0.0),
                rollchrome_core::media::edit::CurvePoint::new(0.25, 0.20),
                rollchrome_core::media::edit::CurvePoint::new(0.75, 0.80),
                rollchrome_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
            red: vec![
                rollchrome_core::media::edit::CurvePoint::new(0.0, 0.0),
                rollchrome_core::media::edit::CurvePoint::new(0.5, 0.55),
                rollchrome_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
            green: vec![
                rollchrome_core::media::edit::CurvePoint::new(0.0, 0.0),
                rollchrome_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
            blue: vec![
                rollchrome_core::media::edit::CurvePoint::new(0.0, 0.0),
                rollchrome_core::media::edit::CurvePoint::new(0.5, 0.45),
                rollchrome_core::media::edit::CurvePoint::new(1.0, 1.0),
            ],
        }),
        hsl: Some(rollchrome_core::media::edit::HslAdjustments {
            red: rollchrome_core::media::edit::HslBand {
                hue: 15.0,
                saturation: 20.0,
                luminance: 10.0,
            },
            green: rollchrome_core::media::edit::HslBand {
                hue: -10.0,
                saturation: -15.0,
                luminance: 5.0,
            },
            blue: rollchrome_core::media::edit::HslBand {
                hue: 20.0,
                saturation: 25.0,
                luminance: -10.0,
            },
            ..Default::default()
        }),
        grading: Some(rollchrome_core::media::edit::ColorGrading {
            shadows: rollchrome_core::media::edit::ColorWheel {
                hue: 200.0,
                saturation: 0.3,
                luminance: -0.1,
            },
            midtones: rollchrome_core::media::edit::ColorWheel {
                hue: 45.0,
                saturation: 0.2,
                luminance: 0.05,
            },
            highlights: rollchrome_core::media::edit::ColorWheel {
                hue: 60.0,
                saturation: 0.25,
                luminance: 0.1,
            },
            global: rollchrome_core::media::edit::ColorWheel {
                hue: 30.0,
                saturation: 0.15,
                luminance: -0.05,
            },
            blending: 50.0,
            balance: 10.0,
        }),
        lut: Some(LutRef {
            name: "test.cube".into(),
            sha256: lut.sha256.clone(),
        }),
        lut_intensity: 0.85,
        ..Default::default()
    };

    let exact_out = apply_recipe(&proxy_img, &recipe, Some(&lut)).unwrap();
    let exact_rgb = exact_out.as_rgb8().unwrap();

    let proxy_lin = LinearBuffer::from_image_buffer(&proxy_img);
    let preview_frame = render_rgba_frame(&proxy_lin, &recipe, Some(&lut)).unwrap();

    assert_eq!(preview_frame.width, proxy_img.width());
    assert_eq!(preview_frame.height, proxy_img.height());
    assert_eq!(preview_frame.bytes.len(), w * h * 4);

    for i in 0..(w * h) {
        let exact_r = exact_rgb[i * 3] as i32;
        let exact_g = exact_rgb[i * 3 + 1] as i32;
        let exact_b = exact_rgb[i * 3 + 2] as i32;

        let prev_r = preview_frame.bytes[i * 4] as i32;
        let prev_g = preview_frame.bytes[i * 4 + 1] as i32;
        let prev_b = preview_frame.bytes[i * 4 + 2] as i32;
        let prev_a = preview_frame.bytes[i * 4 + 3];

        assert_eq!(prev_a, 255, "Alpha channel must be 255 at pixel {i}");
        assert!(
            (exact_r - prev_r).abs() <= 1,
            "Red channel at pixel {i} differs by > 1: exact {exact_r} vs preview {prev_r}"
        );
        assert!(
            (exact_g - prev_g).abs() <= 1,
            "Green channel at pixel {i} differs by > 1: exact {exact_g} vs preview {prev_r}"
        );
        assert!(
            (exact_b - prev_b).abs() <= 1,
            "Blue channel at pixel {i} differs by > 1: exact {exact_b} vs preview {prev_b}"
        );
    }
}

#[test]
fn a_preview_refuses_a_recipe_whose_lut_is_missing() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, LutRef, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 10,
        height: 10,
        data: vec![128; 300],
    };
    let session = PreviewSession::new(&buf).unwrap();

    let recipe = AdjustmentRecipe {
        lut: Some(LutRef {
            name: "missing.cube".into(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        }),
        ..Default::default()
    };

    let err = session
        .render(&recipe, None, PreviewStage::Drag)
        .unwrap_err();
    assert!(
        err.to_string().contains("no LUT was provided"),
        "expected missing LUT refusal, got: {err}"
    );
}

#[test]
fn a_preview_frame_is_rgba_of_the_proxy_size() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 3000,
        height: 2000,
        data: vec![100; 3000 * 2000 * 3],
    };
    let session = PreviewSession::new(&buf).unwrap();
    let recipe = AdjustmentRecipe::default();

    let drag = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(drag.width, 1280);
    assert_eq!(drag.height, 853);
    assert_eq!(drag.bytes.len(), 1280 * 853 * 4);
    assert!(drag.bytes.as_chunks::<4>().0.iter().all(|px| px[3] == 255));

    let settle = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(settle.width, 2560);
    assert_eq!(settle.height, 1706);
    assert_eq!(settle.bytes.len(), 2560 * 1706 * 4);
    assert!(settle
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .all(|px| px[3] == 255));
}

/// The frame the Edit view decodes carries the histogram of exactly the pixels it
/// carries, at the offsets `PREVIEW_FRAME_HEADER_LEN` documents (ED-15).
#[test]
fn binary_preview_ipc_header_encodes_histograms_and_clipping_accurately() {
    use rollchrome_core::media::edit::{
        encode_preview_frame, AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage,
        PREVIEW_FRAME_HEADER_LEN,
    };
    use rollchrome_core::media::histogram;

    // Left third black, middle third mid-grey, right third white: both clips present.
    let (w, h) = (90u32, 20u32);
    let data: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = match (i % w) / 30 {
                0 => 0u8,
                1 => 128,
                _ => 255,
            };
            [v, v, v]
        })
        .collect();
    let session = PreviewSession::new(&ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    })
    .unwrap();
    let frame = session
        .render(&AdjustmentRecipe::default(), None, PreviewStage::Drag)
        .unwrap();

    let bytes = encode_preview_frame(&frame);
    let word = |i: usize| u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());

    assert_eq!(bytes.len(), PREVIEW_FRAME_HEADER_LEN + (w * h * 4) as usize);
    assert_eq!((word(0), word(1), word(2)), (w, h, 1));

    // Counted again from the pixels that follow the header, not taken from the frame.
    let pixels = &bytes[PREVIEW_FRAME_HEADER_LEN..];
    let expected = histogram(pixels, w, h).unwrap();
    assert_eq!(word(3), w * h);
    assert_eq!(word(4), expected.highlight_clipped);
    assert_eq!(word(5), expected.shadow_clipped);
    assert_eq!(word(4), w * h / 3, "the white third is highlight-clipped");
    let to_stored: Vec<f32> = (6..12).map(|i| f32::from_bits(word(i))).collect();
    assert_eq!(to_stored, frame.to_stored.to_vec());
    assert_eq!(
        frame.to_stored,
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        "no geometry: the stored frame"
    );
    assert_eq!(word(5), w * h / 3, "the black third is shadow-clipped");

    for (c, channel) in [
        &expected.red,
        &expected.green,
        &expected.blue,
        &expected.luminance,
    ]
    .into_iter()
    .enumerate()
    {
        let decoded: Vec<u32> = (0..256).map(|b| word(12 + c * 256 + b)).collect();
        assert_eq!(&decoded, channel, "channel {c}");
    }
}

#[test]
fn benchmark_edit_preview() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, CurvePoint, ImageBuffer, Lut, LutRef, PreviewSession, PreviewStage,
        ToneCurves,
    };

    let w = 7360;
    let h = 4912;
    let total_pixels = w * h;
    let mut data = vec![128u8; total_pixels * 3];
    for (i, byte) in data.iter_mut().enumerate() {
        *byte = ((i * 31) % 256) as u8;
    }
    let img36 = ImageBuffer::Rgb8 {
        width: w as u32,
        height: h as u32,
        data,
    };

    let size = 33;
    let mut lut_data = Vec::with_capacity(size * size * size);
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rf = r as f32 / (size - 1) as f32;
                let gf = g as f32 / (size - 1) as f32;
                let bf = b as f32 / (size - 1) as f32;
                lut_data.push([
                    (rf * 0.95 + 0.02).clamp(0.0, 1.0),
                    (gf * 0.98 + 0.01).clamp(0.0, 1.0),
                    (bf * 0.90 + 0.05).clamp(0.0, 1.0),
                ]);
            }
        }
    }
    let lut33 = Lut {
        title: Some("Synthetic 33".into()),
        size,
        domain_min: [0.0, 0.0, 0.0],
        domain_max: [1.0, 1.0, 1.0],
        sha256: "synth33_sha256".into(),
        data: lut_data,
    };

    let t_session = Instant::now();
    let session = PreviewSession::new(&img36).unwrap();
    let session_elapsed = t_session.elapsed();

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        highlights: -25.0,
        shadows: 20.0,
        whites: 15.0,
        blacks: -10.0,
        brightness: 5.0,
        contrast: 15.0,
        saturation: 12.0,
        hue: 10.0,
        curves: Some(ToneCurves {
            luma: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.25, 0.18),
                CurvePoint::new(0.75, 0.82),
                CurvePoint::new(1.0, 1.0),
            ],
            red: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.5, 0.55),
                CurvePoint::new(1.0, 1.0),
            ],
            green: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
            blue: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.5, 0.45),
                CurvePoint::new(1.0, 1.0),
            ],
        }),
        hsl: Some(rollchrome_core::media::edit::HslAdjustments {
            red: rollchrome_core::media::edit::HslBand {
                hue: 15.0,
                saturation: 20.0,
                luminance: 10.0,
            },
            orange: rollchrome_core::media::edit::HslBand {
                hue: 5.0,
                saturation: 10.0,
                luminance: -5.0,
            },
            yellow: rollchrome_core::media::edit::HslBand {
                hue: -10.0,
                saturation: 15.0,
                luminance: 0.0,
            },
            green: rollchrome_core::media::edit::HslBand {
                hue: -15.0,
                saturation: -20.0,
                luminance: 5.0,
            },
            aqua: rollchrome_core::media::edit::HslBand {
                hue: 10.0,
                saturation: 10.0,
                luminance: -10.0,
            },
            blue: rollchrome_core::media::edit::HslBand {
                hue: 20.0,
                saturation: 25.0,
                luminance: -15.0,
            },
            purple: rollchrome_core::media::edit::HslBand {
                hue: -10.0,
                saturation: 15.0,
                luminance: 5.0,
            },
            magenta: rollchrome_core::media::edit::HslBand {
                hue: 10.0,
                saturation: -10.0,
                luminance: 0.0,
            },
        }),
        grading: Some(rollchrome_core::media::edit::ColorGrading {
            shadows: rollchrome_core::media::edit::ColorWheel {
                hue: 210.0,
                saturation: 0.25,
                luminance: -0.05,
            },
            midtones: rollchrome_core::media::edit::ColorWheel {
                hue: 45.0,
                saturation: 0.15,
                luminance: 0.05,
            },
            highlights: rollchrome_core::media::edit::ColorWheel {
                hue: 60.0,
                saturation: 0.20,
                luminance: 0.02,
            },
            global: rollchrome_core::media::edit::ColorWheel {
                hue: 30.0,
                saturation: 0.10,
                luminance: 0.0,
            },
            blending: 60.0,
            balance: 10.0,
        }),
        geometry: Some(rollchrome_core::media::edit::Geometry {
            crop: Some(rollchrome_core::media::edit::NormalizedCrop {
                x: 0.05,
                y: 0.05,
                width: 0.9,
                height: 0.9,
            }),
            straighten: 7.0,
            ..Default::default()
        }),
        lut: Some(LutRef {
            name: "synth33.cube".into(),
            sha256: lut33.sha256.clone(),
        }),
        lut_intensity: 0.8,
        vignette: Some(rollchrome_core::media::edit::Vignette {
            amount: -40.0,
            midpoint: 45.0,
            roundness: 20.0,
            feather: 60.0,
        }),
        grain: Some(rollchrome_core::media::edit::FilmGrain {
            amount: 35.0,
            size: 30.0,
            roughness: 60.0,
        }),
        looks: Some(rollchrome_core::media::edit::LookEffects {
            glow_amount: 30.0,
            glow_threshold: 65.0,
            glow_radius: 25.0,
            halation_amount: 25.0,
            halation_threshold: 75.0,
            halation_radius: 20.0,
            tone_mapper: Some("aces".into()),
        }),
        ..Default::default()
    };

    let _ = session
        .render(&recipe, Some(&lut33), PreviewStage::Drag)
        .unwrap();
    let _ = session
        .render(&recipe, Some(&lut33), PreviewStage::Settle)
        .unwrap();

    let mut drag_times = Vec::with_capacity(40);
    for _ in 0..40 {
        let t0 = Instant::now();
        let frame = session
            .render(&recipe, Some(&lut33), PreviewStage::Drag)
            .unwrap();
        drag_times.push(t0.elapsed());
        assert!(frame.width > 0);
    }
    drag_times.sort();
    let p95_drag_idx = ((drag_times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_drag = drag_times[p95_drag_idx];

    let mut straighten_drag_times = Vec::with_capacity(40);
    for i in 0..40 {
        let angle = -5.0 + (i as f32) * 0.25;
        let mut geom_recipe = recipe.clone();
        if let Some(g) = geom_recipe.geometry.as_mut() {
            g.straighten = angle;
        }
        let t0 = Instant::now();
        let frame = session
            .render(&geom_recipe, Some(&lut33), PreviewStage::Drag)
            .unwrap();
        straighten_drag_times.push(t0.elapsed());
        assert!(frame.width > 0);
    }
    straighten_drag_times.sort();
    let p95_straighten_drag_idx =
        ((straighten_drag_times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_straighten_drag = straighten_drag_times[p95_straighten_drag_idx];

    let mut settle_times = Vec::with_capacity(40);
    for _ in 0..40 {
        let t0 = Instant::now();
        let frame = session
            .render(&recipe, Some(&lut33), PreviewStage::Settle)
            .unwrap();
        settle_times.push(t0.elapsed());
        assert!(frame.width > 0);
    }
    settle_times.sort();
    let p95_settle_idx = ((settle_times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95_settle = settle_times[p95_settle_idx];

    // Round 3 budget (ED-19): the same recipe with four masks, each large and carrying
    // several local adjustments, so most pixels pay for more than one. Each mask may add at
    // most 1.5 ms to a drag frame and 5 ms to a settle frame.
    const MASKS: u32 = 4;
    let masked_recipe = {
        use rollchrome_core::media::edit::{LocalAdjustments, Mask, MaskKind};
        let adj = LocalAdjustments {
            exposure: 0.6,
            temperature: -15.0,
            highlights: -30.0,
            contrast: 12.0,
            saturation: 20.0,
            ..Default::default()
        };
        let mask = |id: &str, kind: MaskKind| Mask {
            id: id.into(),
            name: id.into(),
            kind,
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: adj,
            strokes: Vec::new(),
            components: Vec::new(),
            detail: Default::default(),
        };
        let mut r = recipe.clone();
        r.masks = vec![
            mask(
                "sky",
                MaskKind::Linear {
                    start: [0.5, 0.0],
                    end: [0.5, 0.6],
                },
            ),
            mask(
                "ground",
                MaskKind::Linear {
                    start: [0.5, 1.0],
                    end: [0.5, 0.4],
                },
            ),
            mask(
                "face",
                MaskKind::Radial {
                    center: [0.45, 0.5],
                    radius_x: 0.35,
                    radius_y: 0.3,
                    angle: 20.0,
                    feather: 0.6,
                },
            ),
            mask(
                "frame",
                MaskKind::Radial {
                    center: [0.6, 0.45],
                    radius_x: 0.4,
                    radius_y: 0.4,
                    angle: 0.0,
                    feather: 0.5,
                },
            ),
        ];
        assert_eq!(r.masks.len() as u32, MASKS);
        r
    };
    let time_stage = |stage: PreviewStage| {
        let _ = session.render(&masked_recipe, Some(&lut33), stage).unwrap();
        let mut t: Vec<Duration> = (0..40)
            .map(|_| {
                let t0 = Instant::now();
                let f = session.render(&masked_recipe, Some(&lut33), stage).unwrap();
                let e = t0.elapsed();
                assert!(f.width > 0);
                e
            })
            .collect();
        t.sort();
        t
    };
    let masked_drag = time_stage(PreviewStage::Drag);
    let masked_settle = time_stage(PreviewStage::Settle);
    let p95 = |t: &[Duration]| t[((t.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    let p95_masked_drag = p95(&masked_drag);
    let p95_masked_settle = p95(&masked_settle);

    // ED-21: painting. One brush mask; a 200-point stroke grows by one point per drag frame,
    // as a pointer move does, then the photograph settles. One mask's budget applies.
    let mut painted = recipe.clone();
    painted.masks = vec![{
        use rollchrome_core::media::edit::{LocalAdjustments, Mask, MaskKind, Stroke};
        Mask {
            id: "brush".into(),
            name: "Brush".into(),
            kind: MaskKind::Brush,
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure: 0.7,
                shadows: 25.0,
                ..Default::default()
            },
            strokes: vec![Stroke {
                points: vec![[0.1, 0.5]],
                radius: 0.04,
                feather: 0.5,
                flow: 1.0,
                erase: false,
            }],
            components: Vec::new(),
            detail: Default::default(),
        }
    }];
    let mut paint_times = Vec::with_capacity(200);
    for k in 0..200 {
        let t = k as f32 / 199.0;
        painted.masks[0].strokes[0]
            .points
            .push([0.1 + 0.8 * t, 0.5 + 0.25 * (t * 12.0).sin()]);
        let t0 = Instant::now();
        let f = session
            .render(&painted, Some(&lut33), PreviewStage::Drag)
            .unwrap();
        paint_times.push(t0.elapsed());
        assert!(f.width > 0);
    }
    paint_times.sort();
    let p95_paint = p95(&paint_times);
    let _ = session
        .render(&painted, Some(&lut33), PreviewStage::Settle)
        .unwrap();
    let mut brushed_settle: Vec<Duration> = (0..20)
        .map(|_| {
            let t0 = Instant::now();
            let f = session
                .render(&painted, Some(&lut33), PreviewStage::Settle)
                .unwrap();
            assert!(f.width > 0);
            t0.elapsed()
        })
        .collect();
    brushed_settle.sort();
    let p95_brushed_settle = p95(&brushed_settle);

    // Measure per-stage timing on 1440p settle frame proxy (2560x1708, ~4.37 MP)
    let settle_proxy = session.settle_linear();
    let w_s = settle_proxy.width;
    let _h_s = settle_proxy.height;
    let row_in_len = (w_s * 3) as usize;

    use rayon::prelude::*;

    // 1. Linear adjustments
    let t_lin = Instant::now();
    for _ in 0..10 {
        let h_val = recipe.highlights / 100.0;
        let s_val = recipe.shadows / 100.0;
        let w_val = recipe.whites / 100.0;
        let b_val = recipe.blacks / 100.0;
        let bright = recipe.brightness / 100.0;
        let bright_gamma = (-0.5 * bright).exp2();
        let c = recipe.contrast / 100.0;
        let gamma_minus_one = 0.5 * c;
        let b_exp = bright_gamma - 1.0;
        let c_scale = (1.0f32 / 0.18f32).powf(gamma_minus_one);
        let comb_exp = b_exp + bright_gamma * gamma_minus_one;
        let sat = recipe.saturation / 100.0;
        let vib = recipe.vibrance / 100.0;

        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let mut r = px[0] * 1.414;
                    let mut g = px[1] * 1.414;
                    let mut b = px[2] * 1.414;
                    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let delta_ev = if y >= 0.18 {
                        let w_h = if y >= 0.65 {
                            1.0
                        } else {
                            let t = (y - 0.18) * 2.1276;
                            t * t * (3.0 - 2.0 * t)
                        };
                        let w_w = if y >= 1.0 {
                            1.0
                        } else {
                            let t = (y - 0.18) * 1.2195;
                            t * t * (3.0 - 2.0 * t)
                        };
                        w_h * h_val + w_w * w_val
                    } else {
                        let w_s = if y <= 0.05 {
                            1.0
                        } else {
                            let t = (0.18 - y) * 7.6923;
                            t * t * (3.0 - 2.0 * t)
                        };
                        let w_b = if y <= 0.02 {
                            1.0
                        } else {
                            let t = (0.18 - y) * 6.25;
                            t * t * (3.0 - 2.0 * t)
                        };
                        w_s * s_val + w_b * b_val
                    };
                    let f = delta_ev.exp2();
                    r *= f;
                    g *= f;
                    b *= f;
                    let factor = (comb_exp * y.log2()).exp2() * c_scale;
                    r *= factor;
                    g *= factor;
                    b *= factor;
                    let max = r.max(g).max(b);
                    let min = r.min(g).min(b);
                    let cur_sat = if max > 1e-7 {
                        ((max - min) / max).min(1.0)
                    } else {
                        0.0
                    };
                    let k = ((1.0 + sat) * (1.0 + vib * (1.0 - cur_sat))).max(0.0);
                    let out_r = (y + k * (r - y)).max(0.0);
                    std::hint::black_box(out_r);
                }
            });
    }
    let lin_time = t_lin.elapsed() / 10;

    // 2. Oklab stage: Global Hue rotation and 8-band HSL in ONE pass
    let compiled_hsl = rollchrome_core::media::edit::hsl::CompiledHslTable::from_recipe(
        recipe.hsl.as_ref(),
        recipe.hue,
    );
    let t_hue = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let out = compiled_hsl.apply_linear_srgb(px[0], px[1], px[2]);
                    std::hint::black_box(out);
                }
            });
    }
    let hue_time = t_hue.elapsed() / 10;

    // 3. Display transfer (fast linear-to-sRGB)
    let t_disp = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let out = (
                        rollchrome_core::media::edit::preview::fast_linear_to_srgb(px[0]),
                        rollchrome_core::media::edit::preview::fast_linear_to_srgb(px[1]),
                        rollchrome_core::media::edit::preview::fast_linear_to_srgb(px[2]),
                    );
                    std::hint::black_box(out);
                }
            });
    }
    let disp_time = t_disp.elapsed() / 10;

    // 4. Tone Curves (evaluated once into 1024-entry LUTs, sampled per pixel)
    let curves_table =
        rollchrome_core::media::edit::curves::ToneCurvesTable::from_recipe(recipe.curves.as_ref());
    let t_curves = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let out = curves_table.apply(px[0], px[1], px[2]);
                    std::hint::black_box(out);
                }
            });
    }
    let curves_time = t_curves.elapsed() / 10;

    // 5. Colour Grading (1024-entry luma-indexed LUT precomputed from OkLCh tints and CDL mapping)
    let grading_table = rollchrome_core::media::edit::grading::CompiledGradingTable::from_recipe(
        recipe.grading.as_ref(),
    );
    let t_grading = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let out = grading_table.apply(px[0], px[1], px[2]);
                    std::hint::black_box(out);
                }
            });
    }
    let grading_time = t_grading.elapsed() / 10;

    // 6. 3D LUT Tetrahedral Sampling
    let t_lut = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let out = lut33.sample([px[0], px[1], px[2]]);
                    std::hint::black_box(out);
                }
            });
    }
    let lut_time = t_lut.elapsed() / 10;

    // 7. Vignette (smooth multiplicative gain in linear light post-geometry)
    let compiled_vignette = recipe.vignette.as_ref().unwrap().compile(w_s, _h_s);
    let t_vignette = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .enumerate()
            .for_each(|(y, row)| {
                let y_f = y as f32;
                let v = (y_f + 0.5 - compiled_vignette.yc) * compiled_vignette.inv_hy;
                let v2 = v * v;
                for (x, px) in row.as_chunks::<3>().0.iter().enumerate() {
                    let gain = compiled_vignette.gain_with_v(x as f32, v, v2);
                    let out = (px[0] * gain, px[1] * gain, px[2] * gain);
                    std::hint::black_box(out);
                }
            });
    }
    let vignette_time = t_vignette.elapsed() / 10;

    // 8. Film Grain (luminance only, display domain, parabolic envelope)
    let compiled_grain = recipe
        .grain
        .as_ref()
        .unwrap()
        .compile(&recipe.source_sha256);
    let inv_w_s = 1.0 / (w_s as f32);
    let t_grain = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .enumerate()
            .for_each(|(y, row)| {
                let y_f = y as f32;
                let py1 = (y_f + 0.5) * inv_w_s * compiled_grain.k_base;
                let rp = compiled_grain.row_params(py1);
                for (x, px) in row.as_chunks::<3>().0.iter().enumerate() {
                    let u = (x as f32 + 0.5) * inv_w_s;
                    let delta = compiled_grain.sample_delta_with_row(u, &rp, px[0], px[1], px[2]);
                    let out = (px[0] + delta, px[1] + delta, px[2] + delta);
                    std::hint::black_box(out);
                }
            });
    }
    let grain_time = t_grain.elapsed() / 10;

    // 9. Glow & Halation (reduced-resolution dual-filter blur pyramid)
    let looks_cfg = recipe.looks.as_ref().unwrap();
    let t_looks = Instant::now();
    for _ in 0..10 {
        let bloom =
            rollchrome_core::media::edit::looks::build_combined_bloom(settle_proxy, looks_cfg);
        std::hint::black_box(bloom);
    }
    let looks_time = t_looks.elapsed() / 10;

    // 10. ACES filmic tone mapper (Narkowicz curve on 4.37M pixels)
    let t_tm = Instant::now();
    for _ in 0..10 {
        settle_proxy
            .data
            .par_chunks_exact(row_in_len)
            .for_each(|row| {
                for px in row.as_chunks::<3>().0.iter() {
                    let r = rollchrome_core::media::edit::aces_narkowicz(px[0]);
                    let g = rollchrome_core::media::edit::aces_narkowicz(px[1]);
                    let b = rollchrome_core::media::edit::aces_narkowicz(px[2]);
                    std::hint::black_box((r, g, b));
                }
            });
    }
    let tone_mapper_time = t_tm.elapsed() / 10;

    let min_drag = drag_times[0];
    let median_drag = drag_times[drag_times.len() / 2];
    let min_settle = settle_times[0];
    let median_settle = settle_times[settle_times.len() / 2];

    println!(
        "Preview benchmark [36 MP 7360x4912, {} build]:",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!(
        "  Session creation (proxies + linearisation): {:?}",
        session_elapsed
    );
    println!(
        "  Colour Drag frame (720p 1280x854, cached geometry) over 40: min={:?}, median={:?}, p95={:?}",
        min_drag, median_drag, p95_drag
    );
    println!(
        "  Straighten Drag frame (720p 1280x854, recomputed geometry) over 40: p95={:?}",
        p95_straighten_drag
    );
    println!(
        "  Settle frame (1440p 2560x1708, cached geometry) over 40: min={:?}, median={:?}, p95={:?}",
        min_settle, median_settle, p95_settle
    );
    println!(
        "  With {MASKS} masks: drag median={:?} p95={:?}; settle median={:?} p95={:?}; per mask (medians): drag +{:?}, settle +{:?}",
        masked_drag[masked_drag.len() / 2],
        p95_masked_drag,
        masked_settle[masked_settle.len() / 2],
        p95_masked_settle,
        masked_drag[masked_drag.len() / 2].saturating_sub(median_drag) / MASKS,
        masked_settle[masked_settle.len() / 2].saturating_sub(median_settle) / MASKS,
    );
    println!(
        "  Painting a 200-point stroke: drag frame median={:?} p95={:?}; settle after it p95={:?}",
        paint_times[paint_times.len() / 2],
        p95_paint,
        p95_brushed_settle
    );
    println!("  Per-stage breakdown (1440p settle frame, 4.37 MP):");
    println!(
        "    1. Linear adjustments (tones, exposure, brightness, contrast, sat/vib): {:?}",
        lin_time
    );
    println!(
        "    2. Oklab stage (global hue + 8-band HSL in one pass on 4.37M pixels): {:?}",
        hue_time
    );
    println!(
        "    3. Display transfer (4096-entry fast linear-to-sRGB table): {:?}",
        disp_time
    );
    println!(
        "    4. Tone curves (1024-entry Fritsch-Carlson LUTs, Luma + RGB): {:?}",
        curves_time
    );
    println!(
        "    5. Colour grading (1024-entry luma LUT with OkLCh tints + CDL): {:?}",
        grading_time
    );
    println!(
        "    6. 3D LUT sampling & blending (33x33x33 tetrahedral interpolation): {:?}",
        lut_time
    );
    println!(
        "    7. Vignette (multiplicative gain in linear light on 4.37M pixels): {:?}",
        vignette_time
    );
    println!(
        "    8. Film grain (display-domain luminance noise on 4.37M pixels): {:?}",
        grain_time
    );
    println!(
        "    9. Glow & Halation (reduced-resolution dual-filter blur pyramid): {:?}",
        looks_time
    );
    println!(
        "    10. ACES filmic tone mapper (Narkowicz curve on 4.37M pixels): {:?}",
        tone_mapper_time
    );

    #[cfg(not(debug_assertions))]
    {
        assert!(
            session_elapsed <= Duration::from_millis(1000),
            "Session creation budget is <= 1000 ms, measured {:?}",
            session_elapsed
        );
        assert!(
            p95_drag <= Duration::from_millis(12),
            "Drag frame p95 budget is <= 12 ms, measured {:?}",
            p95_drag
        );
        assert!(
            p95_straighten_drag <= Duration::from_millis(25),
            "Straighten drag frame p95 budget is <= 25 ms, measured {:?}",
            p95_straighten_drag
        );
        assert!(
            p95_settle <= Duration::from_millis(40),
            "Settle frame p95 budget is <= 40 ms, measured {:?}",
            p95_settle
        );
        let drag_budget = Duration::from_millis(12) + Duration::from_micros(1500) * MASKS;
        let settle_budget = Duration::from_millis(40) + Duration::from_millis(5) * MASKS;
        assert!(
            p95_masked_drag <= drag_budget,
            "Drag frame p95 with {MASKS} masks: budget {drag_budget:?}, measured {p95_masked_drag:?}"
        );
        assert!(
            p95_masked_settle <= settle_budget,
            "Settle frame p95 with {MASKS} masks: budget {settle_budget:?}, measured {p95_masked_settle:?}"
        );
        // One brush mask: one mask's share of the budget, while painting and after.
        assert!(
            p95_paint <= Duration::from_micros(13_500),
            "Drag frame p95 while painting a 200-point stroke: budget 13.5 ms, measured {p95_paint:?}"
        );
        assert!(
            p95_brushed_settle <= Duration::from_millis(45),
            "Settle frame p95 with a painted mask: budget 45 ms, measured {p95_brushed_settle:?}"
        );
    }
}

#[test]
fn a_colour_only_change_does_not_recompute_geometry() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, Geometry, ImageBuffer, NormalizedCrop, PreviewSession, PreviewStage,
    };
    use std::sync::atomic::Ordering;

    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 100,
        data: vec![128u8; 100 * 100 * 3],
    };
    let session = PreviewSession::new(&buf).unwrap();

    let mut recipe = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            rotate: 90,
            straighten: 5.0,
            ..Default::default()
        }),
        exposure: 0.0,
        temperature: 0.0,
        ..Default::default()
    };

    // First render with geometry active: should compute geometry once
    let frame1 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(frame1.orientation, 1);
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Scrub exposure
    recipe.exposure = 0.5;
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Scrub temperature
    recipe.temperature = 25.0;
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Scrub contrast
    recipe.contrast = 20.0;
    let _ = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(session.geom_recompute_count.load(Ordering::Relaxed), 1);

    // Now change geometry: straighten angle from 5.0 to 7.0
    if let Some(geom) = recipe.geometry.as_mut() {
        geom.straighten = 7.0;
    }
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.geom_recompute_count.load(Ordering::Relaxed),
        2,
        "Changing geometry must trigger exactly one recomputation"
    );

    // Another colour scrub after new geometry
    recipe.shadows = 15.0;
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.geom_recompute_count.load(Ordering::Relaxed),
        2,
        "Subsequent colour changes must reuse the newly cached geometry"
    );
}

#[test]
fn preview_frame_without_geometry_reports_photo_orientation_and_with_geometry_reports_one() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, Geometry, ImageBuffer, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 60,
        data: vec![128u8; 100 * 60 * 3],
    };
    // Photo with orientation 6
    let session = PreviewSession::from_image_buffer_with_orientation(&buf, 6).unwrap();

    // 1. Recipe with NO geometry reports orientation 6
    let no_geom_recipe = AdjustmentRecipe::default();
    assert!(no_geom_recipe.geometry.is_none());
    let frame_no_geom = session
        .render(&no_geom_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(frame_no_geom.orientation, 6);

    // 2. Recipe with geometry (even identity / crop: null) reports orientation 1
    let with_geom_recipe = AdjustmentRecipe {
        geometry: Some(Geometry::default()),
        ..Default::default()
    };
    let frame_with_geom = session
        .render(&with_geom_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(frame_with_geom.orientation, 1);
}

#[test]
fn the_vignette_follows_the_crop() {
    use rollchrome_core::media::edit::{
        apply_recipe, AdjustmentRecipe, Geometry, ImageBuffer, NormalizedCrop, Vignette,
    };

    let w = 200;
    let h = 200;
    let img = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: vec![200u8; (w * h * 3) as usize],
    };

    // Crop the left half: x=0.0..0.5, y=0.0..1.0 -> output is 100x200
    // Vignette darkens corners strongly
    let recipe = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            ..Default::default()
        }),
        vignette: Some(Vignette {
            amount: -100.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
        }),
        ..Default::default()
    };

    let out = apply_recipe(&img, &recipe, None).unwrap();
    assert_eq!(out.width(), 100);
    assert_eq!(out.height(), 200);

    let data = out.as_rgb8().unwrap();
    // Center of the cropped output is at (50, 100).
    let center_idx = (100 * 100 + 50) * 3;
    let center_val = data[center_idx];

    // Corner of the cropped output is at (0, 0):
    let corner_idx = 0;
    let corner_val = data[corner_idx];

    // Right edge center of the cropped output is at (99, 100):
    let right_edge_idx = (100 * 100 + 99) * 3;
    let right_edge_val = data[right_edge_idx];

    // The vignette center must be at the center of the cropped half (center_val ~ 200)
    assert!(
        (center_val as i32 - 200).abs() <= 5,
        "Vignette center should have minimal attenuation: got {}",
        center_val
    );

    // The corner of the cropped half must be strongly darkened (< 50)
    assert!(
        corner_val < 50,
        "Vignette corner must be strongly darkened: got {}",
        corner_val
    );

    // In the original uncropped frame, (99, 100) was in the middle of the frame.
    // In the cropped frame, it is on the boundary edge, so it MUST be darkened relative to the center!
    assert!(
        right_edge_val < center_val - 20,
        "Edge of cropped frame must be darker than the center: center={}, edge={}",
        center_val,
        right_edge_val
    );
}

#[test]
fn preview_and_export_seed_grain_identically_for_an_unsaved_recipe() {
    use rollchrome_core::media::edit::{
        grain::seed_from_source_sha256, AdjustmentRecipe, FilmGrain, PreviewSession, PreviewStage,
    };
    use rollchrome_core::tools::edit::render_and_write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let image_path = dir.path().join("unsaved_test.jpg");

    let dyn_img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        120,
        80,
        image::Rgb([128, 128, 128]),
    ));
    dyn_img.save(&image_path).unwrap();

    // 1. Open preview session: hashes the file into session.source_sha256
    let session = PreviewSession::open(&image_path).unwrap();
    assert!(!session.source_sha256.is_empty());

    // 2. Create unsaved recipe: source_sha256 is explicitly empty
    let unsaved_recipe = AdjustmentRecipe {
        grain: Some(FilmGrain {
            amount: 60.0,
            size: 25.0,
            roughness: 50.0,
        }),
        source_sha256: String::new(),
        ..Default::default()
    };

    // 3. Preview render: falls back to session.source_sha256
    let preview_frame = session
        .render(&unsaved_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert!(preview_frame.width > 0);

    // 4. Export render: falls back to hashing the source file
    let out_dir = dir.path().join("out");
    let derived = render_and_write(&image_path, &unsaved_recipe, None, &out_dir, "test").unwrap();
    assert!(derived.output.exists());

    // Verify both resolved to the identical seed
    let expected_seed = seed_from_source_sha256(&session.source_sha256);
    assert_ne!(expected_seed, 0);

    let file_bytes = std::fs::read(&image_path).unwrap();
    let hash_bytes = <sha2::Sha256 as sha2::Digest>::digest(&file_bytes);
    let computed_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    assert_eq!(session.source_sha256, computed_hash);
    assert_eq!(seed_from_source_sha256(&computed_hash), expected_seed);
}

#[test]
fn untouched_vignette_and_grain_are_exact_identity() {
    use rollchrome_core::media::edit::{
        apply_recipe, AdjustmentRecipe, FilmGrain, ImageBuffer, Vignette,
    };

    let w = 32;
    let h = 32;
    let mut data8 = Vec::with_capacity((w * h * 3) as usize);
    let mut data16 = Vec::with_capacity((w * h * 3) as usize);
    for i in 0..(w * h) {
        let r8 = ((i * 37) % 256) as u8;
        let g8 = ((i * 73) % 256) as u8;
        let b8 = ((i * 109) % 256) as u8;
        data8.extend_from_slice(&[r8, g8, b8]);

        let r16 = ((i * 10007) % 65536) as u16;
        let g16 = ((i * 20011) % 65536) as u16;
        let b16 = ((i * 30013) % 65536) as u16;
        data16.extend_from_slice(&[r16, g16, b16]);
    }
    let buf8 = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: data8,
    };
    let buf16 = ImageBuffer::Rgb16 {
        width: w,
        height: h,
        data: data16,
    };

    // Recipe with default vignette & grain
    let recipe_defaults = AdjustmentRecipe {
        vignette: Some(Vignette::default()),
        grain: Some(FilmGrain::default()),
        ..Default::default()
    };
    assert!(recipe_defaults.is_identity());

    let out8_defaults = apply_recipe(&buf8, &recipe_defaults, None).unwrap();
    assert_eq!(
        out8_defaults, buf8,
        "Default vignette & grain on Rgb8 must be bit-exact identity"
    );

    let out16_defaults = apply_recipe(&buf16, &recipe_defaults, None).unwrap();
    assert_eq!(
        out16_defaults, buf16,
        "Default vignette & grain on Rgb16 must be bit-exact identity"
    );

    // Recipe with zero amount vignette & grain
    let recipe_zeros = AdjustmentRecipe {
        vignette: Some(Vignette {
            amount: 0.0,
            midpoint: 40.0,
            roundness: 20.0,
            feather: 80.0,
        }),
        grain: Some(FilmGrain {
            amount: 0.0,
            size: 50.0,
            roughness: 100.0,
        }),
        ..Default::default()
    };
    assert!(recipe_zeros.is_identity());

    let out8_zeros = apply_recipe(&buf8, &recipe_zeros, None).unwrap();
    assert_eq!(
        out8_zeros, buf8,
        "Zero-amount vignette & grain on Rgb8 must be bit-exact identity"
    );

    let out16_zeros = apply_recipe(&buf16, &recipe_zeros, None).unwrap();
    assert_eq!(
        out16_zeros, buf16,
        "Zero-amount vignette & grain on Rgb16 must be bit-exact identity"
    );
}

#[test]
fn vignette_attenuation_is_identical_on_preview_proxy_and_full_export() {
    use rollchrome_core::media::edit::{
        apply_recipe, AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage, Vignette,
    };

    let w = 2400;
    let h = 1600;
    let flat_buf = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: vec![128u8; (w * h * 3) as usize],
    };

    let vignette = Vignette {
        amount: -50.0,
        midpoint: 50.0,
        roundness: 0.0,
        feather: 50.0,
    };
    let recipe = AdjustmentRecipe {
        vignette: Some(vignette),
        ..Default::default()
    };

    // Full export
    let export_out = apply_recipe(&flat_buf, &recipe, None).unwrap();
    let export_data = export_out.as_rgb8().unwrap();

    // Preview proxy
    let session = PreviewSession::new(&flat_buf).unwrap();
    let drag_frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();

    // Sample normalized locations: center (0.5, 0.5), corner (0.05, 0.05), midpoint (0.25, 0.25)
    let test_points = [(0.5f32, 0.5f32), (0.05, 0.05), (0.25, 0.25), (0.1, 0.5)];
    for &(u, v) in &test_points {
        let ex_x = ((u * w as f32).round() as u32).min(w - 1);
        let ex_y = ((v * h as f32).round() as u32).min(h - 1);
        let ex_idx = ((ex_y * w + ex_x) * 3) as usize;
        let ex_r = export_data[ex_idx] as f32;

        let pr_x = ((u * drag_frame.width as f32).round() as u32).min(drag_frame.width - 1);
        let pr_y = ((v * drag_frame.height as f32).round() as u32).min(drag_frame.height - 1);
        let pr_idx = ((pr_y * drag_frame.width + pr_x) * 4) as usize;
        let pr_r = drag_frame.bytes[pr_idx] as f32;

        let diff = (ex_r - pr_r).abs();
        assert!(
            diff <= 1.5,
            "Vignette attenuation mismatch at ({u}, {v}): export={ex_r}, proxy={pr_r}, diff={diff}"
        );
    }
}

#[test]
fn grain_is_byte_identical_between_two_runs_of_the_same_photo() {
    use rollchrome_core::media::edit::{
        apply_recipe, AdjustmentRecipe, FilmGrain, ImageBuffer, PreviewSession, PreviewStage,
    };

    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 100,
        data: vec![128u8; 100 * 100 * 3],
    };
    let recipe = AdjustmentRecipe {
        source_sha256: "test_sha_deterministic".into(),
        grain: Some(FilmGrain {
            amount: 50.0,
            size: 25.0,
            roughness: 50.0,
        }),
        ..Default::default()
    };

    let run1 = apply_recipe(&buf, &recipe, None).unwrap();
    let run2 = apply_recipe(&buf, &recipe, None).unwrap();
    assert_eq!(
        run1, run2,
        "Export grain must be bit-exact identical between runs"
    );

    let session = PreviewSession::new(&buf).unwrap();
    let preview1 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let preview2 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        preview1.bytes, preview2.bytes,
        "Preview grain must be bit-exact identical between runs"
    );
}

#[test]
fn grain_evaluation_is_fully_deterministic_across_threads() {
    use rollchrome_core::media::edit::{apply_recipe, AdjustmentRecipe, FilmGrain, ImageBuffer};

    let w = 200;
    let h = 200;
    let buf = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: vec![128u8; (w * h * 3) as usize],
    };
    let recipe = AdjustmentRecipe {
        source_sha256: "thread_determinism_test".into(),
        grain: Some(FilmGrain {
            amount: 75.0,
            size: 30.0,
            roughness: 60.0,
        }),
        ..Default::default()
    };

    let pool1 = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let out_1_thread = pool1.install(|| apply_recipe(&buf, &recipe, None).unwrap());

    let pool8 = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();
    let out_8_threads = pool8.install(|| apply_recipe(&buf, &recipe, None).unwrap());

    assert_eq!(
        out_1_thread, out_8_threads,
        "Grain evaluation must produce byte-identical output across 1 and 8 threads"
    );
}

#[test]
fn grain_appearance_and_density_are_scale_independent_between_proxy_and_export() {
    use rollchrome_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, FilmGrain, ImageBuffer,
        PreviewSession, PreviewStage,
    };

    // Full export resolution: 2400x1600 (3:2)
    let w_exp = 2400;
    let h_exp = 1600;
    let exp_buf = ImageBuffer::Rgb8 {
        width: w_exp,
        height: h_exp,
        data: vec![128u8; (w_exp * h_exp * 3) as usize],
    };

    let recipe = AdjustmentRecipe {
        source_sha256: "scale_independence_test_seed".into(),
        grain: Some(FilmGrain {
            amount: 50.0,
            size: 40.0,
            roughness: 30.0,
        }),
        ..Default::default()
    };

    // 1. Full export
    let export_out = apply_recipe(&exp_buf, &recipe, None).unwrap();

    // 2. Preview proxy (drag proxy is 1280x853)
    let session = PreviewSession::new(&exp_buf).unwrap();
    let proxy_frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let w_pr = proxy_frame.width;
    let h_pr = proxy_frame.height;

    // Downscale export to proxy dimensions
    let export_downscaled = downscale_image_buffer(&export_out, w_pr.max(h_pr)).unwrap();
    assert_eq!(export_downscaled.width(), w_pr);
    assert_eq!(export_downscaled.height(), h_pr);

    let down_data = export_downscaled.as_rgb8().unwrap();
    let proxy_bytes = &proxy_frame.bytes;

    // Compare over a 4x4 grid of patches
    let patches_x = 4;
    let patches_y = 4;
    let patch_w = w_pr / patches_x;
    let patch_h = h_pr / patches_y;

    for py in 0..patches_y {
        for px in 0..patches_x {
            let start_x = px * patch_w;
            let start_y = py * patch_h;

            let mut sum_pr = 0.0f64;
            let mut sum_down = 0.0f64;
            let mut count = 0usize;

            for y in start_y..(start_y + patch_h) {
                for x in start_x..(start_x + patch_w) {
                    let pr_idx = ((y * w_pr + x) * 4) as usize;
                    let pr_y = 0.2126 * proxy_bytes[pr_idx] as f64
                        + 0.7152 * proxy_bytes[pr_idx + 1] as f64
                        + 0.0722 * proxy_bytes[pr_idx + 2] as f64;

                    let down_idx = ((y * w_pr + x) * 3) as usize;
                    let down_y = 0.2126 * down_data[down_idx] as f64
                        + 0.7152 * down_data[down_idx + 1] as f64
                        + 0.0722 * down_data[down_idx + 2] as f64;

                    sum_pr += pr_y;
                    sum_down += down_y;
                    count += 1;
                }
            }

            let mean_pr = sum_pr / count as f64;
            let mean_down = sum_down / count as f64;

            // Assert mean luminance within 1%
            let mean_diff_pct = (mean_pr - mean_down).abs() / mean_pr;
            assert!(
                mean_diff_pct <= 0.01,
                "Mean luminance diff {mean_diff_pct:.4} exceeds 1% at patch ({px}, {py}): pr={mean_pr}, down={mean_down}"
            );

            // Compute standard deviations
            let mut var_pr = 0.0f64;
            let mut var_down = 0.0f64;
            for y in start_y..(start_y + patch_h) {
                for x in start_x..(start_x + patch_w) {
                    let pr_idx = ((y * w_pr + x) * 4) as usize;
                    let pr_y = 0.2126 * proxy_bytes[pr_idx] as f64
                        + 0.7152 * proxy_bytes[pr_idx + 1] as f64
                        + 0.0722 * proxy_bytes[pr_idx + 2] as f64;

                    let down_idx = ((y * w_pr + x) * 3) as usize;
                    let down_y = 0.2126 * down_data[down_idx] as f64
                        + 0.7152 * down_data[down_idx + 1] as f64
                        + 0.0722 * down_data[down_idx + 2] as f64;

                    var_pr += (pr_y - mean_pr).powi(2);
                    var_down += (down_y - mean_down).powi(2);
                }
            }

            let std_pr = (var_pr / count as f64).sqrt();
            let std_down = (var_down / count as f64).sqrt();

            // Assert local standard deviation within 15%
            let std_diff_pct = (std_pr - std_down).abs() / std_pr.max(1e-5);
            assert!(
                std_diff_pct <= 0.15,
                "Std dev diff {std_diff_pct:.4} exceeds 15% at patch ({px}, {py}): pr={std_pr}, down={std_down}"
            );
        }
    }
}

#[test]
fn untouched_looks_are_exact_identity() {
    use rollchrome_core::media::edit::{apply_recipe, AdjustmentRecipe, ImageBuffer, LookEffects};

    let w = 32;
    let h = 32;
    let mut data8 = Vec::with_capacity((w * h * 3) as usize);
    let mut data16 = Vec::with_capacity((w * h * 3) as usize);
    for i in 0..(w * h) {
        let r8 = ((i * 37) % 256) as u8;
        let g8 = ((i * 73) % 256) as u8;
        let b8 = ((i * 109) % 256) as u8;
        data8.extend_from_slice(&[r8, g8, b8]);

        let r16 = ((i * 10007) % 65536) as u16;
        let g16 = ((i * 20011) % 65536) as u16;
        let b16 = ((i * 30013) % 65536) as u16;
        data16.extend_from_slice(&[r16, g16, b16]);
    }
    let buf8 = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: data8,
    };
    let buf16 = ImageBuffer::Rgb16 {
        width: w,
        height: h,
        data: data16,
    };

    let recipe = AdjustmentRecipe {
        looks: Some(LookEffects::default()),
        ..Default::default()
    };
    assert!(recipe.is_identity());

    let out8 = apply_recipe(&buf8, &recipe, None).unwrap();
    assert_eq!(
        out8, buf8,
        "Default looks on Rgb8 must be exact bit identity"
    );

    let out16 = apply_recipe(&buf16, &recipe, None).unwrap();
    assert_eq!(
        out16, buf16,
        "Default looks on Rgb16 must be exact bit identity"
    );
}

#[test]
fn tone_mapper_off_preserves_exact_identity() {
    use rollchrome_core::media::edit::{apply_recipe, AdjustmentRecipe, ImageBuffer, LookEffects};

    let buf = ImageBuffer::Rgb8 {
        width: 16,
        height: 16,
        data: vec![120u8; 16 * 16 * 3],
    };
    let recipe = AdjustmentRecipe {
        looks: Some(LookEffects {
            glow_amount: 0.0,
            glow_threshold: 70.0,
            glow_radius: 30.0,
            halation_amount: 0.0,
            halation_threshold: 80.0,
            halation_radius: 20.0,
            tone_mapper: None,
        }),
        ..Default::default()
    };
    assert!(recipe.is_identity());
    let out = apply_recipe(&buf, &recipe, None).unwrap();
    assert_eq!(out, buf, "Tone mapper off must preserve bit-exact identity");
}

#[test]
fn narkowicz_aces_compresses_highlights_monotonically_without_inversion() {
    use rollchrome_core::media::edit::aces_narkowicz;

    let mut prev = 0.0f32;
    for i in 0..2000 {
        let x = i as f32 * 0.05;
        let y = aces_narkowicz(x);
        assert!(
            y >= prev,
            "Monotonicity violation at x={x}: prev={prev}, y={y}"
        );
        assert!(y <= 1.0, "ACES output exceeded 1.0 at x={x}: y={y}");
        prev = y;
    }

    // Extreme high dynamic range highlights
    let y_100 = aces_narkowicz(100.0);
    let y_1000 = aces_narkowicz(1000.0);
    assert!((0.99..=1.0).contains(&y_100));
    assert!(y_1000 >= y_100 && y_1000 <= 1.0);
}

#[test]
fn aces_tone_mapper_places_mid_grey_where_the_doc_says() {
    use rollchrome_core::media::edit::aces_narkowicz;

    let y = aces_narkowicz(0.18);
    // Unscaled Narkowicz ACES maps 0.18 to ~0.2669 as documented in module docs
    assert!(
        (y - 0.2669).abs() < 0.005,
        "Mid-grey 0.18 should map to ~0.2669, got {y}"
    );
}

#[test]
fn glow_pyramid_executes_within_interactive_budget_during_drag() {
    use rollchrome_core::media::edit::{looks::build_combined_bloom, LinearBuffer, LookEffects};
    use std::time::Instant;

    let w = 1280;
    let h = 854;
    let mut data = vec![0.1f32; (w * h * 3) as usize];
    // Add specular highlight in center
    for y in 400..454 {
        for x in 600..680 {
            let idx = ((y * w + x) * 3) as usize;
            data[idx] = 2.5;
            data[idx + 1] = 2.5;
            data[idx + 2] = 2.5;
        }
    }
    let linear = LinearBuffer {
        width: w,
        height: h,
        data,
    };
    let looks = LookEffects {
        glow_amount: 50.0,
        glow_threshold: 60.0,
        glow_radius: 35.0,
        halation_amount: 40.0,
        halation_threshold: 70.0,
        halation_radius: 25.0,
        tone_mapper: None,
    };

    let t0 = Instant::now();
    let bloom = build_combined_bloom(&linear, &looks);
    let elapsed = t0.elapsed();

    assert!(bloom.is_some());
    // Interactive budget for 720p pyramid is <= 5 ms
    assert!(
        elapsed.as_millis() <= 5,
        "Glow pyramid took {:?}, exceeding 5 ms interactive drag budget",
        elapsed
    );
}

#[test]
fn glow_and_halation_decay_smoothly_and_scale_with_image_resolution() {
    use rollchrome_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, ImageBuffer, LookEffects,
        PreviewSession, PreviewStage,
    };

    let w = 7360;
    let h = 4912;
    let mut data = vec![30u8; (w * h * 3) as usize];

    // Place a bright highlight disc in center
    let cx = w / 2;
    let cy = h / 2;
    let r_disc = 200;
    for y in (cy - r_disc)..(cy + r_disc) {
        for x in (cx - r_disc)..(cx + r_disc) {
            let dx = (x as i32) - (cx as i32);
            let dy = (y as i32) - (cy as i32);
            if dx * dx + dy * dy <= (r_disc as i32) * (r_disc as i32) {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = 250;
                data[idx + 1] = 250;
                data[idx + 2] = 250;
            }
        }
    }
    let img36 = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    };

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        looks: Some(LookEffects {
            glow_amount: 35.0,
            glow_threshold: 60.0,
            glow_radius: 30.0,
            halation_amount: 30.0,
            halation_threshold: 70.0,
            halation_radius: 20.0,
            tone_mapper: None,
        }),
        ..Default::default()
    };

    // 1. Full 36 MP export
    let export_out = apply_recipe(&img36, &recipe, None).unwrap();

    // 2. Drag preview proxy (1280x854)
    let session = PreviewSession::new(&img36).unwrap();
    let proxy_frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let pr_w = proxy_frame.width;
    let pr_h = proxy_frame.height;

    // Downscale full export to proxy size
    let export_down = downscale_image_buffer(&export_out, pr_w.max(pr_h)).unwrap();
    assert_eq!(export_down.width(), pr_w);
    assert_eq!(export_down.height(), pr_h);

    let down_bytes = export_down.as_rgb8().unwrap();
    let pr_bytes = &proxy_frame.bytes;

    // Compare over sampling points across the image
    let mut max_diff = 0.0f32;
    let mut sum_diff = 0.0f64;
    let mut count = 0usize;

    for y in (0..pr_h).step_by(8) {
        for x in (0..pr_w).step_by(8) {
            let pr_idx = ((y * pr_w + x) * 4) as usize;
            let down_idx = ((y * pr_w + x) * 3) as usize;

            let diff_r = (pr_bytes[pr_idx] as f32 - down_bytes[down_idx] as f32).abs();
            let diff_g = (pr_bytes[pr_idx + 1] as f32 - down_bytes[down_idx + 1] as f32).abs();
            let diff_b = (pr_bytes[pr_idx + 2] as f32 - down_bytes[down_idx + 2] as f32).abs();

            let avg_diff = (diff_r + diff_g + diff_b) / 3.0;
            if avg_diff > max_diff {
                max_diff = avg_diff;
            }
            sum_diff += avg_diff as f64;
            count += 1;
        }
    }

    let mean_diff = (sum_diff / count as f64) as f32;
    // Mean delta E must be well within <= 1.5 code values
    assert!(
        mean_diff <= 1.5,
        "Mean delta E {mean_diff} exceeds 1.5 (max={max_diff})"
    );
}

// ---------------------------------------------------------------------------
// ED-19: masks
// ---------------------------------------------------------------------------

fn radial_mask(center: [f32; 2], radius: f32, exposure: f32) -> rollchrome_core::media::edit::Mask {
    use rollchrome_core::media::edit::{LocalAdjustments, Mask, MaskKind};
    Mask {
        id: "r".into(),
        name: "Radial".into(),
        kind: MaskKind::Radial {
            center,
            radius_x: radius,
            radius_y: radius,
            angle: 0.0,
            feather: 0.3,
        },
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

/// A grey frame with a smooth gradient, so misplacement would show, and a red marker
/// square centred at stored-normalised `marker`.
fn marked_frame(w: u32, h: u32, marker: [f32; 2]) -> rollchrome_core::media::edit::ImageBuffer {
    let (mx, my) = ((marker[0] * w as f32) as i64, (marker[1] * h as f32) as i64);
    let half = (w.max(h) / 60) as i64;
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if (x - mx).abs() <= half && (y - my).abs() <= half {
                data.extend_from_slice(&[230, 20, 20]);
            } else {
                let v = (80 + (x * 60 / w as i64) + (y * 30 / h as i64)) as u8;
                data.extend_from_slice(&[v, v, v]);
            }
        }
    }
    rollchrome_core::media::edit::ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    }
}

/// Centroid of the pixels where `weight` is positive, in frame pixels.
fn centroid(w: u32, h: u32, weight: impl Fn(usize) -> f32) -> (f32, f32) {
    let (mut sx, mut sy, mut sw) = (0.0f64, 0.0f64, 0.0f64);
    for y in 0..h {
        for x in 0..w {
            let k = weight((y * w + x) as usize).max(0.0) as f64;
            sx += k * (x as f64 + 0.5);
            sy += k * (y as f64 + 0.5);
            sw += k;
        }
    }
    assert!(sw > 0.0, "nothing to locate");
    ((sx / sw) as f32, (sy / sw) as f32)
}

/// The effect of a mask drawn over a marker lands on the marker, whatever the crop,
/// rotation, straighten, flip or EXIF orientation: masks are placed on the photograph's
/// content, not on the frame around it (ED-19, decision 2).
#[test]
fn a_mask_follows_the_crop_and_straighten() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, Geometry, NormalizedCrop, PreviewSession, PreviewStage,
    };

    let at = [0.3, 0.35];
    let img = marked_frame(1500, 1000, at);
    let geometries: Vec<(&str, Option<Geometry>, u32)> = vec![
        ("none", None, 1),
        (
            "crop",
            Some(Geometry {
                crop: Some(NormalizedCrop {
                    x: 0.1,
                    y: 0.2,
                    width: 0.6,
                    height: 0.7,
                }),
                ..Default::default()
            }),
            1,
        ),
        (
            "rotate 90",
            Some(Geometry {
                rotate: 90,
                ..Default::default()
            }),
            1,
        ),
        (
            "straighten 8 and flip",
            Some(Geometry {
                straighten: 8.0,
                flip_h: true,
                ..Default::default()
            }),
            1,
        ),
        ("EXIF orientation 6", Some(Geometry::default()), 6),
    ];

    for (name, geometry, orientation) in geometries {
        let session =
            PreviewSession::from_image_buffer_with_orientation(&img, orientation).unwrap();
        let base = AdjustmentRecipe {
            geometry: geometry.clone(),
            ..Default::default()
        };
        let masked = AdjustmentRecipe {
            masks: vec![radial_mask(at, 0.06, 1.5)],
            ..base.clone()
        };
        let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
        let lit = session.render(&masked, None, PreviewStage::Drag).unwrap();
        let (w, h) = (plain.width, plain.height);

        let marker = centroid(w, h, |i| {
            let p = &plain.bytes[i * 4..i * 4 + 3];
            if p[0] > 150 && p[1] < 90 {
                1.0
            } else {
                0.0
            }
        });
        let effect = centroid(w, h, |i| {
            let luma = |b: &[u8]| b[0] as f32 + b[1] as f32 + b[2] as f32;
            luma(&lit.bytes[i * 4..i * 4 + 3]) - luma(&plain.bytes[i * 4..i * 4 + 3])
        });
        let dist = ((marker.0 - effect.0).powi(2) + (marker.1 - effect.1).powi(2)).sqrt();
        assert!(
            dist < 3.0,
            "{name}: the mask's effect is centred at {effect:?}, the marker at {marker:?}"
        );
    }
}

/// Where no mask reaches, the frame is byte for byte what it is without masks; a mask
/// that switches a stage on (saturation here) must not disturb the pixels it misses.
#[test]
fn pixels_no_mask_covers_are_unchanged_by_it() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, PreviewSession, PreviewStage};

    let img = marked_frame(1200, 800, [0.8, 0.8]);
    let session = PreviewSession::new(&img).unwrap();
    let base = AdjustmentRecipe {
        exposure: 0.3,
        contrast: 12.0,
        ..Default::default()
    };
    let mut mask = radial_mask([0.5, 0.5], 0.1, 1.0);
    mask.adjustments.saturation = 40.0;
    mask.adjustments.contrast = -20.0;
    mask.adjustments.shadows = 30.0;
    let masked = AdjustmentRecipe {
        masks: vec![mask],
        ..base.clone()
    };

    for stage in [PreviewStage::Drag, PreviewStage::Settle] {
        let a = session.render(&base, None, stage).unwrap();
        let b = session.render(&masked, None, stage).unwrap();
        let (w, h) = (a.width as usize, a.height as usize);
        let mut changed_inside = 0;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                // Radius 0.1 of the long edge: 0.1 of the width, 0.15 of the height.
                let r = (((u - 0.5) / 0.1).powi(2) + ((v - 0.5) / 0.15).powi(2)).sqrt();
                if r > 1.01 {
                    assert_eq!(
                        a.bytes[i..i + 4],
                        b.bytes[i..i + 4],
                        "outside the mask at ({x}, {y})"
                    );
                } else if r < 0.5 && a.bytes[i..i + 3] != b.bytes[i..i + 3] {
                    changed_inside += 1;
                }
            }
        }
        assert!(changed_inside > 0, "the mask changed nothing inside");
    }
}

/// A mask whose adjustments are all zero renders exactly as no mask at all, preview and
/// export.
#[test]
fn a_mask_with_zero_adjustments_changes_nothing() {
    use rollchrome_core::media::edit::{
        apply_recipe, AdjustmentRecipe, PreviewSession, PreviewStage,
    };

    let img = marked_frame(600, 400, [0.5, 0.5]);
    let base = AdjustmentRecipe {
        exposure: 0.4,
        ..Default::default()
    };
    let masked = AdjustmentRecipe {
        masks: vec![radial_mask([0.5, 0.5], 0.3, 0.0)],
        ..base.clone()
    };
    assert_eq!(
        apply_recipe(&img, &base, None).unwrap(),
        apply_recipe(&img, &masked, None).unwrap()
    );
    let session = PreviewSession::new(&img).unwrap();
    assert_eq!(
        session
            .render(&base, None, PreviewStage::Settle)
            .unwrap()
            .bytes,
        session
            .render(&masked, None, PreviewStage::Settle)
            .unwrap()
            .bytes
    );
    assert!(masked.masks[0].is_identity());
}

/// The export, rendered at full resolution and downscaled, matches the preview of the same
/// masked recipe (the ED-16 measure: mean difference ≤ 1.5 code values).
#[test]
fn export_matches_preview_with_masks_within_delta_e_1_5() {
    use rollchrome_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, LocalAdjustments, Mask, MaskKind,
        PreviewSession, PreviewStage,
    };

    let img = marked_frame(4000, 2667, [0.6, 0.4]);
    let sky = Mask {
        id: "sky".into(),
        name: "Sky".into(),
        kind: MaskKind::Linear {
            start: [0.5, 0.1],
            end: [0.5, 0.6],
        },
        invert: false,
        opacity: 0.8,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: -0.8,
            temperature: -20.0,
            highlights: -40.0,
            saturation: 25.0,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
        detail: Default::default(),
    };
    let mut face = radial_mask([0.6, 0.4], 0.12, 0.7);
    face.adjustments.contrast = 15.0;
    face.adjustments.shadows = 30.0;
    face.adjustments.tint = 8.0;

    let recipe = AdjustmentRecipe {
        exposure: 0.2,
        contrast: 10.0,
        masks: vec![sky, face],
        ..Default::default()
    };

    let export = apply_recipe(&img, &recipe, None).unwrap();
    let session = PreviewSession::new(&img).unwrap();
    let preview = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let down = downscale_image_buffer(&export, preview.width.max(preview.height)).unwrap();
    assert_eq!(
        (down.width(), down.height()),
        (preview.width, preview.height)
    );
    let down = down.as_rgb8().unwrap();

    let (mut sum, mut n) = (0.0f64, 0usize);
    for i in 0..(preview.width * preview.height) as usize {
        let d: f32 = (0..3)
            .map(|c| (preview.bytes[i * 4 + c] as f32 - down[i * 3 + c] as f32).abs())
            .sum::<f32>()
            / 3.0;
        sum += d as f64;
        n += 1;
    }
    let mean = (sum / n as f64) as f32;
    assert!(
        mean <= 1.5,
        "mean difference {mean} between preview and export exceeds 1.5"
    );
}

/// A v2 sidecar, written before masks existed, loads with none; a v3 one with masks
/// round-trips through the sidecar unchanged.
#[test]
fn a_v2_sidecar_loads_with_no_masks_and_masks_round_trip_in_v3() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("masked.jpg", 64, 48);
    let sidecar = sidecar_path(&img);
    std::fs::write(
        &sidecar,
        serde_json::json!({"version": 2, "source_sha256": "x", "exposure": 0.5,
            "temperature": 0.0, "tint": 0.0, "highlights": 0.0, "shadows": 0.0, "contrast": 0.0,
            "saturation": 0.0, "vibrance": 0.0, "lut": null, "lut_intensity": 1.0})
        .to_string(),
    )
    .unwrap();
    let v2 = load_recipe(&sidecar).unwrap();
    assert!(v2.masks.is_empty());

    let recipe = AdjustmentRecipe {
        masks: vec![radial_mask([0.4, 0.6], 0.2, -0.5)],
        ..v2
    };
    save_recipe(&img, &recipe).unwrap();
    let back = load_recipe(&sidecar).unwrap();
    assert_eq!(back.version, 3);
    assert_eq!(back.masks, recipe.masks);
}

/// The coverage the overlay shows is where the mask acts: brightest where the effect is
/// strongest, absent where the frame is untouched, through the same geometry (ED-20).
#[test]
fn the_mask_overlay_shows_where_the_mask_acts() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, Geometry, NormalizedCrop, PreviewSession, PreviewStage,
    };

    let img = marked_frame(1500, 1000, [0.3, 0.35]);
    let session = PreviewSession::from_image_buffer_with_orientation(&img, 6).unwrap();
    let mut mask = radial_mask([0.3, 0.35], 0.08, 1.2);
    mask.invert = true;
    let base = AdjustmentRecipe {
        geometry: Some(Geometry {
            straighten: 5.0,
            crop: Some(NormalizedCrop {
                x: 0.05,
                y: 0.1,
                width: 0.8,
                height: 0.85,
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let masked = AdjustmentRecipe {
        masks: vec![mask],
        ..base.clone()
    };

    let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
    let lit = session.render(&masked, None, PreviewStage::Drag).unwrap();
    let (w, h, coverage) = session
        .mask_coverage(&masked, "r", PreviewStage::Drag)
        .unwrap();
    assert_eq!((w, h), (lit.width, lit.height));
    // The frame tells the view the same map the coverage was computed through.
    assert_ne!(lit.to_stored, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    let (mut strong, mut strong_changed) = (0, 0);
    for (i, &cov) in coverage.iter().enumerate() {
        let changed = lit.bytes[i * 4..i * 4 + 3] != plain.bytes[i * 4..i * 4 + 3];
        if cov == 0 {
            assert!(
                !changed,
                "pixel {i} changed where the overlay shows nothing"
            );
        } else if cov > 128 {
            strong += 1;
            if changed {
                strong_changed += 1;
            }
        }
    }
    // An inverted radial: most of the frame is strongly covered, and +1.2 EV changes it.
    assert!(
        strong > (w * h / 2) as usize,
        "only {strong} strongly covered pixels"
    );
    assert!(
        strong_changed * 100 >= strong * 99,
        "{strong_changed} of {strong} strongly covered pixels changed"
    );
    assert!(session
        .mask_coverage(&masked, "missing", PreviewStage::Drag)
        .is_err());
}

// ---------------------------------------------------------------------------
// ED-21: brush
// ---------------------------------------------------------------------------

fn brush_mask(
    strokes: Vec<rollchrome_core::media::edit::Stroke>,
    exposure: f32,
) -> rollchrome_core::media::edit::Mask {
    use rollchrome_core::media::edit::{LocalAdjustments, Mask, MaskKind};
    Mask {
        id: "b".into(),
        name: "Brush".into(),
        kind: MaskKind::Brush,
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure,
            ..Default::default()
        },
        strokes,
        components: Vec::new(),
        detail: Default::default(),
    }
}

fn brush_stroke(
    points: Vec<[f32; 2]>,
    radius: f32,
    erase: bool,
) -> rollchrome_core::media::edit::Stroke {
    rollchrome_core::media::edit::Stroke {
        points,
        radius,
        feather: 0.5,
        flow: 1.0,
        erase,
    }
}

/// A painted mask looks the same in the export, drawn at full size (capped at 4096 on the
/// long edge) and downscaled, as in the preview: the ED-16 measure, mean ≤ 1.5 code values.
#[test]
fn brush_strokes_rasterise_identically_at_proxy_and_export_size() {
    use rollchrome_core::media::edit::{
        apply_recipe, downscale_image_buffer, AdjustmentRecipe, PreviewSession, PreviewStage,
    };

    // Wider than the export raster cap, so the capped path is the one measured.
    let img = marked_frame(5000, 3333, [0.5, 0.5]);
    let wave: Vec<[f32; 2]> = (0..80)
        .map(|i| {
            let t = i as f32 / 79.0;
            [0.1 + 0.8 * t, 0.5 + 0.2 * (t * 9.0).sin()]
        })
        .collect();
    let recipe = AdjustmentRecipe {
        masks: vec![brush_mask(
            vec![
                brush_stroke(wave.clone(), 0.04, false),
                brush_stroke(vec![[0.3, 0.3], [0.7, 0.7]], 0.02, true),
            ],
            1.2,
        )],
        ..Default::default()
    };

    let export = apply_recipe(&img, &recipe, None).unwrap();
    let session = PreviewSession::new(&img).unwrap();
    let preview = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let down = downscale_image_buffer(&export, preview.width.max(preview.height)).unwrap();
    let down = down.as_rgb8().unwrap();
    let mut sum = 0.0f64;
    let n = (preview.width * preview.height) as usize;
    for i in 0..n {
        let d: f32 = (0..3)
            .map(|c| (preview.bytes[i * 4 + c] as f32 - down[i * 3 + c] as f32).abs())
            .sum::<f32>()
            / 3.0;
        sum += d as f64;
    }
    let mean = (sum / n as f64) as f32;
    assert!(
        mean <= 1.5,
        "mean difference {mean} between preview and export exceeds 1.5"
    );
}

/// A dab painted over a marker lands on the marker under crop, rotation and orientation,
/// as the shapes do: strokes are in the stored frame too.
#[test]
fn a_brushed_mask_follows_the_crop_and_rotation() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, Geometry, NormalizedCrop, PreviewSession, PreviewStage,
    };

    let at = [0.62, 0.3];
    let img = marked_frame(1500, 1000, at);
    for (name, geometry, orientation) in [
        ("none", None, 1),
        (
            "crop and rotate",
            Some(Geometry {
                rotate: 270,
                crop: Some(NormalizedCrop {
                    x: 0.0,
                    y: 0.1,
                    width: 0.9,
                    height: 0.8,
                }),
                ..Default::default()
            }),
            1,
        ),
        ("EXIF orientation 8", Some(Geometry::default()), 8),
    ] {
        let session =
            PreviewSession::from_image_buffer_with_orientation(&img, orientation).unwrap();
        let base = AdjustmentRecipe {
            geometry: geometry.clone(),
            ..Default::default()
        };
        let painted = AdjustmentRecipe {
            masks: vec![brush_mask(vec![brush_stroke(vec![at], 0.05, false)], 1.5)],
            ..base.clone()
        };
        let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
        let lit = session.render(&painted, None, PreviewStage::Drag).unwrap();
        let (w, h) = (plain.width, plain.height);
        let marker = centroid(w, h, |i| {
            let p = &plain.bytes[i * 4..i * 4 + 3];
            if p[0] > 150 && p[1] < 90 {
                1.0
            } else {
                0.0
            }
        });
        let effect = centroid(w, h, |i| {
            let luma = |b: &[u8]| b[0] as f32 + b[1] as f32 + b[2] as f32;
            luma(&lit.bytes[i * 4..i * 4 + 3]) - luma(&plain.bytes[i * 4..i * 4 + 3])
        });
        let dist = ((marker.0 - effect.0).powi(2) + (marker.1 - effect.1).powi(2)).sqrt();
        assert!(
            dist < 3.0,
            "{name}: effect at {effect:?}, marker at {marker:?}"
        );
    }
}

/// Painting a stroke frame by frame draws each new segment once, not the stroke again, and
/// erasing it all away renders exactly as no mask (ED-21).
#[test]
fn painting_draws_each_new_segment_once_and_erasing_restores_exactly() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, PreviewSession, PreviewStage};

    let img = marked_frame(1200, 800, [0.5, 0.5]);
    let session = PreviewSession::new(&img).unwrap();
    let path: Vec<[f32; 2]> = (0..50).map(|i| [0.2 + 0.012 * i as f32, 0.4]).collect();

    let mut recipe = AdjustmentRecipe {
        masks: vec![brush_mask(
            vec![brush_stroke(path[..1].to_vec(), 0.03, false)],
            1.0,
        )],
        ..Default::default()
    };
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let start = session.brush_segments_drawn();
    for k in 2..=path.len() {
        recipe.masks[0].strokes[0].points = path[..k].to_vec();
        session.render(&recipe, None, PreviewStage::Drag).unwrap();
    }
    assert_eq!(session.brush_segments_drawn() - start, path.len() - 1);

    let without = session
        .render(&AdjustmentRecipe::default(), None, PreviewStage::Settle)
        .unwrap();
    let mut erase = brush_stroke(path.clone(), 0.06, true);
    erase.feather = 0.0;
    recipe.masks[0].strokes.push(erase);
    let erased = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(
        erased.bytes, without.bytes,
        "an erased stroke must leave no trace"
    );
}

// ---------------------------------------------------------------------------
// ED-23: automatic masks, as stored
// ---------------------------------------------------------------------------

/// A stored automatic mask acts where its pixels say, in the stored frame, through the
/// geometry like every mask; a damaged one is refused rather than masking the wrong place.
#[test]
fn a_stored_automatic_mask_acts_where_it_covers_and_a_damaged_one_is_refused() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, AutoTarget, Geometry, LocalAdjustments, Mask, MaskKind, PreviewSession,
        PreviewStage, StoredRaster,
    };

    // Covers a square around the marker at (0.3, 0.35) of the stored frame.
    let (mw, mh) = (300u32, 200u32);
    let data: Vec<u8> = (0..mw * mh)
        .map(|i| {
            // Cell centres, so the square is centred on the marker exactly.
            let (x, y) = (
                ((i % mw) as f32 + 0.5) / mw as f32,
                ((i / mw) as f32 + 0.5) / mh as f32,
            );
            if (x - 0.3).abs() < 0.08 && (y - 0.35).abs() < 0.12 {
                255
            } else {
                0
            }
        })
        .collect();
    let stored = StoredRaster::encode(mw, mh, &data, "test").unwrap();
    let mask = |raster: StoredRaster| Mask {
        id: "auto".into(),
        name: "Subject".into(),
        kind: MaskKind::Auto {
            target: AutoTarget::Subject,
            mask: raster,
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: 1.5,
            ..Default::default()
        },
        strokes: Vec::new(),
        components: Vec::new(),
        detail: Default::default(),
    };

    let img = marked_frame(1500, 1000, [0.3, 0.35]);
    for (name, geometry, orientation) in [
        ("none", None, 1),
        (
            "rotate and orientation 6",
            Some(Geometry {
                rotate: 90,
                ..Default::default()
            }),
            6,
        ),
    ] {
        let session =
            PreviewSession::from_image_buffer_with_orientation(&img, orientation).unwrap();
        let base = AdjustmentRecipe {
            geometry: geometry.clone(),
            ..Default::default()
        };
        let masked = AdjustmentRecipe {
            masks: vec![mask(stored.clone())],
            ..base.clone()
        };
        let plain = session.render(&base, None, PreviewStage::Drag).unwrap();
        let lit = session.render(&masked, None, PreviewStage::Drag).unwrap();
        let (w, h) = (plain.width, plain.height);
        let marker = centroid(w, h, |i| {
            let p = &plain.bytes[i * 4..i * 4 + 3];
            if p[0] > 150 && p[1] < 90 {
                1.0
            } else {
                0.0
            }
        });
        // Which pixels changed clearly, not by how much: the frame brightens left to right,
        // so weighting by the gain, or counting the faint fringe where only the brighter side
        // moves a code value, would pull the centre to the right.
        let effect = centroid(w, h, |i| {
            let luma = |b: &[u8]| b[0] as f32 + b[1] as f32 + b[2] as f32;
            let d = luma(&lit.bytes[i * 4..i * 4 + 3]) - luma(&plain.bytes[i * 4..i * 4 + 3]);
            if d > 30.0 {
                1.0
            } else {
                0.0
            }
        });
        let dist = ((marker.0 - effect.0).powi(2) + (marker.1 - effect.1).powi(2)).sqrt();
        assert!(
            dist < 4.0,
            "{name}: effect at {effect:?}, marker at {marker:?}"
        );
    }

    let mut damaged = stored.clone();
    damaged.png.truncate(damaged.png.len() / 2);
    let session = PreviewSession::new(&img).unwrap();
    let recipe = AdjustmentRecipe {
        masks: vec![mask(damaged)],
        ..Default::default()
    };
    assert!(session.render(&recipe, None, PreviewStage::Drag).is_err());
}

#[test]
fn the_full_stage_renders_every_source_pixel() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, ImageBuffer, PreviewSession, PreviewStage,
    };
    let w = 800u32;
    let h = 600u32;
    let data = vec![128u8; (w * h * 3) as usize];
    let img = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    };
    let session = PreviewSession::new(&img).unwrap();
    let recipe = AdjustmentRecipe::default();
    let frame = session.render(&recipe, None, PreviewStage::Full).unwrap();
    assert_eq!(frame.width, w);
    assert_eq!(frame.height, h);
    assert_eq!(frame.bytes.len(), (w * h * 4) as usize);
}

#[test]
fn the_full_stage_fits_the_canvas_limit() {
    use rollchrome_core::media::edit::{fit_full_edge, FULL_MAX_EDGE};
    // Within limits: unmodified
    assert_eq!(fit_full_edge(4000, 3000, FULL_MAX_EDGE), (4000, 3000));
    assert_eq!(fit_full_edge(8192, 5461, FULL_MAX_EDGE), (8192, 5461));
    // Over limit: downscaled to FULL_MAX_EDGE on the long edge
    assert_eq!(fit_full_edge(10000, 5000, 8192), (8192, 4096));
    assert_eq!(fit_full_edge(6000, 12000, 8192), (4096, 8192));
    // Zero max_edge: unmodified
    assert_eq!(fit_full_edge(10000, 5000, 0), (10000, 5000));
}

#[test]
fn opening_a_session_and_rendering_drag_and_settle_decodes_nothing_more() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, PreviewSession, PreviewStage};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jpg");
    image::RgbImage::new(400, 300).save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();
    assert_eq!(session.full_decodes(), 0);

    let recipe = AdjustmentRecipe::default();
    let _ = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(session.full_decodes(), 0);

    let _ = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    assert_eq!(session.full_decodes(), 0);
}

#[test]
fn the_full_buffer_is_decoded_once() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, PreviewSession, PreviewStage};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jpg");
    image::RgbImage::new(400, 300).save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();
    assert_eq!(session.full_decodes(), 0);

    let recipe1 = AdjustmentRecipe::default();
    let _ = session.render(&recipe1, None, PreviewStage::Full).unwrap();
    assert_eq!(session.full_decodes(), 1);

    let recipe2 = AdjustmentRecipe {
        exposure: 1.0,
        ..Default::default()
    };
    let _ = session.render(&recipe2, None, PreviewStage::Full).unwrap();
    assert_eq!(session.full_decodes(), 1);
}

#[test]
fn the_full_stage_applies_crop_and_masks_as_settle_does() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, Geometry, ImageBuffer, LocalAdjustments, Mask, MaskKind, NormalizedCrop,
        PreviewSession, PreviewStage,
    };
    // Create an image with uniform colour patches (1200 x 800)
    let w = 1200u32;
    let h = 800u32;
    let mut data = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 3) as usize;
            let patch_x = x / 300;
            let patch_y = y / 200;
            data[idx] = (patch_x * 60 + 40) as u8;
            data[idx + 1] = (patch_y * 50 + 50) as u8;
            data[idx + 2] = ((patch_x + patch_y) * 30 + 60) as u8;
        }
    }
    let img = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    };
    let session = PreviewSession::new(&img).unwrap();

    let crop = NormalizedCrop {
        x: 0.1,
        y: 0.1,
        width: 0.8,
        height: 0.8,
    };
    let geom = Geometry {
        crop: Some(crop),
        rotate: 0,
        straighten: 0.0,
        flip_h: false,
        flip_v: false,
        aspect: Some("original".into()),
    };
    let radial = Mask {
        id: "radial1".into(),
        name: "Radial".into(),
        kind: MaskKind::Radial {
            center: [0.5, 0.5],
            radius_x: 0.3,
            radius_y: 0.3,
            angle: 0.0,
            feather: 0.5,
        },
        invert: false,
        opacity: 100.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: 1.0,
            ..Default::default()
        },
        strokes: vec![],
        components: Vec::new(),
        detail: Default::default(),
    };
    let recipe = AdjustmentRecipe {
        geometry: Some(geom),
        masks: vec![radial],
        ..Default::default()
    };

    let settle_frame = session.render(&recipe, None, PreviewStage::Settle).unwrap();
    let full_frame = session.render(&recipe, None, PreviewStage::Full).unwrap();

    // Check corresponding positions across an 8x8 grid
    for gy in 1..9 {
        for gx in 1..9 {
            let u = gx as f32 / 10.0;
            let v = gy as f32 / 10.0;
            let sx =
                ((u * settle_frame.width as f32) as usize).min(settle_frame.width as usize - 1);
            let sy =
                ((v * settle_frame.height as f32) as usize).min(settle_frame.height as usize - 1);
            let fx = ((u * full_frame.width as f32) as usize).min(full_frame.width as usize - 1);
            let fy = ((v * full_frame.height as f32) as usize).min(full_frame.height as usize - 1);

            let s_idx = (sy * settle_frame.width as usize + sx) * 4;
            let f_idx = (fy * full_frame.width as usize + fx) * 4;

            for c in 0..3 {
                let s_val = settle_frame.bytes[s_idx + c] as i32;
                let f_val = full_frame.bytes[f_idx + c] as i32;
                assert!(
                    (s_val - f_val).abs() <= 2,
                    "Channel {c} mismatch at ({u}, {v}): settle={s_val}, full={f_val}"
                );
            }
        }
    }
}
