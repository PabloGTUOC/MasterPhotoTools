//! Integration tests for detail processing (RP-7): sharpening, noise reduction, hot pixels.

use rollchrome_core::media::edit::detail::apply_detail;
use rollchrome_core::media::edit::pipeline::{
    apply_recipe_with_orientation, AdjustmentRecipe, ImageBuffer, LinearBuffer,
};
use rollchrome_core::media::edit::preview::{fast_linear_to_srgb, PreviewSession, PreviewStage};
use rollchrome_core::media::edit::Detail;

#[test]
fn detail_at_rest_changes_nothing() {
    let buf = ImageBuffer::Rgb8 {
        width: 120,
        height: 80,
        data: (0..120 * 80 * 3).map(|i| ((i * 7) % 256) as u8).collect(),
    };
    let session = PreviewSession::new(&buf).unwrap();

    let recipe_none = AdjustmentRecipe {
        detail: None,
        ..AdjustmentRecipe::default()
    };

    let recipe_identity = AdjustmentRecipe {
        detail: Some(Detail::default()),
        ..AdjustmentRecipe::default()
    };

    let frame_none_drag = session
        .render(&recipe_none, None, PreviewStage::Drag)
        .unwrap();
    let frame_id_drag = session
        .render(&recipe_identity, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(frame_none_drag.bytes, frame_id_drag.bytes);

    let frame_none_settle = session
        .render(&recipe_none, None, PreviewStage::Settle)
        .unwrap();
    let frame_id_settle = session
        .render(&recipe_identity, None, PreviewStage::Settle)
        .unwrap();
    assert_eq!(frame_none_settle.bytes, frame_id_settle.bytes);

    let frame_none_full = session
        .render(&recipe_none, None, PreviewStage::Full)
        .unwrap();
    let frame_id_full = session
        .render(&recipe_identity, None, PreviewStage::Full)
        .unwrap();
    assert_eq!(frame_none_full.bytes, frame_id_full.bytes);

    let exp_none = apply_recipe_with_orientation(&buf, &recipe_none, None, 1).unwrap();
    let exp_id = apply_recipe_with_orientation(&buf, &recipe_identity, None, 1).unwrap();
    assert_eq!(exp_none.as_rgb8().unwrap(), exp_id.as_rgb8().unwrap());
}

#[test]
fn sharpening_raises_the_contrast_of_an_edge() {
    let w = 64;
    let h = 64;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let val = if x < 32 { 0.2f32 } else { 0.8f32 };
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };
    let detail = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 1.0,
        sharpen_masking: 0.0,
        ..Detail::default()
    };
    let sharpened = apply_detail(&lin, &detail, 1.0);

    let y = 32;
    let dark_val = sharpened.data[(y * w + 31) * 3];
    let light_val = sharpened.data[(y * w + 32) * 3];

    assert!(
        dark_val < 0.2,
        "dark side should undershoot below 0.2, got {dark_val}"
    );
    assert!(
        light_val > 0.8,
        "light side should overshoot above 0.8, got {light_val}"
    );
    assert!(
        light_val - dark_val > 0.6,
        "contrast across edge should increase"
    );
}

#[test]
fn sharpening_leaves_a_flat_area_alone() {
    let w = 64;
    let h = 64;
    let data = vec![0.5f32; w * h * 3];
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data: data.clone(),
    };
    let detail = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 2.0,
        sharpen_masking: 0.0,
        ..Detail::default()
    };
    let sharpened = apply_detail(&lin, &detail, 1.0);
    for (i, (&orig, &sharp)) in data.iter().zip(sharpened.data.iter()).enumerate() {
        assert!(
            (orig - sharp).abs() < 1e-4,
            "mismatch at index {i}: orig={orig}, sharp={sharp}"
        );
    }
}

