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
            ..Detail::default()
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

#[test]
fn sharpening_radius_changes_the_result_and_narrows_below_a_pixel() {
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

    // 1. Radii 0.6 and 1.2 give different results at scale 1.0
    let d06 = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 0.6,
        sharpen_masking: 0.0,
        ..Detail::default()
    };
    let d12 = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 1.2,
        sharpen_masking: 0.0,
        ..Detail::default()
    };
    let res06 = apply_detail(&lin, &d06, 1.0);
    let res12 = apply_detail(&lin, &d12, 1.0);
    assert_ne!(
        res06.data, res12.data,
        "radii 0.6 and 1.2 must give different results at scale 1.0"
    );

    // 2. At scale 0.25 with radius 1.0 the sharpening reaches no further than 2 pixels from an edge
    let d10 = Detail {
        sharpen_amount: 100.0,
        sharpen_radius: 1.0,
        sharpen_masking: 0.0,
        ..Detail::default()
    };
    let res_scale_025 = apply_detail(&lin, &d10, 0.25);
    // Edge transition is between x = 31 and x = 32.
    // Pixels at distance > 2 from the boundary are x <= 29 and x >= 34.
    for y in 0..h {
        for x in 0..=29 {
            let orig = lin.data[(y * w + x) * 3];
            let sharp = res_scale_025.data[(y * w + x) * 3];
            assert!(
                (sharp - orig).abs() < 1e-5,
                "sharpening at scale 0.25 must reach no further than 2 pixels from edge: x={x}, orig={orig}, sharp={sharp}"
            );
        }
        for x in 34..w {
            let orig = lin.data[(y * w + x) * 3];
            let sharp = res_scale_025.data[(y * w + x) * 3];
            assert!(
                (sharp - orig).abs() < 1e-5,
                "sharpening at scale 0.25 must reach no further than 2 pixels from edge: x={x}, orig={orig}, sharp={sharp}"
            );
        }
    }
}

// =============================================================================
// RP-8 Acceptance Tests: Clarity, Texture, Dehaze (Presence)
// =============================================================================

#[test]
fn presence_at_rest_changes_nothing() {
    let w = 100;
    let h = 100;
    let mut data = vec![0.0f32; w * h * 3];
    for (i, val) in data.iter_mut().enumerate() {
        *val = ((i * 17) % 256) as f32 / 255.0;
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data: data.clone(),
    };
    let detail = Detail::default();
    let res = apply_detail(&lin, &detail, 1.0);
    assert_eq!(res.data, lin.data);
}

