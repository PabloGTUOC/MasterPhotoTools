//! Tests for point colour adjustments (RP-10).

use rollchrome_core::media::edit::color::linear_srgb_to_oklab;
use rollchrome_core::media::edit::point_colour::{CompiledPointColours, PointColour};
use rollchrome_core::media::edit::{
    apply_recipe, decode_image, AdjustmentRecipe, BlackAndWhite, LinearBuffer, PreviewSession,
    PreviewStage,
};
use rollchrome_core::tools::edit::save_recipe;

#[test]
fn a_point_colour_at_rest_changes_nothing() {
    let mut buf = LinearBuffer::new(4, 4);
    // Fill with colorful and neutral linear sRGB pixels
    for (i, px) in buf.data.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let v = (i as f32 + 1.0) / 17.0;
        px[0] = v;
        px[1] = (v * 1.5).min(1.0);
        px[2] = (1.0 - v).max(0.0);
    }
    let orig = buf.data.clone();

    let recipe_rest = AdjustmentRecipe {
        point_colours: vec![PointColour {
            hue: 120.0,
            chroma: 0.15,
            lightness: 0.5,
            range: 50.0,
            hue_shift: 0.0,
            saturation: 0.0,
            luminance: 0.0,
        }],
        ..Default::default()
    };

    assert!(recipe_rest.is_identity());
    buf.apply_adjustments(&recipe_rest);

    assert_eq!(
        buf.data, orig,
        "a point colour at rest must produce identical bytes"
    );
}

#[test]
fn saturation_changes_the_picked_colour_and_leaves_another_alone() {
    // Red pixel in linear sRGB: high R, low G/B
    let r_lin = 0.8f32;
    let g_lin = 0.1f32;
    let b_lin = 0.1f32;
    let (l_red, a_red, b_red) = linear_srgb_to_oklab(r_lin, g_lin, b_lin);
    let c_red = (a_red * a_red + b_red * b_red).sqrt();
    let h_red = b_red.atan2(a_red).to_degrees().rem_euclid(360.0);

    // Blue pixel in linear sRGB: low R/G, high B
    let (l_blue, a_blue, b_blue) = linear_srgb_to_oklab(0.1, 0.1, 0.8);
    let c_blue_orig = (a_blue * a_blue + b_blue * b_blue).sqrt();

    // Picked on red with positive saturation boost (+60%)
    let pt = PointColour {
        hue: h_red,
        chroma: c_red,
        lightness: l_red,
        range: 50.0,
        hue_shift: 0.0,
        saturation: 60.0,
        luminance: 0.0,
    };
    let compiled = CompiledPointColours::compile(&[pt]);

    // Apply to red
    let (_l_red_out, a_red_out, b_red_out) = compiled.apply(l_red, a_red, b_red);
    let c_red_out = (a_red_out * a_red_out + b_red_out * b_red_out).sqrt();
    assert!(
        c_red_out > c_red * 1.1,
        "Red chroma should increase significantly: before {c_red}, after {c_red_out}"
    );

    // Apply to blue (far away in hue): should be untouched
    let (l_blue_out, a_blue_out, b_blue_out) = compiled.apply(l_blue, a_blue, b_blue);
    let c_blue_out = (a_blue_out * a_blue_out + b_blue_out * b_blue_out).sqrt();
    assert!(
        (c_blue_out - c_blue_orig).abs() < 1e-5,
        "Blue chroma must be untouched: before {c_blue_orig}, after {c_blue_out}"
    );
    assert_eq!(
        (l_blue_out, a_blue_out, b_blue_out),
        (l_blue, a_blue, b_blue)
    );
}