#[test]
fn masking_keeps_noise_in_flat_areas_from_being_sharpened() {
    let w = 64;
    let h = 64;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let val = if x < 32 {
                let n = if (x + y) % 2 == 0 { 0.005 } else { -0.005 };
                0.5 + n
            } else {
                0.8
            };
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };
    let unmasked = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 1.0,
        sharpen_masking: 0.0,
        ..Detail::default()
    };
    let masked = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 1.0,
        sharpen_masking: 80.0,
        ..Detail::default()
    };
    let sharp_unmasked = apply_detail(&lin, &unmasked, 1.0);
    let sharp_masked = apply_detail(&lin, &masked, 1.0);

    let mut diff_unmasked = 0.0f32;
    let mut diff_masked = 0.0f32;
    let mut count = 0;
    for y in 10..54 {
        for x in 5..25 {
            let orig = lin.data[(y * w + x) * 3];
            let u = sharp_unmasked.data[(y * w + x) * 3];
            let m = sharp_masked.data[(y * w + x) * 3];
            diff_unmasked += (u - orig).abs();
            diff_masked += (m - orig).abs();
            count += 1;
        }
    }
    let mean_unmasked_change = diff_unmasked / count as f32;
    let mean_masked_change = diff_masked / count as f32;
    assert!(
        mean_masked_change < mean_unmasked_change * 0.25,
        "masked change ({mean_masked_change}) should be much smaller than unmasked ({mean_unmasked_change})"
    );
}

#[test]
fn luminance_noise_reduction_halves_the_noise_and_keeps_the_edge() {
    let w = 128;
    let h = 128;
    let mut rng = 123456789u64;
    let mut rand_f32 = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((rng >> 33) as f32) / (1u32 << 31) as f32
    };

    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let base = if x < 64 { 0.25f32 } else { 0.75f32 };
            let u1 = rand_f32().max(1e-6);
            let u2 = rand_f32();
            let g = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
            let noise = g * 0.04;
            let val = (base + noise).clamp(0.01, 1.0);
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = val;
            }
        }
    }

    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };

    let detail = Detail {
        noise_luminance: 50.0,
        ..Detail::default()
    };
    let denoised = apply_detail(&lin, &detail, 1.0);

    // Calculate std dev in flat noisy region (x in 10..50, y in 10..118)
    let mut sum_in = 0.0f32;
    let mut sum_sq_in = 0.0f32;
    let mut sum_out = 0.0f32;
    let mut sum_sq_out = 0.0f32;
    let mut count = 0;

    for y in 10..118 {
        for x in 10..50 {
            let v_in = lin.data[(y * w + x) * 3];
            let v_out = denoised.data[(y * w + x) * 3];
            sum_in += v_in;
            sum_sq_in += v_in * v_in;
            sum_out += v_out;
            sum_sq_out += v_out * v_out;
            count += 1;
        }
    }

    let n = count as f32;
    let var_in = (sum_sq_in / n) - (sum_in / n).powi(2);
    let var_out = (sum_sq_out / n) - (sum_out / n).powi(2);
    let std_in = var_in.max(0.0).sqrt();
    let std_out = var_out.max(0.0).sqrt();

    assert!(
        std_out <= 0.5 * std_in,
        "std dev should be halved: std_in={std_in}, std_out={std_out}"
    );

    // Check step at edge: difference between right side (x in 78..118) and left side (x in 10..50)
    let mut left_sum_in = 0.0f32;
    let mut right_sum_in = 0.0f32;
    let mut left_sum_out = 0.0f32;
    let mut right_sum_out = 0.0f32;
    let mut plateau_count = 0;

    for y in 20..108 {
        for dx in 0..30 {
            let x_left = 15 + dx;
            let x_right = 80 + dx;
            left_sum_in += lin.data[(y * w + x_left) * 3];
            right_sum_in += lin.data[(y * w + x_right) * 3];
            left_sum_out += denoised.data[(y * w + x_left) * 3];
            right_sum_out += denoised.data[(y * w + x_right) * 3];
            plateau_count += 1;
        }
    }

    let pn = plateau_count as f32;
    let step_in = (right_sum_in / pn) - (left_sum_in / pn);
    let step_out = (right_sum_out / pn) - (left_sum_out / pn);

    assert!(
        step_out >= 0.90 * step_in,
        "step height should be >= 90%: step_in={step_in}, step_out={step_out}"
    );
}