#[test]
fn clarity_raises_mid_tone_contrast_on_a_soft_edge() {
    let w = 800;
    let h = 600;
    let mut data = vec![0.0f32; w * h * 3];
    // A soft gradient from 0.3 to 0.7 across 40 px (x from 380 to 420)
    for y in 0..h {
        for x in 0..w {
            let y_val = if x < 380 {
                0.3f32
            } else if x >= 420 {
                0.7f32
            } else {
                let t = (x - 380) as f32 / 40.0;
                0.3f32 + 0.4f32 * (t * t * (3.0 - 2.0 * t))
            };
            let lin_val = y_val * y_val;
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = lin_val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };
    let detail = Detail {
        clarity: 100.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    let y = 300;
    let orig_left = lin.data[(y * w + 380) * 3].sqrt();
    let orig_right = lin.data[(y * w + 420) * 3].sqrt();
    let orig_contrast = orig_right - orig_left;

    let res_left = res.data[(y * w + 380) * 3].sqrt();
    let res_right = res.data[(y * w + 420) * 3].sqrt();
    let res_contrast = res_right - res_left;

    assert!(
        res_contrast > orig_contrast,
        "Clarity +100 should raise contrast across soft edge: orig={orig_contrast}, res={res_contrast}"
    );
}

#[test]
fn clarity_leaves_blacks_and_whites_alone() {
    let w = 64;
    let h = 64;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let val = if x < 32 { 0.0f32 } else { 1.0f32 };
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

    for clarity_val in [100.0f32, -100.0f32] {
        let detail = Detail {
            clarity: clarity_val,
            ..Detail::default()
        };
        let res = apply_detail(&lin, &detail, 1.0);

        for y in 0..h {
            for x in 0..20 {
                for c in 0..3 {
                    let v = res.data[(y * w + x) * 3 + c];
                    assert!(
                        v.abs() < 1e-6,
                        "Black region should remain 0 under clarity {clarity_val}, got {v}"
                    );
                }
            }
            for x in 44..w {
                for c in 0..3 {
                    let v = res.data[(y * w + x) * 3 + c];
                    assert!(
                        (v - 1.0).abs() < 1e-6,
                        "White region should remain 1 under clarity {clarity_val}, got {v}"
                    );
                }
            }
        }
    }
}

#[test]
fn negative_clarity_softens() {
    let w = 120;
    let h = 120;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            // Midtone texture with alternating perturbation
            let y_val = 0.5f32 + 0.1f32 * if (x + y) % 2 == 0 { 1.0 } else { -1.0 };
            let lin_val = y_val * y_val;
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = lin_val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };
    let detail = Detail {
        clarity: -100.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    let n = (w * h) as f64;
    let orig_mean: f64 = lin.data.iter().map(|&v| v as f64).sum::<f64>() / (n * 3.0);
    let res_mean: f64 = res.data.iter().map(|&v| v as f64).sum::<f64>() / (n * 3.0);

    let orig_var: f64 = lin
        .data
        .iter()
        .map(|&v| (v as f64 - orig_mean).powi(2))
        .sum::<f64>()
        / (n * 3.0);
    let res_var: f64 = res
        .data
        .iter()
        .map(|&v| (v as f64 - res_mean).powi(2))
        .sum::<f64>()
        / (n * 3.0);

    assert!(
        (res_mean - orig_mean).abs() < 0.01,
        "Mean should remain unchanged under negative clarity: orig={orig_mean}, res={res_mean}"
    );
    assert!(
        res_var < orig_var * 0.9,
        "Variance should drop under negative clarity: orig_var={orig_var}, res_var={res_var}"
    );
}

#[test]
fn clarity_does_not_halo_at_a_hard_edge() {
    let w = 100;
    let h = 100;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let y_val = if x < 50 { 0.2f32 } else { 0.8f32 };
            let lin_val = y_val * y_val;
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = lin_val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };
    let detail = Detail {
        clarity: 100.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    // Step height is 0.8 - 0.2 = 0.6.
    // 3% overshoot is 0.6 * 0.03 = 0.018.
    let y_max = (0.8f32 + 0.018).powi(2);
    let y_min = (0.2f32 - 0.018).max(0.0).powi(2);

    for (i, &val) in res.data.iter().enumerate() {
        assert!(
            val <= y_max + 1e-4,
            "Pixel at {i} overshoots above 3%: val={val}, max={y_max}"
        );
        assert!(
            val >= y_min - 1e-4,
            "Pixel at {i} undershoots below 3%: val={val}, min={y_min}"
        );
    }
}

#[test]
fn texture_lifts_fine_detail_and_leaves_broad_gradients_alone() {
    let w = 1200;
    let h = 800;
    let mut data = vec![0.0f32; w * h * 3];
    // Broad ramp (0.2 -> 0.8 over 200 px, say x from 500 to 700) + high frequency ripple (amplitude 0.02, wavelength 6 px)
    for y in 0..h {
        for x in 0..w {
            let ramp = if x < 500 {
                0.2f32
            } else if x >= 700 {
                0.8f32
            } else {
                0.2f32 + 0.6f32 * ((x - 500) as f32 / 200.0)
            };
            let ripple = 0.02f32 * (2.0 * std::f32::consts::PI * x as f32 / 6.0).sin();
            let y_val = ramp + ripple;
            let lin_val = y_val * y_val;
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = lin_val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data,
    };
    let detail = Detail {
        texture: 100.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    // Ripple amplitude check near middle of the ramp (x around 580..620)
    let y_row = 400;
    let mut orig_amp_sum = 0.0f32;
    let mut res_amp_sum = 0.0f32;
    for period in 0..5 {
        // For wavelength 6: peak is at period*6 + 1.5, trough at period*6 + 4.5
        // Sampling integer points 1 and 4 or 2 and 5 gives a consistent amplitude measurement
        let x_peak = 580 + period * 6 + 1;
        let x_trough = 580 + period * 6 + 4;
        let orig_diff = (lin.data[(y_row * w + x_peak) * 3].sqrt()
            - lin.data[(y_row * w + x_trough) * 3].sqrt())
        .abs();
        let res_diff = (res.data[(y_row * w + x_peak) * 3].sqrt()
            - res.data[(y_row * w + x_trough) * 3].sqrt())
        .abs();
        orig_amp_sum += orig_diff;
        res_amp_sum += res_diff;
    }
    assert!(
        res_amp_sum > orig_amp_sum * 1.25,
        "Texture +100 should boost ripple amplitude by > 25%: orig={orig_amp_sum}, res={res_amp_sum}"
    );

    // Broad ramp slope check: average over integer periods of 6 px to cancel ripple
    let orig_ramp_start: f32 = (0..12)
        .map(|dx| lin.data[(y_row * w + 520 + dx) * 3].sqrt())
        .sum::<f32>()
        / 12.0;
    let orig_ramp_end: f32 = (0..12)
        .map(|dx| lin.data[(y_row * w + 660 + dx) * 3].sqrt())
        .sum::<f32>()
        / 12.0;
    let orig_slope = (orig_ramp_end - orig_ramp_start) / 140.0;

    let res_ramp_start: f32 = (0..12)
        .map(|dx| res.data[(y_row * w + 520 + dx) * 3].sqrt())
        .sum::<f32>()
        / 12.0;
    let res_ramp_end: f32 = (0..12)
        .map(|dx| res.data[(y_row * w + 660 + dx) * 3].sqrt())
        .sum::<f32>()
        / 12.0;
    let res_slope = (res_ramp_end - res_ramp_start) / 140.0;

    let slope_diff = (res_slope - orig_slope).abs() / orig_slope;
    assert!(
        slope_diff < 0.02,
        "Broad ramp slope should remain unchanged within < 2%: diff={slope_diff}"
    );
}

#[test]
fn dehaze_restores_a_hazy_scene() {
    let w = 120;
    let h = 120;
    let t_known = 0.4f32;
    let a_known = 0.85f32;

    let mut hazy_data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            // Synthetic scene with shadows (near 0) and highlights (near 0.8)
            let clear_val = if (x / 20 + y / 20) % 2 == 0 {
                0.05f32
            } else {
                0.80f32
            };
            let hazy_val = clear_val * t_known + a_known * (1.0 - t_known);
            for c in 0..3 {
                hazy_data[(y * w + x) * 3 + c] = hazy_val;
            }
        }
    }

    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data: hazy_data.clone(),
    };
    let detail = Detail {
        dehaze: 100.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    let n = (w * h * 3) as f64;
    let hazy_mean = hazy_data.iter().map(|&v| v as f64).sum::<f64>() / n;
    let hazy_std = (hazy_data
        .iter()
        .map(|&v| (v as f64 - hazy_mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();

    let res_mean = res.data.iter().map(|&v| v as f64).sum::<f64>() / n;
    let res_std = (res
        .data
        .iter()
        .map(|&v| (v as f64 - res_mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();

    assert!(
        res_std >= hazy_std * 1.50,
        "Recovered std dev should increase by >= 50%: hazy_std={hazy_std}, res_std={res_std}"
    );
}

#[test]
fn dehaze_leaves_a_clear_scene_nearly_alone() {
    let w = 120;
    let h = 120;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            // High-contrast scene with deep black shadows (dark channel = 0)
            let val = if (x / 10) % 2 == 0 || (y / 10) % 2 == 0 {
                0.0f32
            } else {
                0.7f32
            };
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data: data.clone(),
    };
    let detail = Detail {
        dehaze: 50.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    let n = (w * h * 3) as f64;
    let orig_mean = lin.data.iter().map(|&v| v as f64).sum::<f64>() / n;
    let res_mean = res.data.iter().map(|&v| v as f64).sum::<f64>() / n;

    let diff = (res_mean - orig_mean).abs() / orig_mean;
    assert!(
        diff < 0.03,
        "Dehaze +50 on a clear scene should change mean by < 3%: diff={diff}"
    );
}

#[test]
fn dehaze_does_not_tint_a_neutral_scene() {
    let w = 64;
    let h = 64;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let val = 0.3f32 + 0.3f32 * ((x + y) as f32 / 128.0);
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
        dehaze: 50.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    for idx in 0..w * h {
        let r = res.data[idx * 3];
        let g = res.data[idx * 3 + 1];
        let b = res.data[idx * 3 + 2];
        assert!(
            (r - g).abs() < 1e-6,
            "R and G must be identical for neutral scene at pixel {idx}: r={r}, g={g}"
        );
        assert!(
            (g - b).abs() < 1e-6,
            "G and B must be identical for neutral scene at pixel {idx}: g={g}, b={b}"
        );
    }
}

#[test]
fn negative_dehaze_adds_haze() {
    let w = 80;
    let h = 80;
    let mut data = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let val = if (x / 8 + y / 8) % 2 == 0 {
                0.1f32
            } else {
                0.9f32
            };
            for c in 0..3 {
                data[(y * w + x) * 3 + c] = val;
            }
        }
    }
    let lin = LinearBuffer {
        width: w as u32,
        height: h as u32,
        data: data.clone(),
    };
    let detail = Detail {
        dehaze: -50.0,
        ..Detail::default()
    };
    let res = apply_detail(&lin, &detail, 1.0);

    let n = (w * h * 3) as f64;
    let orig_mean = lin.data.iter().map(|&v| v as f64).sum::<f64>() / n;
    let orig_var = lin
        .data
        .iter()
        .map(|&v| (v as f64 - orig_mean).powi(2))
        .sum::<f64>()
        / n;

    let res_mean = res.data.iter().map(|&v| v as f64).sum::<f64>() / n;
    let res_var = res
        .data
        .iter()
        .map(|&v| (v as f64 - res_mean).powi(2))
        .sum::<f64>()
        / n;

    assert!(
        res_var <= orig_var * 0.70,
        "Dehaze -50 must reduce variance by >= 30%: orig_var={orig_var}, res_var={res_var}"
    );

    // Shadows lifted: check dark region pixels (originally 0.1)
    let min_orig = lin.data.iter().copied().fold(f32::INFINITY, f32::min);
    let min_res = res.data.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(
        min_res > min_orig + 0.05,
        "Shadows should be lifted: min_orig={min_orig}, min_res={min_res}"
    );
}

#[test]
fn presence_scales_with_the_proxy() {
    let w_half = 1200;
    let h_half = 800;
    let w_full = 2400;
    let h_full = 1600;

    // Generate deterministic test pattern
    let mut data_half = vec![0.0f32; w_half * h_half * 3];
    for y in 0..h_half {
        for x in 0..w_half {
            let u = x as f32 / w_half as f32;
            let v = y as f32 / h_half as f32;
            let val = 0.2 + 0.5 * u + 0.1 * (v * 10.0).sin();
            for c in 0..3 {
                data_half[(y * w_half + x) * 3 + c] = val;
            }
        }
    }
    let lin_half = LinearBuffer {
        width: w_half as u32,
        height: h_half as u32,
        data: data_half,
    };

    let mut data_full = vec![0.0f32; w_full * h_full * 3];
    for y in 0..h_full {
        for x in 0..w_full {
            let u = x as f32 / w_full as f32;
            let v = y as f32 / h_full as f32;
            let val = 0.2 + 0.5 * u + 0.1 * (v * 10.0).sin();
            for c in 0..3 {
                data_full[(y * w_full + x) * 3 + c] = val;
            }
        }
    }
    let lin_full = LinearBuffer {
        width: w_full as u32,
        height: h_full as u32,
        data: data_full,
    };

    let detail = Detail {
        clarity: 50.0,
        texture: 50.0,
        dehaze: 30.0,
        ..Detail::default()
    };

    let res_half = apply_detail(&lin_half, &detail, 0.5);
    let res_full = apply_detail(&lin_full, &detail, 1.0);

    // Downsample res_full by 2x area average
    let mut downsampled = vec![0.0f32; w_half * h_half * 3];
    for y in 0..h_half {
        for x in 0..w_half {
            for c in 0..3 {
                let v00 = res_full.data[((y * 2) * w_full + (x * 2)) * 3 + c];
                let v10 = res_full.data[((y * 2) * w_full + (x * 2 + 1)) * 3 + c];
                let v01 = res_full.data[((y * 2 + 1) * w_full + (x * 2)) * 3 + c];
                let v11 = res_full.data[((y * 2 + 1) * w_full + (x * 2 + 1)) * 3 + c];
                downsampled[(y * w_half + x) * 3 + c] = 0.25 * (v00 + v10 + v01 + v11);
            }
        }
    }

    // Mean absolute difference between downsampled full and half-resolution result
    let n = (w_half * h_half * 3) as f64;
    let mut sum_abs_diff = 0.0f64;
    for (&d, &h) in downsampled.iter().zip(res_half.data.iter()) {
        sum_abs_diff += (d as f64 - h as f64).abs();
    }
    let mean_abs_diff = sum_abs_diff / n;
    assert!(
        mean_abs_diff < 0.015,
        "Mean abs difference between downsampled full and half resolution should be < 0.015, got {mean_abs_diff}"
    );
}

#[test]
fn export_and_full_preview_agree_on_presence() {
    let buf = ImageBuffer::Rgb8 {
        width: 120,
        height: 80,
        data: (0..120 * 80 * 3).map(|i| ((i * 7) % 256) as u8).collect(),
    };
    let session = PreviewSession::new(&buf).unwrap();

    let recipe = AdjustmentRecipe {
        detail: Some(Detail {
            clarity: 40.0,
            texture: -20.0,
            dehaze: 15.0,
            ..Detail::default()
        }),
        ..AdjustmentRecipe::default()
    };

    let frame_full = session.render(&recipe, None, PreviewStage::Full).unwrap();
    let exported = apply_recipe_with_orientation(&buf, &recipe, None, 1).unwrap();
    let export_bytes = exported.as_rgb8().unwrap();

    let n_pixels = (buf.width() * buf.height()) as usize;
    for i in 0..n_pixels {
        for c in 0..3 {
            let p_val = frame_full.bytes[i * 4 + c];
            let e_val = export_bytes[i * 3 + c];
            assert!(
                (p_val as i32 - e_val as i32).abs() <= 1,
                "Export and PreviewStage::Full must agree on presence: pixel {i} channel {c}: preview {p_val}, export {e_val}"
            );
        }
    }
}

#[test]
fn a_detail_written_before_presence_still_loads() {
    let json = r#"{
        "sharpen_amount": 50.0,
        "sharpen_radius": 1.2,
        "sharpen_masking": 10.0,
        "noise_luminance": 25.0,
        "noise_colour": 30.0,
        "hot_pixels": true
    }"#;
    let detail: Detail = serde_json::from_str(json).unwrap();
    assert_eq!(detail.sharpen_amount, 50.0);
    assert_eq!(detail.sharpen_radius, 1.2);
    assert_eq!(detail.sharpen_masking, 10.0);
    assert_eq!(detail.noise_luminance, 25.0);
    assert_eq!(detail.noise_colour, 30.0);
    assert!(detail.hot_pixels);
    assert_eq!(detail.clarity, 0.0);
    assert_eq!(detail.texture, 0.0);
    assert_eq!(detail.dehaze, 0.0);
}