#[test]
fn hue_shift_moves_the_picked_colour() {
    let (l, a, b_val) = linear_srgb_to_oklab(0.8, 0.2, 0.1);
    let c = (a * a + b_val * b_val).sqrt();
    let h = b_val.atan2(a).to_degrees().rem_euclid(360.0);

    // Shift hue by +50 on the slider (which scales to +50 * 0.3 = +15 degrees)
    let pt = PointColour {
        hue: h,
        chroma: c,
        lightness: l,
        range: 50.0,
        hue_shift: 50.0,
        saturation: 0.0,
        luminance: 0.0,
    };
    let compiled = CompiledPointColours::compile(&[pt]);

    let (_l_out, a_out, b_out) = compiled.apply(l, a, b_val);
    let h_out = b_out.atan2(a_out).to_degrees().rem_euclid(360.0);
    let expected_h = (h + 50.0 * 0.3).rem_euclid(360.0);

    assert!(
        (h_out - expected_h).abs() < 0.2,
        "Hue shift should move picked hue from {h}° to ~{expected_h}°, got {h_out}°"
    );
}

#[test]
fn luminance_changes_the_picked_colour() {
    let (l, a, b_val) = linear_srgb_to_oklab(0.7, 0.2, 0.2);
    let c = (a * a + b_val * b_val).sqrt();
    let h = b_val.atan2(a).to_degrees().rem_euclid(360.0);

    // Boost luminance by +60%
    let pt_bright = PointColour {
        hue: h,
        chroma: c,
        lightness: l,
        range: 50.0,
        hue_shift: 0.0,
        saturation: 0.0,
        luminance: 60.0,
    };
    let compiled_bright = CompiledPointColours::compile(&[pt_bright]);
    let (l_bright, _, _) = compiled_bright.apply(l, a, b_val);
    assert!(
        l_bright > l,
        "Positive luminance should increase lightness: before {l}, after {l_bright}"
    );

    // Lower luminance by -60%
    let pt_dark = PointColour {
        hue: h,
        chroma: c,
        lightness: l,
        range: 50.0,
        hue_shift: 0.0,
        saturation: 0.0,
        luminance: -60.0,
    };
    let compiled_dark = CompiledPointColours::compile(&[pt_dark]);
    let (l_dark, _, _) = compiled_dark.apply(l, a, b_val);
    assert!(
        l_dark < l,
        "Negative luminance should decrease lightness: before {l}, after {l_dark}"
    );
}

#[test]
fn a_wider_range_reaches_further() {
    let (l0, a0, b0) = linear_srgb_to_oklab(0.8, 0.1, 0.1);
    let c0 = (a0 * a0 + b0 * b0).sqrt();
    let h0 = b0.atan2(a0).to_degrees().rem_euclid(360.0);

    // Pixel at 20° hue distance with same chroma and lightness
    let h_test = (h0 + 20.0).rem_euclid(360.0);
    let (sin_t, cos_t) = h_test.to_radians().sin_cos();
    let (l_test, a_test, b_test) = (l0, c0 * cos_t, c0 * sin_t);

    // Point with range = 0 (hue width = 8° < 20°): should NOT reach test pixel
    let pt_narrow = PointColour {
        hue: h0,
        chroma: c0,
        lightness: l0,
        range: 0.0,
        hue_shift: 50.0,
        saturation: 0.0,
        luminance: 0.0,
    };
    let compiled_narrow = CompiledPointColours::compile(&[pt_narrow]);
    let (_, a_narrow, b_narrow) = compiled_narrow.apply(l_test, a_test, b_test);
    assert_eq!(
        (a_narrow, b_narrow),
        (a_test, b_test),
        "Range 0 should not reach 20° away"
    );

    // Point with range = 100 (hue width = 60° > 20°): SHOULD reach test pixel
    let pt_wide = PointColour {
        hue: h0,
        chroma: c0,
        lightness: l0,
        range: 100.0,
        hue_shift: 50.0,
        saturation: 0.0,
        luminance: 0.0,
    };
    let compiled_wide = CompiledPointColours::compile(&[pt_wide]);
    let (_, a_wide, b_wide) = compiled_wide.apply(l_test, a_test, b_test);
    let h_wide = b_wide.atan2(a_wide).to_degrees().rem_euclid(360.0);
    assert!(
        (h_wide - h_test).abs() > 0.5,
        "Range 100 should reach 20° away and shift hue, got {h_wide} vs orig {h_test}"
    );
}