#[test]
fn colour_noise_reduction_halves_chroma_noise_and_leaves_luminance() {
    let w = 128;
    let h = 128;
    let mut rng = 987654321u64;
    let mut rand_f32 = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((rng >> 33) as f32) / (1u32 << 31) as f32
    };

    let mut data = vec![0.0f32; w * h * 3];
    let base = 0.4f32; // base linear value
    for y in 0..h {
        for x in 0..w {
            let u1 = rand_f32().max(1e-6);
            let u2 = rand_f32();
            let g1 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
            let u3 = rand_f32().max(1e-6);
            let u4 = rand_f32();
            let g2 = (-2.0 * u3.ln()).sqrt() * (2.0 * std::f32::consts::PI * u4).cos();

            // Opposing variations in R and B that preserve luminance Y = 0.2126 R + 0.7152 G + 0.0722 B
            let dr = g1 * 0.05;
            let db = g2 * 0.05;
            let dg = -(0.2126 * dr + 0.0722 * db) / 0.7152;

            let r = (base + dr).clamp(0.01, 1.0);
            let g = (base + dg).clamp(0.01, 1.0);
            let b = (base + db).clamp(0.01, 1.0);

            data[(y * w + x) * 3] = r;
            data[(y * w + x) * 3 + 1] = g;
            data[(y * w + x) * 3 + 2] = b;
        }
    }

    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };

    let detail = Detail {
        noise_colour: 50.0,
        ..Detail::default()
    };
    let denoised = apply_detail(&lin, &detail, 1.0);

    // Compute chroma (Cb = B - Y, Cr = R - Y) variance before and after
    let mut sum_chroma_sq_in = 0.0f32;
    let mut sum_chroma_sq_out = 0.0f32;
    let mut sum_luma_in = 0.0f32;
    let mut sum_luma_out = 0.0f32;
    let count = (w - 20) * (h - 20);

    for y in 10..(h - 10) {
        for x in 10..(w - 10) {
            let r_in = lin.data[(y * w + x) * 3].sqrt();
            let g_in = lin.data[(y * w + x) * 3 + 1].sqrt();
            let b_in = lin.data[(y * w + x) * 3 + 2].sqrt();
            let y_in = 0.2126 * r_in + 0.7152 * g_in + 0.0722 * b_in;
            let cb_in = b_in - y_in;
            let cr_in = r_in - y_in;
            sum_chroma_sq_in += cb_in * cb_in + cr_in * cr_in;
            sum_luma_in += y_in;

            let r_out = denoised.data[(y * w + x) * 3].sqrt();
            let g_out = denoised.data[(y * w + x) * 3 + 1].sqrt();
            let b_out = denoised.data[(y * w + x) * 3 + 2].sqrt();
            let y_out = 0.2126 * r_out + 0.7152 * g_out + 0.0722 * b_out;
            let cb_out = b_out - y_out;
            let cr_out = r_out - y_out;
            sum_chroma_sq_out += cb_out * cb_out + cr_out * cr_out;
            sum_luma_out += y_out;
        }
    }

    let std_chroma_in = (sum_chroma_sq_in / count as f32).sqrt();
    let std_chroma_out = (sum_chroma_sq_out / count as f32).sqrt();

    assert!(
        std_chroma_out <= 0.5 * std_chroma_in,
        "chroma noise should be halved: in={std_chroma_in}, out={std_chroma_out}"
    );

    let mean_luma_in = sum_luma_in / count as f32;
    let mean_luma_out = sum_luma_out / count as f32;
    let luma_diff_ratio = (mean_luma_out - mean_luma_in).abs() / mean_luma_in;
    assert!(
        luma_diff_ratio <= 0.05,
        "luma should be within 5%: in={mean_luma_in}, out={mean_luma_out}, ratio={luma_diff_ratio}"
    );
}

