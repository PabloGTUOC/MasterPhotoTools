//! Integration tests for eyedropper white balance, auto tone,
//! and 8-band black & white mixer (RP-4).

use rollchrome_core::media::edit::hsl::BlackAndWhite;
use rollchrome_core::media::edit::pipeline::{apply_recipe, AdjustmentRecipe, ImageBuffer};
use rollchrome_core::media::edit::preview::{PreviewSession, PreviewStage};
use rollchrome_core::media::edit::{ColorGrading, ColorWheel};

fn make_rgb8_image(width: u32, height: u32, fill: [u8; 3]) -> ImageBuffer {
    let size = (width as usize) * (height as usize) * 3;
    let mut data = Vec::with_capacity(size);
    for _ in 0..(width * height) {
        data.extend_from_slice(&fill);
    }
    ImageBuffer::Rgb8 {
        width,
        height,
        data,
    }
}

fn compute_display_luma_percentile(bytes: &[u8], percentile: f64) -> f32 {
    let mut hist = [0u32; 256];
    let n_pixels = bytes.len() / 4;
    for px in bytes.as_chunks::<4>().0 {
        let y = (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32)
            .round()
            .clamp(0.0, 255.0) as usize;
        hist[y] += 1;
    }
    let target_count = ((n_pixels as f64 * percentile).ceil() as u32).max(1);
    let mut acc = 0;
    for (val, &cnt) in hist.iter().enumerate() {
        acc += cnt;
        if acc >= target_count {
            return val as f32;
        }
    }
    255.0
}

// ---------------------------------------------------------------------------
// 1. picking_a_grey_patch_with_a_cast_makes_it_neutral
// ---------------------------------------------------------------------------
#[test]
fn picking_a_grey_patch_with_a_cast_makes_it_neutral() {
    // A grey patch with a warm yellow/red cast: R=120, G=110, B=100
    let img = make_rgb8_image(64, 64, [120, 110, 100]);
    let session = PreviewSession::from_image_buffer(&img).expect("preview session creation");

    let pick = session
        .white_balance_at(0.5, 0.5)
        .expect("pick on warm grey patch");
    assert!(!pick.clamped, "test cast must fit within slider range");

    // Real render through pipeline with picked WB
    let recipe = AdjustmentRecipe {
        temperature: pick.temperature,
        tint: pick.tint,
        ..Default::default()
    };
    let frame = session
        .render(&recipe, None, PreviewStage::Drag)
        .expect("render drag frame");

    let px = &frame.bytes[..4];
    let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
    assert!(
        (r - g).abs() <= 2 && (g - b).abs() <= 2 && (r - b).abs() <= 2,
        "output must be neutral within 2 levels, got R={r}, G={g}, B={b}"
    );
}

// ---------------------------------------------------------------------------
// 2. picking_the_same_spot_again_gives_the_same_values_whatever_the_recipe
// ---------------------------------------------------------------------------
#[test]
fn picking_the_same_spot_again_gives_the_same_values_whatever_the_recipe() {
    let img = make_rgb8_image(64, 64, [120, 100, 90]);
    let session = PreviewSession::from_image_buffer(&img).expect("preview session creation");

    let pick1 = session
        .white_balance_at(0.5, 0.5)
        .expect("first pick with identity");

    // Render with strong edits (exposure, tint, contrast, etc.)
    let modified_recipe = AdjustmentRecipe {
        exposure: 1.5,
        temperature: -45.0,
        tint: 30.0,
        contrast: 25.0,
        saturation: -50.0,
        ..Default::default()
    };
    let _ = session
        .render(&modified_recipe, None, PreviewStage::Settle)
        .expect("render modified");

    // Second pick at the same spot must read the unadjusted settle linear buffer
    let pick2 = session
        .white_balance_at(0.5, 0.5)
        .expect("second pick with modified recipe");

    assert_eq!(
        pick1.temperature, pick2.temperature,
        "temperature pick must be identical regardless of recipe"
    );
    assert_eq!(
        pick1.tint, pick2.tint,
        "tint pick must be identical regardless of recipe"
    );
    assert_eq!(
        pick1.clamped, pick2.clamped,
        "clamped flag must be identical regardless of recipe"
    );
}