#[test]
fn near_neutrals_are_never_touched() {
    // Pure neutral grey
    let (l_grey, a_grey, b_grey) = linear_srgb_to_oklab(0.5, 0.5, 0.5);

    // Very low chroma pixel (chroma <= 0.005)
    let (l_low, a_low, b_low) = linear_srgb_to_oklab(0.501, 0.5, 0.499);

    let pt = PointColour {
        hue: 0.0,
        chroma: 0.003,
        lightness: 0.5,
        range: 100.0,
        hue_shift: 80.0,
        saturation: 100.0,
        luminance: 50.0,
    };
    let compiled = CompiledPointColours::compile(&[pt]);

    let res_grey = compiled.apply(l_grey, a_grey, b_grey);
    assert_eq!(
        res_grey,
        (l_grey, a_grey, b_grey),
        "neutrals must be untouched"
    );

    let res_low = compiled.apply(l_low, a_low, b_low);
    assert_eq!(
        res_low,
        (l_low, a_low, b_low),
        "near-neutrals under fade must be untouched"
    );
}

#[test]
fn two_points_do_not_depend_on_their_order() {
    let pt1 = PointColour {
        hue: 30.0,
        chroma: 0.15,
        lightness: 0.6,
        range: 40.0,
        hue_shift: 30.0,
        saturation: -20.0,
        luminance: 10.0,
    };
    let pt2 = PointColour {
        hue: 45.0,
        chroma: 0.12,
        lightness: 0.55,
        range: 60.0,
        hue_shift: -15.0,
        saturation: 40.0,
        luminance: -25.0,
    };

    let compiled12 = CompiledPointColours::compile(&[pt1.clone(), pt2.clone()]);
    let compiled21 = CompiledPointColours::compile(&[pt2, pt1]);

    // Test a grid of colors spanning the overlap region
    for hue in (20..=60).step_by(5) {
        let (sin_h, cos_h) = (hue as f32).to_radians().sin_cos();
        let l = 0.58;
        let c = 0.14;
        let (a, b) = (c * cos_h, c * sin_h);

        let out12 = compiled12.apply(l, a, b);
        let out21 = compiled21.apply(l, a, b);

        assert_eq!(
            out12, out21,
            "Order of points must produce identical outputs at hue {hue}°"
        );
    }
}

#[test]
fn black_and_white_ignores_point_colours() {
    let mut buf = LinearBuffer::new(8, 8);
    for (i, px) in buf.data.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        px[0] = ((i * 17) % 255) as f32 / 255.0;
        px[1] = ((i * 31) % 255) as f32 / 255.0;
        px[2] = ((i * 53) % 255) as f32 / 255.0;
    }
    let mut buf2 = buf.clone();

    let bw = BlackAndWhite {
        red: 20.0,
        orange: 0.0,
        yellow: -10.0,
        green: 15.0,
        aqua: 0.0,
        blue: -20.0,
        purple: 0.0,
        magenta: 10.0,
    };

    let recipe_bw_only = AdjustmentRecipe {
        black_and_white: Some(bw),
        ..Default::default()
    };
    buf.apply_adjustments(&recipe_bw_only);

    let recipe_bw_with_points = AdjustmentRecipe {
        black_and_white: Some(bw),
        point_colours: vec![PointColour {
            hue: 30.0,
            chroma: 0.2,
            lightness: 0.5,
            range: 80.0,
            hue_shift: 80.0,
            saturation: 100.0,
            luminance: 50.0,
        }],
        ..Default::default()
    };
    buf2.apply_adjustments(&recipe_bw_with_points);

    assert_eq!(
        buf.data, buf2.data,
        "Black & White must completely ignore point colours"
    );
}