#[test]
fn a_hot_pixel_is_removed_and_its_neighbours_are_untouched() {
    let w = 32;
    let h = 32;
    let mut data = vec![0.1f32; w * h * 3];
    let hot_x = 16;
    let hot_y = 16;
    data[(hot_y * w + hot_x) * 3] = 1.0;
    data[(hot_y * w + hot_x) * 3 + 1] = 1.0;
    data[(hot_y * w + hot_x) * 3 + 2] = 1.0;

    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };

    let detail = Detail {
        hot_pixels: true,
        ..Detail::default()
    };
    let cleaned = apply_detail(&lin, &detail, 1.0);

    // The hot pixel should be replaced by median of neighbours (0.1)
    let hot_cleaned = cleaned.data[(hot_y * w + hot_x) * 3];
    assert!(
        (hot_cleaned - 0.1).abs() < 1e-4,
        "hot pixel should be replaced: {hot_cleaned}"
    );

    // Its 8 neighbours should be untouched (0.1)
    for dy in -1i32..=1i32 {
        for dx in -1i32..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (hot_x as i32 + dx) as usize;
            let ny = (hot_y as i32 + dy) as usize;
            for c in 0..3 {
                let v = cleaned.data[(ny * w + nx) * 3 + c];
                assert!(
                    (v - 0.1).abs() < 1e-5,
                    "neighbour at ({nx}, {ny}) should be untouched, got {v}"
                );
            }
        }
    }
}

#[test]
fn a_bright_star_wider_than_one_pixel_survives() {
    let w = 32;
    let h = 32;
    let mut data = vec![0.02f32; w * h * 3];
    // 2x2 star
    for y in 15..=16 {
        for x in 15..=16 {
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = 0.9;
            }
        }
    }

    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };

    let detail = Detail {
        hot_pixels: true,
        ..Detail::default()
    };
    let cleaned = apply_detail(&lin, &detail, 1.0);

    for y in 15..=16 {
        for x in 15..=16 {
            for c in 0..3 {
                let v = cleaned.data[(y * w + x) * 3 + c];
                assert!(
                    (v - 0.9).abs() < 1e-4,
                    "star pixel at ({x}, {y}) should survive, got {v}"
                );
            }
        }
    }
}

fn downscale_2x(src: &LinearBuffer) -> LinearBuffer {
    let out_w = src.width / 2;
    let out_h = src.height / 2;
    let mut out_data = vec![0.0f32; (out_w * out_h * 3) as usize];

    for oy in 0..out_h {
        for ox in 0..out_w {
            for c in 0..3 {
                let s00 = src.data[((oy * 2 * src.width + ox * 2) * 3 + c) as usize];
                let s01 = src.data[((oy * 2 * src.width + (ox * 2 + 1)) * 3 + c) as usize];
                let s10 = src.data[(((oy * 2 + 1) * src.width + ox * 2) * 3 + c) as usize];
                let s11 = src.data[(((oy * 2 + 1) * src.width + (ox * 2 + 1)) * 3 + c) as usize];
                out_data[((oy * out_w + ox) * 3 + c) as usize] = (s00 + s01 + s10 + s11) * 0.25;
            }
        }
    }

    LinearBuffer {
        width: out_w,
        height: out_h,
        data: out_data,
    }
}

#[test]
fn the_radius_scales_with_the_proxy() {
    let w = 128;
    let h = 128;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let v = ((x as f32 * 0.1).sin() * (y as f32 * 0.1).cos() * 0.4 + 0.5).clamp(0.0, 1.0);
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = v;
            }
        }
    }
    let full = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };

    let detail = Detail {
        sharpen_amount: 80.0,
        sharpen_radius: 2.0,
        sharpen_masking: 0.0,
        ..Detail::default()
    };

    let full_sharpened = apply_detail(&full, &detail, 1.0);
    let halved_of_full = downscale_2x(&full_sharpened);

    let half_source = downscale_2x(&full);
    let sharpened_half = apply_detail(&half_source, &detail, 0.5);

    // Compare in 8-bit levels (0..255)
    let mut total_diff = 0.0f32;
    let count = halved_of_full.data.len();
    for i in 0..count {
        let lev_a = (fast_linear_to_srgb(halved_of_full.data[i]) * 255.0).round();
        let lev_b = (fast_linear_to_srgb(sharpened_half.data[i]) * 255.0).round();
        total_diff += (lev_a - lev_b).abs();
    }
    let mean_diff = total_diff / count as f32;
    assert!(
        mean_diff <= 2.0,
        "mean difference between scaled radius and halved sharpened image must be <= 2.0 levels, got {mean_diff}"
    );
}