// ---------------------------------------------------------------------------
// 3. picking_beyond_the_slider_range_clamps_and_says_so
// ---------------------------------------------------------------------------
#[test]
fn picking_beyond_the_slider_range_clamps_and_says_so() {
    // Extreme blue patch requiring huge temperature shift beyond slider range [-100, +100]
    let img = make_rgb8_image(50, 50, [10, 128, 240]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let pick = session
        .white_balance_at(0.5, 0.5)
        .expect("pick should succeed with clamping");
    assert!(pick.clamped, "must flag clamped = true");
    assert_eq!(
        pick.temperature, 100.0,
        "temperature must clamp to upper limit 100.0"
    );
}

// ---------------------------------------------------------------------------
// 4. picking_a_clipped_or_black_spot_is_refused
// ---------------------------------------------------------------------------
#[test]
fn picking_a_clipped_or_black_spot_is_refused() {
    // Clipped highlight (255 in 8-bit sRGB maps to 1.0 >= 0.98 linear)
    let img_clipped = make_rgb8_image(50, 50, [255, 255, 255]);
    let session_clipped = PreviewSession::from_image_buffer(&img_clipped).unwrap();
    let err_clipped = session_clipped
        .white_balance_at(0.5, 0.5)
        .expect_err("must refuse clipped highlight");
    assert!(
        err_clipped.to_string().to_lowercase().contains("clipped")
            || err_clipped.to_string().to_lowercase().contains("highlight"),
        "error must mention clipped highlight; got: {err_clipped}"
    );

    // Deep shadow / black spot (0 in 8-bit sRGB maps to 0.0 < 0.005 linear)
    let img_black = make_rgb8_image(50, 50, [0, 0, 0]);
    let session_black = PreviewSession::from_image_buffer(&img_black).unwrap();
    let err_black = session_black
        .white_balance_at(0.5, 0.5)
        .expect_err("must refuse deep shadow");
    assert!(
        err_black.to_string().to_lowercase().contains("shadow")
            || err_black.to_string().to_lowercase().contains("black"),
        "error must mention deep shadow; got: {err_black}"
    );
}

// ---------------------------------------------------------------------------
// 5. a_point_outside_the_frame_is_refused
// ---------------------------------------------------------------------------
#[test]
fn a_point_outside_the_frame_is_refused() {
    let img = make_rgb8_image(50, 50, [128, 128, 128]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    assert!(session.white_balance_at(-0.1, 0.5).is_err());
    assert!(session.white_balance_at(1.1, 0.5).is_err());
    assert!(session.white_balance_at(0.5, -0.01).is_err());
    assert!(session.white_balance_at(0.5, 1.05).is_err());
    assert!(session.white_balance_at(f32::NAN, 0.5).is_err());
    assert!(session.white_balance_at(0.5, f32::NAN).is_err());
}

// ---------------------------------------------------------------------------
// 6. auto_lifts_an_underexposed_photograph_into_the_band
// ---------------------------------------------------------------------------
#[test]
fn auto_lifts_an_underexposed_photograph_into_the_band() {
    // Underexposed image with luma ~60 (below the [95, 135] band)
    let img = make_rgb8_image(100, 100, [60, 60, 60]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let auto = session.auto_tone().expect("auto tone on dark image");
    assert!(
        auto.exposure > 0.0,
        "must lift exposure for dark image, got {}",
        auto.exposure
    );

    let recipe = AdjustmentRecipe {
        exposure: auto.exposure,
        ..Default::default()
    };
    let frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let med = compute_display_luma_percentile(&frame.bytes, 0.5);
    assert!(
        (med - 95.0).abs() <= 4.0,
        "median display luma must be lifted into the band (95 ± 4), got {med}"
    );
}

// ---------------------------------------------------------------------------
// 7. auto_leaves_a_well_exposed_photograph_alone
// ---------------------------------------------------------------------------
#[test]
fn auto_leaves_a_well_exposed_photograph_alone() {
    // Grey 110 is within the target band [95, 135]
    let img = make_rgb8_image(100, 100, [110, 110, 110]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let auto = session.auto_tone().expect("auto tone on normal image");
    assert_eq!(
        auto.exposure, 0.0,
        "well-exposed photograph must have exposure exactly 0, got {}",
        auto.exposure
    );
}

// ---------------------------------------------------------------------------
// 8. auto_puts_the_white_and_black_points_on_target
// ---------------------------------------------------------------------------
#[test]
fn auto_puts_the_white_and_black_points_on_target() {
    // 1000 pixels:
    // 980 pixels at 110 (median 110, so exposure is 0.0)
    // 10 pixels (1%) at 220 (highlights below 235 target)
    // 10 pixels (1%) at 18 (shadows above 12 target)
    let mut data = Vec::with_capacity(1000 * 3);
    for _ in 0..980 {
        data.extend_from_slice(&[110, 110, 110]);
    }
    for _ in 0..10 {
        data.extend_from_slice(&[220, 220, 220]);
    }
    for _ in 0..10 {
        data.extend_from_slice(&[18, 18, 18]);
    }
    let img = ImageBuffer::Rgb8 {
        width: 100,
        height: 10,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let auto = session.auto_tone().expect("auto tone execution");
    assert_eq!(auto.exposure, 0.0);

    let recipe = AdjustmentRecipe {
        exposure: auto.exposure,
        whites: auto.whites,
        blacks: auto.blacks,
        ..Default::default()
    };
    let frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let p995 = compute_display_luma_percentile(&frame.bytes, 0.995);
    let p005 = compute_display_luma_percentile(&frame.bytes, 0.005);

    assert!(
        (p995 - 235.0).abs() <= 4.0,
        "white point (99.5th percentile) must be on target (235 ± 4), got {p995}"
    );
    assert!(
        (p005 - 12.0).abs() <= 4.0,
        "black point (0.5th percentile) must be on target (12 ± 4), got {p005}"
    );
}

// ---------------------------------------------------------------------------
// 9. black_and_white_output_has_no_colour_anywhere
// ---------------------------------------------------------------------------
#[test]
fn black_and_white_output_has_no_colour_anywhere() {
    // 100x10 gradient including saturated colours and near-neutrals
    let mut data = Vec::with_capacity(1000 * 3);
    for i in 0..1000 {
        let t = i as f32 / 999.0;
        let r = ((1.0 - t) * 200.0 + t * 50.0 + (i % 5) as f32)
            .round()
            .clamp(0.0, 255.0) as u8;
        let g = (t * 180.0 + (1.0 - t) * 60.0 + ((i + 2) % 5) as f32)
            .round()
            .clamp(0.0, 255.0) as u8;
        let b = (((i * 7) % 256) as f32 * 0.5 + 80.0).clamp(0.0, 255.0) as u8;
        data.extend_from_slice(&[r, g, b]);
    }
    let img = ImageBuffer::Rgb8 {
        width: 100,
        height: 10,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let bw_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        ..Default::default()
    };
    let frame = session
        .render(&bw_recipe, None, PreviewStage::Drag)
        .unwrap();

    for (idx, px) in frame.bytes.as_chunks::<4>().0.iter().enumerate() {
        let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
        assert!(
            (r - g).abs() <= 1 && (g - b).abs() <= 1 && (r - b).abs() <= 1,
            "pixel {idx} must have no colour (R==G==B within 1 level), got R={r}, G={g}, B={b}"
        );
    }
}

// ---------------------------------------------------------------------------
// 10. the_red_band_darkens_red_and_leaves_blue
// ---------------------------------------------------------------------------
#[test]
fn the_red_band_darkens_red_and_leaves_blue() {
    // 2 pixels: pixel 0 is pure red [220, 30, 30], pixel 1 is pure blue [30, 30, 220]
    let data = vec![220, 30, 30, 30, 30, 220];
    let img = ImageBuffer::Rgb8 {
        width: 2,
        height: 1,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let default_bw = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        ..Default::default()
    };
    let frame_default = session
        .render(&default_bw, None, PreviewStage::Drag)
        .unwrap();

    let red_darkened = BlackAndWhite {
        red: -80.0,
        ..Default::default()
    };
    let red_darkened_bw = AdjustmentRecipe {
        black_and_white: Some(red_darkened),
        ..Default::default()
    };
    let frame_red_dark = session
        .render(&red_darkened_bw, None, PreviewStage::Drag)
        .unwrap();

    let red_def = frame_default.bytes[0] as f32;
    let red_dark = frame_red_dark.bytes[0] as f32;
    assert!(
        red_dark < red_def - 20.0,
        "red band -80 must darken red pixel: default={red_def}, darkened={red_dark}"
    );

    let blue_def = frame_default.bytes[4] as f32;
    let blue_dark = frame_red_dark.bytes[4] as f32;
    assert!(
        (blue_def - blue_dark).abs() <= 2.0,
        "red band -80 must leave blue pixel unaffected: default={blue_def}, modified={blue_dark}"
    );
}

// ---------------------------------------------------------------------------
// 11. colour_grading_still_tints_a_black_and_white_photograph
// ---------------------------------------------------------------------------
#[test]
fn colour_grading_still_tints_a_black_and_white_photograph() {
    let img = make_rgb8_image(50, 50, [150, 100, 50]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let bw_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        ..Default::default()
    };
    let bw_frame = session
        .render(&bw_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(bw_frame.bytes[0], bw_frame.bytes[1]);
    assert_eq!(bw_frame.bytes[1], bw_frame.bytes[2]);

    // Apply global grading tint (sepia/warm)
    let grading_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        grading: Some(ColorGrading {
            global: ColorWheel {
                hue: 35.0,
                saturation: 0.8,
                luminance: 0.0,
            },
            ..Default::default()
        }),
        ..Default::default()
    };
    let graded_frame = session
        .render(&grading_recipe, None, PreviewStage::Drag)
        .unwrap();

    let r = graded_frame.bytes[0];
    let g = graded_frame.bytes[1];
    let b = graded_frame.bytes[2];
    assert!(
        r > b + 10,
        "grading must tint the black & white photograph (R={r} > B={b} + 10)"
    );
    assert_ne!(
        r, g,
        "graded black & white photograph must no longer be strictly monochrome"
    );
}

// ---------------------------------------------------------------------------
// 12. export_and_preview_agree_on_black_and_white
// ---------------------------------------------------------------------------
#[test]
fn export_and_preview_agree_on_black_and_white() {
    let mut data = Vec::with_capacity(50 * 50 * 3);
    for y in 0..50 {
        for x in 0..50 {
            let r = (x * 5) as u8;
            let g = (y * 5) as u8;
            let b = ((x + y) * 2) as u8;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    let img = ImageBuffer::Rgb8 {
        width: 50,
        height: 50,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let bw = BlackAndWhite {
        red: 35.0,
        green: -40.0,
        blue: 25.0,
        ..Default::default()
    };
    let recipe = AdjustmentRecipe {
        black_and_white: Some(bw),
        ..Default::default()
    };

    let prev_frame = session.render(&recipe, None, PreviewStage::Full).unwrap();
    let export_buf = apply_recipe(&img, &recipe, None).expect("export apply adjustments");

    let export_rgb8 = match export_buf {
        ImageBuffer::Rgb8 { data, .. } => data,
        ImageBuffer::Rgb16 { .. } => panic!("expected Rgb8"),
    };

    for i in 0..(50 * 50) {
        let pr = prev_frame.bytes[i * 4] as i32;
        let pg = prev_frame.bytes[i * 4 + 1] as i32;
        let pb = prev_frame.bytes[i * 4 + 2] as i32;

        let er = export_rgb8[i * 3] as i32;
        let eg = export_rgb8[i * 3 + 1] as i32;
        let eb = export_rgb8[i * 3 + 2] as i32;

        assert!(
            (pr - er).abs() <= 2 && (pg - eg).abs() <= 2 && (pb - eb).abs() <= 2,
            "preview and export must agree within 2 levels at pixel {i}: prev=({pr},{pg},{pb}), exp=({er},{eg},{eb})"
        );
    }
}

// ---------------------------------------------------------------------------
// 13. a_recipe_written_before_black_and_white_still_loads
// ---------------------------------------------------------------------------
#[test]
fn a_recipe_written_before_black_and_white_still_loads() {
    let base = AdjustmentRecipe {
        exposure: 0.75,
        contrast: 15.0,
        temperature: -20.0,
        ..Default::default()
    };
    let json_val = serde_json::to_value(&base).unwrap();
    let mut map = json_val.as_object().unwrap().clone();
    map.remove("black_and_white");
    let legacy_json = serde_json::to_string(&map).unwrap();

    let recipe: AdjustmentRecipe = serde_json::from_str(&legacy_json)
        .expect("must parse legacy recipe without black_and_white");
    assert!(
        recipe.black_and_white.is_none(),
        "black_and_white field must default to None"
    );
    assert_eq!(recipe.exposure, 0.75);
    assert_eq!(recipe.contrast, 15.0);
    assert_eq!(recipe.temperature, -20.0);
}

// ---------------------------------------------------------------------------
// 14. auto_does_not_blow_highlights_to_lift_the_median (RP-4 Review Item 4)
// ---------------------------------------------------------------------------
#[test]
fn auto_does_not_blow_highlights_to_lift_the_median() {
    // 1000 pixels:
    // 990 pixels (99%) dark background at 20 (very underexposed, median = 20)
    // 10 pixels (1%) bright highlight area at 240
    let mut data = Vec::with_capacity(1000 * 3);
    for _ in 0..990 {
        data.extend_from_slice(&[20, 20, 20]);
    }
    for _ in 0..10 {
        data.extend_from_slice(&[240, 240, 240]);
    }
    let img = ImageBuffer::Rgb8 {
        width: 100,
        height: 10,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let auto = session
        .auto_tone()
        .expect("auto tone on mostly dark photograph with bright area");

    // The exposure must stay below +2.0
    assert!(
        auto.exposure < 2.0,
        "exposure must stay below +2.0 to protect highlights, got {}",
        auto.exposure
    );

    // When rendered with this exposure, the 99.5th percentile must stay at or below 254
    let recipe = AdjustmentRecipe {
        exposure: auto.exposure,
        ..Default::default()
    };
    let frame = session.render(&recipe, None, PreviewStage::Drag).unwrap();
    let p995 = compute_display_luma_percentile(&frame.bytes, 0.995);
    assert!(
        p995 <= 254.0,
        "99.5th percentile of display luma must stay at or below 254, got {p995}"
    );
}