#[test]
fn picking_a_colour_returns_its_oklch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.jpg");

    // Pure red image in 8-bit sRGB: [220, 40, 40]
    let img = image::RgbImage::from_fn(64, 64, |_, _| image::Rgb([220, 40, 40]));
    img.save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();
    let pick = session.colour_at(0.5, 0.5).expect("pick colour");

    // sRGB red has OkLCh hue around 29° ± 5°, chroma around 0.18 ± 0.05, lightness around 0.55 ± 0.1
    assert!(
        (pick.hue - 29.0).abs() < 5.0,
        "Expected picked red hue ~29°, got {}",
        pick.hue
    );
    assert!(
        pick.chroma > 0.10,
        "Expected high chroma for red, got {}",
        pick.chroma
    );
    assert!(
        pick.lightness > 0.4 && pick.lightness < 0.7,
        "Expected mid lightness for red, got {}",
        pick.lightness
    );
}

#[test]
fn picking_a_neutral_spot_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grey.jpg");

    // Flat neutral grey
    let img = image::RgbImage::from_fn(64, 64, |_, _| image::Rgb([128, 128, 128]));
    img.save(&path).unwrap();

    let session = PreviewSession::open(&path).unwrap();
    let err = session
        .colour_at(0.5, 0.5)
        .expect_err("neutral pick must be refused");

    assert!(
        err.to_string().contains("Chroma below 0.01"),
        "Refusal must mention chroma below 0.01; got: {err}"
    );
}

#[test]
fn export_and_preview_agree_on_point_colour() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_gradient.jpg");

    let img = image::RgbImage::from_fn(128, 128, |x, y| {
        let r = (x * 2) as u8;
        let g = (y * 2) as u8;
        let b = 100u8;
        image::Rgb([r, g, b])
    });
    img.save(&path).unwrap();

    let recipe = AdjustmentRecipe {
        point_colours: vec![PointColour {
            hue: 30.0,
            chroma: 0.15,
            lightness: 0.5,
            range: 50.0,
            hue_shift: 40.0,
            saturation: 30.0,
            luminance: -20.0,
        }],
        ..Default::default()
    };

    let session = PreviewSession::open(&path).unwrap();
    let settle_frame = session.render(&recipe, None, PreviewStage::Settle).unwrap();

    let decoded = decode_image(&path).unwrap();
    let exported = apply_recipe(&decoded, &recipe, None).unwrap();
    let exp_bytes = exported.as_rgb8().unwrap();

    // Check corresponding pixels across the image
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
fn more_than_eight_points_are_refused_on_saving() {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("photo.jpg");
    image::RgbImage::new(10, 10).save(&image_path).unwrap();

    let recipe = AdjustmentRecipe {
        point_colours: (0..9)
            .map(|i| PointColour {
                hue: i as f32 * 40.0,
                chroma: 0.1,
                lightness: 0.5,
                range: 50.0,
                hue_shift: 0.0,
                saturation: 0.0,
                luminance: 0.0,
            })
            .collect(),
        ..Default::default()
    };

    let err = save_recipe(&image_path, &recipe).expect_err("saving 9 points must be refused");
    assert!(
        err.to_string()
            .contains("Cannot save more than 8 point colours"),
        "Error message must specify refusal for > 8 points; got: {err}"
    );
}

#[test]
fn a_recipe_written_before_point_colour_still_loads() {
    let legacy_json = r#"{
        "version": 3,
        "source_sha256": "abcdef123456",
        "exposure": 0.5,
        "temperature": 10.0,
        "tint": -5.0,
        "highlights": 0.0,
        "shadows": 0.0,
        "contrast": 0.0,
        "saturation": 0.0,
        "vibrance": 0.0,
        "lut": null,
        "lut_intensity": 1.0
    }"#;

    let recipe: AdjustmentRecipe =
        serde_json::from_str(legacy_json).expect("deserialize legacy recipe");
    assert_eq!(recipe.exposure, 0.5);
    assert!(
        recipe.point_colours.is_empty(),
        "point_colours must default to empty"
    );
}