#[test]
fn export_and_full_preview_agree_on_detail() {
    let w = 120;
    let h = 90;
    let img_data: Vec<u8> = (0..w * h * 3).map(|i| ((i * 13) % 256) as u8).collect();
    let buf = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data: img_data,
    };
    let session = PreviewSession::new(&buf).unwrap();

    let recipe = AdjustmentRecipe {
        detail: Some(Detail {
            sharpen_amount: 80.0,
            sharpen_radius: 1.5,
            sharpen_masking: 20.0,
            noise_luminance: 30.0,
            noise_colour: 30.0,
            hot_pixels: true,
        }),
        ..AdjustmentRecipe::default()
    };

    let full_frame = session.render(&recipe, None, PreviewStage::Full).unwrap();
    let export_buf = apply_recipe_with_orientation(&buf, &recipe, None, 1).unwrap();
    let export_bytes = export_buf.as_rgb8().unwrap();

    let mut total_diff = 0.0f32;
    let n_pixels = (w * h) as usize;
    for i in 0..n_pixels {
        let pr = full_frame.bytes[i * 4] as f32;
        let pg = full_frame.bytes[i * 4 + 1] as f32;
        let pb = full_frame.bytes[i * 4 + 2] as f32;

        let er = export_bytes[i * 3] as f32;
        let eg = export_bytes[i * 3 + 1] as f32;
        let eb = export_bytes[i * 3 + 2] as f32;

        total_diff += ((pr - er).abs() + (pg - eg).abs() + (pb - eb).abs()) / 3.0;
    }
    let mean_diff = total_diff / n_pixels as f32;
    assert!(
        mean_diff <= 2.0,
        "export and full preview should agree within 2 levels, got {mean_diff}"
    );
}

#[test]
fn changing_another_slider_does_not_recompute_detail() {
    let buf = ImageBuffer::Rgb8 {
        width: 100,
        height: 100,
        data: vec![128; 100 * 100 * 3],
    };
    let session = PreviewSession::new(&buf).unwrap();
    let mut recipe = AdjustmentRecipe {
        detail: Some(Detail {
            sharpen_amount: 50.0,
            ..Detail::default()
        }),
        ..AdjustmentRecipe::default()
    };

    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(session.detail_computes(), 1);

    // Modify exposure only
    recipe.exposure = 0.5;
    session.render(&recipe, None, PreviewStage::Drag).unwrap();
    assert_eq!(
        session.detail_computes(),
        1,
        "detail should not be recomputed when another slider changes"
    );
}

#[test]
fn a_recipe_written_before_detail_still_loads() {
    let mut val = serde_json::to_value(AdjustmentRecipe::default()).unwrap();
    val.as_object_mut().unwrap().remove("detail");
    val.as_object_mut()
        .unwrap()
        .insert("exposure".to_string(), serde_json::json!(0.5));
    let recipe: AdjustmentRecipe = serde_json::from_value(val).unwrap();
    assert!(recipe.detail.is_none());
    assert_eq!(recipe.exposure, 0.5);
    assert!(!recipe.is_identity()); // exposure is 0.5

    let mut val_empty = serde_json::to_value(AdjustmentRecipe::default()).unwrap();
    val_empty.as_object_mut().unwrap().remove("detail");
    let recipe_empty: AdjustmentRecipe = serde_json::from_value(val_empty).unwrap();
    assert!(recipe_empty.detail.is_none());
    assert!(recipe_empty.is_identity());
}

#[test]
fn a_sharpened_photograph_is_not_identity() {
    let mut recipe = AdjustmentRecipe::default();
    assert!(recipe.is_identity());

    recipe.detail = Some(Detail {
        sharpen_amount: 50.0,
        ..Detail::default()
    });
    assert!(!recipe.is_identity());
    assert!(!recipe.detail.as_ref().unwrap().is_identity());
}
