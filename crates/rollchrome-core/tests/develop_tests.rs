mod support;

use phototools_core::error::Error;
use rollchrome_core::media::develop::colour::{camera_to_prophoto, invert_3x3_f32, mul_3x3_vec};
use rollchrome_core::media::develop::{
    develop, develop_linear, render, Demosaic, DevelopOptions, Look, OutputFormat,
};
use rollchrome_core::media::edit::lut::Lut;
use std::collections::BTreeSet;
use support::dng::{DngBuilder, PreviewIfd0, XTRANS_PATTERN};

const TEST_CAM_3X3: [[f32; 3]; 3] = [[0.9, -0.3, -0.1], [-0.4, 1.2, 0.2], [-0.05, 0.15, 0.7]];

/// Helper: builds sensor values from linear ProPhoto triples via the inverse pipeline.
fn prophoto_to_sensor(
    prophoto: [f32; 3],
    prophoto_to_cam: [[f32; 3]; 3],
    wb_coeffs: [f32; 3],
    black_level: u32,
    white_level: u32,
) -> ([f32; 3], [u16; 3]) {
    let cam_norm = mul_3x3_vec(prophoto_to_cam, prophoto);
    let min_wb = wb_coeffs[0].min(wb_coeffs[1]).min(wb_coeffs[2]);
    let wb_norm = [
        wb_coeffs[0] / min_wb,
        wb_coeffs[1] / min_wb,
        wb_coeffs[2] / min_wb,
    ];

    let cam_raw = [
        cam_norm[0] / wb_norm[0],
        cam_norm[1] / wb_norm[1],
        cam_norm[2] / wb_norm[2],
    ];

    let range = (white_level - black_level) as f32;
    let u16_vals = [
        (black_level as f32 + cam_raw[0] * range)
            .round()
            .clamp(0.0, 65535.0) as u16,
        (black_level as f32 + cam_raw[1] * range)
            .round()
            .clamp(0.0, 65535.0) as u16,
        (black_level as f32 + cam_raw[2] * range)
            .round()
            .clamp(0.0, 65535.0) as u16,
    ];

    (cam_raw, u16_vals)
}

// -------------------------------------------------------------------------
// Integration tests: 14 to 24
// -------------------------------------------------------------------------

#[test]
fn a_handmade_bayer_dng_develops_to_its_known_grey_and_patches() {
    let width = 96u32;
    let height = 96u32;
    let black = 512u32;
    let white = 16383u32;
    let wb = [1.0f32, 1.0f32, 1.0f32];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible camera matrix");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible prophoto-to-camera matrix");

    // Surround grey 0.18
    let grey_pro = [0.18f32, 0.18, 0.18];
    let (_grey_cam, grey_u16) = prophoto_to_sensor(grey_pro, prophoto2cam, wb, black, white);

    // Four patches chosen so sensor values land between 5% and 80% (0.05 and 0.80):
    // Patch 1: Warm skin/orange-red: [0.35, 0.22, 0.16]
    // Patch 2: Leaf green: [0.16, 0.32, 0.18]
    // Patch 3: Sky/cobalt blue: [0.15, 0.20, 0.38]
    // Patch 4: Yellow/amber: [0.38, 0.32, 0.14]
    let patches = [
        (
            "Patch 1 (warm)",
            [0.35f32, 0.22, 0.16],
            (12..36, 12..36),
            (24, 24),
        ),
        (
            "Patch 2 (green)",
            [0.16f32, 0.32, 0.18],
            (12..36, 60..84),
            (24, 72),
        ),
        (
            "Patch 3 (blue)",
            [0.15f32, 0.20, 0.38],
            (60..84, 12..36),
            (72, 24),
        ),
        (
            "Patch 4 (yellow)",
            [0.38f32, 0.32, 0.14],
            (60..84, 60..84),
            (72, 72),
        ),
    ];

    let mut patch_u16_map = Vec::new();
    for (name, pro, _, _) in &patches {
        let (cam, u16s) = prophoto_to_sensor(*pro, prophoto2cam, wb, black, white);
        for (c, &val) in cam.iter().enumerate() {
            assert!(
                (0.05..=0.80).contains(&val),
                "{} channel {} sensor value {} outside 5%..80%",
                name,
                c,
                val
            );
        }
        patch_u16_map.push(u16s);
    }

    // Build Bayer sensor mosaic: RGGB
    // (r%2==0, c%2==0)->R(0), (r%2==0, c%2==1)->G(1), (r%2==1, c%2==0)->G(1), (r%2==1, c%2==1)->B(2)
    let bayer_color_at = |r: usize, c: usize| -> usize {
        match (r % 2, c % 2) {
            (0, 0) => 0,
            (0, 1) => 1,
            (1, 0) => 1,
            (1, 1) => 2,
            _ => unreachable!(),
        }
    };

    let mut raw_pixels = vec![0u16; (width * height) as usize];
    for r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let ch = bayer_color_at(r, c);
            let mut pixel_val = grey_u16[ch];
            for (idx, (_, _, (r_range, c_range), _)) in patches.iter().enumerate() {
                if r_range.contains(&r) && c_range.contains(&c) {
                    pixel_val = patch_u16_map[idx][ch];
                    break;
                }
            }
            raw_pixels[r * width as usize + c] = pixel_val;
        }
    }

    let builder = DngBuilder::new_bayer(width, height);
    let dng_bytes = builder.build(&raw_pixels);

    let res =
        develop_linear(&dng_bytes, &DevelopOptions::default()).expect("develop_linear Bayer DNG");
    assert_eq!(res.demosaic, Demosaic::Bayer);
    assert_eq!(res.width, width);
    assert_eq!(res.height, height);

    // Verify grey field at centre (48, 48)
    let grey_dev = res.pixels[48 * width as usize + 48];
    for c in 0..3 {
        let rel_err = (grey_dev[c] - grey_pro[c]).abs() / grey_pro[c];
        assert!(
            rel_err <= 0.01,
            "Grey surround ch {}: got {}, expected {}, rel err {}",
            c,
            grey_dev[c],
            grey_pro[c],
            rel_err
        );
    }

    // Verify patches at centre
    for (name, pro, _, (cr, cc)) in &patches {
        let dev = res.pixels[cr * width as usize + cc];
        for c in 0..3 {
            let rel_err = (dev[c] - pro[c]).abs() / pro[c];
            assert!(
                rel_err <= 0.01,
                "{} ch {}: got {}, expected {}, rel err {}",
                name,
                c,
                dev[c],
                pro[c],
                rel_err
            );
        }
    }
}

#[test]
fn a_handmade_xtrans_dng_develops_to_its_known_grey_and_patches() {
    let width = 96u32;
    let height = 96u32;
    let black = 512u32;
    let white = 16383u32;
    let wb = [1.0f32, 1.0f32, 1.0f32];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible camera matrix");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible prophoto-to-camera matrix");

    let grey_pro = [0.18f32, 0.18, 0.18];
    let (_grey_cam, grey_u16) = prophoto_to_sensor(grey_pro, prophoto2cam, wb, black, white);

    let patches = [
        (
            "Patch 1 (warm)",
            [0.35f32, 0.22, 0.16],
            (12..36, 12..36),
            (24, 24),
        ),
        (
            "Patch 2 (green)",
            [0.16f32, 0.32, 0.18],
            (12..36, 60..84),
            (24, 72),
        ),
        (
            "Patch 3 (blue)",
            [0.15f32, 0.20, 0.38],
            (60..84, 12..36),
            (72, 24),
        ),
        (
            "Patch 4 (yellow)",
            [0.38f32, 0.32, 0.14],
            (60..84, 60..84),
            (72, 72),
        ),
    ];

    let mut patch_u16_map = Vec::new();
    for (name, pro, _, _) in &patches {
        let (cam, u16s) = prophoto_to_sensor(*pro, prophoto2cam, wb, black, white);
        for (c, &val) in cam.iter().enumerate() {
            assert!(
                (0.05..=0.80).contains(&val),
                "{} channel {} sensor value {} outside 5%..80%",
                name,
                c,
                val
            );
        }
        patch_u16_map.push(u16s);
    }

    let xtrans_color_at = |r: usize, c: usize| -> usize {
        let pr = r % 6;
        let pc = c % 6;
        XTRANS_PATTERN[pr * 6 + pc] as usize
    };

    let mut raw_pixels = vec![0u16; (width * height) as usize];
    for r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let ch = xtrans_color_at(r, c);
            let mut pixel_val = grey_u16[ch];
            for (idx, (_, _, (r_range, c_range), _)) in patches.iter().enumerate() {
                if r_range.contains(&r) && c_range.contains(&c) {
                    pixel_val = patch_u16_map[idx][ch];
                    break;
                }
            }
            raw_pixels[r * width as usize + c] = pixel_val;
        }
    }

    let builder = DngBuilder::new_xtrans(width, height);
    let dng_bytes = builder.build(&raw_pixels);

    let res =
        develop_linear(&dng_bytes, &DevelopOptions::default()).expect("develop_linear X-Trans DNG");
    assert_eq!(res.demosaic, Demosaic::XTrans);
    assert_eq!(res.width, width);
    assert_eq!(res.height, height);

    // Verify grey surround at centre (48, 48)
    let grey_dev = res.pixels[48 * width as usize + 48];
    for c in 0..3 {
        let rel_err = (grey_dev[c] - grey_pro[c]).abs() / grey_pro[c];
        assert!(
            rel_err <= 0.01,
            "X-Trans Grey surround ch {}: got {}, expected {}, rel err {}",
            c,
            grey_dev[c],
            grey_pro[c],
            rel_err
        );
    }

    // Verify patches at centre
    for (name, pro, _, (cr, cc)) in &patches {
        let dev = res.pixels[cr * width as usize + cc];
        for c in 0..3 {
            let rel_err = (dev[c] - pro[c]).abs() / pro[c];
            assert!(
                rel_err <= 0.01,
                "X-Trans {} ch {}: got {}, expected {}, rel err {}",
                name,
                c,
                dev[c],
                pro[c],
                rel_err
            );
        }
    }
}

#[test]
fn the_output_keeps_more_than_256_levels_of_a_smooth_gradient() {
    let width = 2048u32;
    let height = 16u32;
    let black = 512u32;
    let white = 16383u32;
    let range = (white - black) as f32;

    let mut raw_pixels = Vec::with_capacity((width * height) as usize);
    for _r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let lin = 0.02 + 0.88 * (c as f32 / (width - 1) as f32);
            let u16_val = (black as f32 + lin * range).round().clamp(0.0, 65535.0) as u16;
            raw_pixels.push(u16_val);
        }
    }

    let builder = DngBuilder::new_xtrans(width, height);
    let dng_bytes = builder.build(&raw_pixels);

    let developed =
        develop(&dng_bytes, &DevelopOptions::default()).expect("develop 14-bit gradient DNG");

    let mid_row = 8;
    let mut unique_greens = BTreeSet::new();
    for c in 0..width {
        let pixel = developed.image.get_pixel(c, mid_row);
        unique_greens.insert(pixel[1]);
    }

    assert!(
        unique_greens.len() >= 1024,
        "Distinct green levels = {} < 1024",
        unique_greens.len()
    );
}

#[test]
fn clipped_highlights_stay_neutral() {
    let width = 96u32;
    let height = 96u32;
    let black = 512u32;
    let white = 16383u32;

    // AsShotNeutral: (0.5, 1.0, 0.7) -> wb_coeffs = [2.0, 1.0, 1.4286]
    let neutral = [0.5f32, 1.0, 0.7];
    let wb = [1.0 / neutral[0], 1.0 / neutral[1], 1.0 / neutral[2]];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible");

    let grey_pro = [0.18f32, 0.18, 0.18];
    let (_, grey_u16) = prophoto_to_sensor(grey_pro, prophoto2cam, wb, black, white);

    // Test Bayer
    {
        let mut raw_pixels = vec![0u16; (width * height) as usize];
        for r in 0..(height as usize) {
            for c in 0..(width as usize) {
                let ch = match (r % 2, c % 2) {
                    (0, 0) => 0,
                    (0, 1) => 1,
                    (1, 0) => 1,
                    (1, 1) => 2,
                    _ => unreachable!(),
                };
                if (38..=58).contains(&r) && (38..=58).contains(&c) {
                    raw_pixels[r * width as usize + c] = white as u16; // Clipped highlight
                } else {
                    raw_pixels[r * width as usize + c] = grey_u16[ch];
                }
            }
        }

        let mut builder = DngBuilder::new_bayer(width, height);
        builder.as_shot_neutral = Some(neutral);
        let dng_bytes = builder.build(&raw_pixels);

        let dev = develop(&dng_bytes, &DevelopOptions::default()).expect("develop clipped Bayer");
        let clipped = dev.image.get_pixel(48, 48);
        assert_eq!(clipped[0], 65535, "Bayer R not clipped to 65535");
        assert_eq!(clipped[1], 65535, "Bayer G not clipped to 65535");
        assert_eq!(clipped[2], 65535, "Bayer B not clipped to 65535");

        let surround = dev.image.get_pixel(10, 10);
        let g = surround[1] as f32;
        let diff_r = (surround[0] as f32 - g).abs() / g;
        let diff_b = (surround[2] as f32 - g).abs() / g;
        assert!(diff_r <= 0.01, "Bayer surround R mismatch: {}", diff_r);
        assert!(diff_b <= 0.01, "Bayer surround B mismatch: {}", diff_b);
    }

    // Test X-Trans
    {
        let mut raw_pixels = vec![0u16; (width * height) as usize];
        for r in 0..(height as usize) {
            for c in 0..(width as usize) {
                let pr = r % 6;
                let pc = c % 6;
                let ch = XTRANS_PATTERN[pr * 6 + pc] as usize;
                if (38..=58).contains(&r) && (38..=58).contains(&c) {
                    raw_pixels[r * width as usize + c] = white as u16; // Clipped highlight
                } else {
                    raw_pixels[r * width as usize + c] = grey_u16[ch];
                }
            }
        }

        let mut builder = DngBuilder::new_xtrans(width, height);
        builder.as_shot_neutral = Some(neutral);
        let dng_bytes = builder.build(&raw_pixels);

        let dev = develop(&dng_bytes, &DevelopOptions::default()).expect("develop clipped X-Trans");
        let clipped = dev.image.get_pixel(48, 48);
        assert_eq!(clipped[0], 65535, "X-Trans R not clipped to 65535");
        assert_eq!(clipped[1], 65535, "X-Trans G not clipped to 65535");
        assert_eq!(clipped[2], 65535, "X-Trans B not clipped to 65535");

        let surround = dev.image.get_pixel(10, 10);
        let g = surround[1] as f32;
        let diff_r = (surround[0] as f32 - g).abs() / g;
        let diff_b = (surround[2] as f32 - g).abs() / g;
        assert!(diff_r <= 0.01, "X-Trans surround R mismatch: {}", diff_r);
        assert!(diff_b <= 0.01, "X-Trans surround B mismatch: {}", diff_b);
    }
}

#[test]
fn an_exposure_offset_of_one_stop_doubles_the_linear_values() {
    let width = 96u32;
    let height = 96u32;
    let black = 512u32;
    let white = 16383u32;
    let wb = [1.0f32, 1.0f32, 1.0f32];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible");
    let (_, grey_u16) = prophoto_to_sensor([0.18, 0.18, 0.18], prophoto2cam, wb, black, white);

    let mut raw_pixels = vec![0u16; (width * height) as usize];
    for r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let ch = match (r % 2, c % 2) {
                (0, 0) => 0,
                (0, 1) => 1,
                (1, 0) => 1,
                (1, 1) => 2,
                _ => unreachable!(),
            };
            raw_pixels[r * width as usize + c] = grey_u16[ch];
        }
    }

    let builder = DngBuilder::new_bayer(width, height);
    let dng_bytes = builder.build(&raw_pixels);

    let dev0 = develop_linear(&dng_bytes, &DevelopOptions { exposure_ev: 0.0 }).unwrap();
    let dev_plus1 = develop_linear(&dng_bytes, &DevelopOptions { exposure_ev: 1.0 }).unwrap();
    let dev_minus1 = develop_linear(&dng_bytes, &DevelopOptions { exposure_ev: -1.0 }).unwrap();

    let p0 = dev0.pixels[48 * width as usize + 48];
    let pp = dev_plus1.pixels[48 * width as usize + 48];
    let pm = dev_minus1.pixels[48 * width as usize + 48];

    for c in 0..3 {
        let ratio_plus = pp[c] / p0[c];
        let ratio_minus = pm[c] / p0[c];

        assert!(
            (ratio_plus - 2.0).abs() <= 0.002,
            "+1 EV ratio = {} (expected 2.0 +- 0.1%)",
            ratio_plus
        );
        assert!(
            (ratio_minus - 0.5).abs() <= 0.0005,
            "-1 EV ratio = {} (expected 0.5 +- 0.1%)",
            ratio_minus
        );
    }
}

#[test]
fn an_exposure_outside_four_stops_is_refused() {
    let builder = DngBuilder::new_bayer(96, 96);
    let pixels = vec![3000u16; 96 * 96];
    let dng_bytes = builder.build(&pixels);

    let err_pos = develop_linear(&dng_bytes, &DevelopOptions { exposure_ev: 4.5 });
    match err_pos {
        Err(Error::Refused(_)) => {}
        other => panic!("Expected Error::Refused for +4.5 EV, got {:?}", other),
    }

    let err_neg = develop_linear(&dng_bytes, &DevelopOptions { exposure_ev: -4.5 });
    match err_neg {
        Err(Error::Refused(_)) => {}
        other => panic!("Expected Error::Refused for -4.5 EV, got {:?}", other),
    }

    let err_nan = develop_linear(
        &dng_bytes,
        &DevelopOptions {
            exposure_ev: f32::NAN,
        },
    );
    match err_nan {
        Err(Error::Refused(_)) => {}
        other => panic!("Expected Error::Refused for NaN EV, got {:?}", other),
    }
}

#[test]
fn bytes_that_are_not_a_raw_are_refused_with_a_reason() {
    // 1. Valid JPEG image
    let mut jpeg_bytes = Vec::new();
    let img = image::RgbImage::new(16, 16);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg_bytes);
    encoder
        .encode(img.as_raw(), 16, 16, image::ExtendedColorType::Rgb8)
        .expect("JPEG encode");

    let res_jpeg = develop_linear(&jpeg_bytes, &DevelopOptions::default());
    assert!(res_jpeg.is_err(), "JPEG should be refused");

    // 2. 100 random bytes
    let random_bytes = vec![0x42u8; 100];
    let res_random = develop_linear(&random_bytes, &DevelopOptions::default());
    assert!(res_random.is_err(), "Random bytes should be refused");
}

#[test]
fn a_dng_with_an_unsupported_layout_is_refused_rather_than_previewed() {
    let mut builder = DngBuilder::new_bayer(96, 96);
    builder.photometric = 34892; // LinearRaw
    builder.samples_per_pixel = 3;

    let pixels = vec![3000u16; 96 * 96 * 3];
    let dng_bytes = builder.build(&pixels);

    let res = develop_linear(&dng_bytes, &DevelopOptions::default());
    match res {
        Err(Error::Refused(reason)) => {
            assert!(
                reason.contains("linear DNG"),
                "Refusal reason should name layout, got: {}",
                reason
            );
        }
        other => panic!("Expected Refused(\"linear DNG\"), got {:?}", other),
    }
}

#[test]
fn the_output_takes_the_default_crop() {
    let width = 120u32;
    let height = 120u32;

    let mut builder = DngBuilder::new_bayer(width, height);
    // Active area: top=4, left=4, bottom=116, right=116 (112x112)
    builder.active_area = Some([4, 4, 116, 116]);
    // Crop: origin (6, 6), size (80, 80)
    builder.default_crop_origin = Some([6, 6]);
    builder.default_crop_size = Some([80, 80]);

    let pixels = vec![3000u16; (width * height) as usize];
    let dng_bytes = builder.build(&pixels);

    let res = develop_linear(&dng_bytes, &DevelopOptions::default()).expect("develop_linear crop");
    assert_eq!(res.width, 80, "Expected crop width 80");
    assert_eq!(res.height, 80, "Expected crop height 80");
}

#[test]
fn the_orientation_is_reported_not_applied() {
    let width = 96u32;
    let height = 96u32;

    let mut builder = DngBuilder::new_bayer(width, height);
    builder.orientation = Some(6); // 90° CW

    let pixels = vec![3000u16; (width * height) as usize];
    let dng_bytes = builder.build(&pixels);

    let res =
        develop_linear(&dng_bytes, &DevelopOptions::default()).expect("develop_linear orientation");
    assert_eq!(res.orientation, 6, "Expected orientation 6");
    assert_eq!(res.width, width, "Width should be unrotated");
    assert_eq!(res.height, height, "Height should be unrotated");
}

#[test]
fn develop_never_reads_the_embedded_preview() {
    let width = 96u32;
    let height = 96u32;
    let black = 512u32;
    let white = 16383u32;
    let wb = [1.0f32, 1.0f32, 1.0f32];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible");

    // Raw CFA is solid grey (0.18)
    let (_, grey_u16) = prophoto_to_sensor([0.18, 0.18, 0.18], prophoto2cam, wb, black, white);
    let mut raw_pixels = vec![0u16; (width * height) as usize];
    for r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let ch = match (r % 2, c % 2) {
                (0, 0) => 0,
                (0, 1) => 1,
                (1, 0) => 1,
                (1, 1) => 2,
                _ => unreachable!(),
            };
            raw_pixels[r * width as usize + c] = grey_u16[ch];
        }
    }

    // IFD0 preview is solid red 32x32 RGB
    let preview_pixels = [255u8, 0, 0].repeat(32 * 32);

    let mut builder = DngBuilder::new_bayer(width, height);
    builder.preview_ifd0 = Some(PreviewIfd0 {
        width: 32,
        height: 32,
        pixels: preview_pixels,
    });

    let dng_bytes = builder.build(&raw_pixels);

    let res = develop_linear(&dng_bytes, &DevelopOptions::default())
        .expect("develop_linear with preview IFD0");

    // Output must be grey (R, G, B within 1% of each other), NEVER red
    let mid = res.pixels[48 * width as usize + 48];
    let diff_rg = (mid[0] - mid[1]).abs() / mid[1];
    let diff_bg = (mid[2] - mid[1]).abs() / mid[1];
    assert!(
        diff_rg <= 0.01,
        "Expected grey channels, R and G differ by {}",
        diff_rg
    );
    assert!(
        diff_bg <= 0.01,
        "Expected grey channels, B and G differ by {}",
        diff_bg
    );
    assert!(
        mid[0] < 0.30,
        "Red channel is high ({}), likely preview was read instead of sensor data!",
        mid[0]
    );
}

#[test]
fn a_developed_raw_renders_end_to_end() {
    let width = 96u32;
    let height = 96u32;
    let raw_pixels = vec![3000u16; (width * height) as usize];
    let builder = DngBuilder::new_xtrans(width, height);
    let dng_bytes = builder.build(&raw_pixels);

    let dev = develop(&dng_bytes, &DevelopOptions::default()).expect("develop X-Trans DNG");

    // Inverting 17^3 .cube
    let mut cube_str = String::from("TITLE \"Invert\"\nLUT_3D_SIZE 17\n");
    for b in 0..17 {
        for g in 0..17 {
            for r in 0..17 {
                let rf = 1.0 - r as f32 / 16.0;
                let gf = 1.0 - g as f32 / 16.0;
                let bf = 1.0 - b as f32 / 16.0;
                use std::fmt::Write;
                let _ = writeln!(&mut cube_str, "{rf:.6} {gf:.6} {bf:.6}");
            }
        }
    }
    let lut = Lut::from_cube("invert.cube", cube_str.as_bytes()).expect("valid LUT");
    let look = Look::srgb(lut);

    // Render JPEG
    let rendered_jpeg = render(&dev, Some(&look), OutputFormat::Jpeg { quality: 90 })
        .expect("render JPEG with look");
    assert_eq!(rendered_jpeg.extension, "jpg");
    assert_eq!(rendered_jpeg.width, dev.image.width());
    assert_eq!(rendered_jpeg.height, dev.image.height());
    let dec_jpeg = image::load_from_memory(&rendered_jpeg.bytes).expect("decode JPEG");
    assert_eq!(dec_jpeg.width(), dev.image.width());
    assert_eq!(dec_jpeg.height(), dev.image.height());

    // Render TIFF
    let rendered_tiff =
        render(&dev, Some(&look), OutputFormat::Tiff).expect("render TIFF with look");
    assert_eq!(rendered_tiff.extension, "tif");
    assert_eq!(rendered_tiff.width, dev.image.width());
    assert_eq!(rendered_tiff.height, dev.image.height());

    let mut tiff_dec = tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_tiff.bytes))
        .expect("decode TIFF");
    assert_eq!(
        tiff_dec.dimensions().expect("TIFF dimensions"),
        (dev.image.width(), dev.image.height())
    );
    assert_eq!(
        tiff_dec.colortype().expect("TIFF colortype"),
        tiff::ColorType::RGB(16)
    );

    // Profile check (tag 34675)
    let profile_tag = tiff_dec
        .find_tag(tiff::tags::Tag::IccProfile)
        .expect("find tag")
        .expect("ICC tag 34675 present");
    let icc_bytes: Vec<u8> = match profile_tag {
        tiff::decoder::ifd::Value::List(l) => l
            .into_iter()
            .map(|v| match v {
                tiff::decoder::ifd::Value::Byte(b) => b,
                _ => panic!("expected Byte in list"),
            })
            .collect(),
        tiff::decoder::ifd::Value::Byte(b) => vec![b],
        _ => panic!("unexpected tag type"),
    };
    let profile = moxcms::ColorProfile::new_from_slice(&icc_bytes).expect("parse ICC profile");
    assert!(profile.red_trc.is_some());

    match tiff_dec.read_image().expect("read TIFF pixels") {
        tiff::decoder::DecodingResult::U16(pixels) => {
            assert_eq!(
                pixels.len(),
                (dev.image.width() * dev.image.height() * 3) as usize
            );
        }
        _ => panic!("expected U16 decoding result"),
    }
}

#[test]
fn a_default_crop_wider_than_the_active_area_still_develops() {
    let width = 96u32;
    let height = 64u32;
    let black = 512u32;
    let white = 16383u32;
    let wb = [1.0f32, 1.0f32, 1.0f32];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible camera matrix");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible prophoto-to-camera matrix");

    let grey_pro = [0.18f32, 0.18, 0.18];
    let (_grey_cam, grey_u16) = prophoto_to_sensor(grey_pro, prophoto2cam, wb, black, white);

    let bayer_color_at = |r: usize, c: usize| -> usize {
        match (r % 2, c % 2) {
            (0, 0) => 0,
            (0, 1) => 1,
            (1, 0) => 1,
            (1, 1) => 2,
            _ => unreachable!(),
        }
    };

    let mut raw_pixels = vec![0u16; (width * height) as usize];
    for r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let ch = bayer_color_at(r, c);
            raw_pixels[r * width as usize + c] = grey_u16[ch];
        }
    }

    let mut builder = DngBuilder::new_bayer(width, height);
    // Active area is 80 columns wide: [top=0, left=0, bottom=64, right=80]
    builder.active_area = Some([0, 0, height, 80]);
    // Crop is 88 columns wide: origin [0, 0], size [88, 64]
    builder.default_crop_origin = Some([0, 0]);
    builder.default_crop_size = Some([88, height]);

    let dng_bytes = builder.build(&raw_pixels);
    let res = develop_linear(&dng_bytes, &DevelopOptions::default())
        .expect("develop_linear Bayer DNG with wide crop");

    assert_eq!(res.demosaic, Demosaic::Bayer);
    assert_eq!(res.width, 88);
    assert_eq!(res.height, height);

    // Verify grey field at centre of crop
    let grey_dev = res.pixels[(32 * 88 + 44) as usize];
    for c in 0..3 {
        let rel_err = (grey_dev[c] - grey_pro[c]).abs() / grey_pro[c];
        assert!(
            rel_err <= 0.01,
            "Grey surround ch {}: got {}, expected {}, rel err {}",
            c,
            grey_dev[c],
            grey_pro[c],
            rel_err
        );
    }

    // Also verify grey field in the columns 80..88 (outside active area)
    let grey_dev_outside = res.pixels[(32 * 88 + 84) as usize];
    for c in 0..3 {
        let rel_err = (grey_dev_outside[c] - grey_pro[c]).abs() / grey_pro[c];
        assert!(
            rel_err <= 0.01,
            "Grey outside active area ch {}: got {}, expected {}, rel err {}",
            c,
            grey_dev_outside[c],
            grey_pro[c],
            rel_err
        );
    }
}

#[test]
fn an_xtrans_crop_beyond_the_active_area_keeps_its_colours() {
    let width = 96u32;
    let height = 72u32;
    let black = 512u32;
    let white = 16383u32;
    let wb = [1.0f32, 1.0f32, 1.0f32];

    let cam2prophoto = camera_to_prophoto(TEST_CAM_3X3).expect("invertible camera matrix");
    let prophoto2cam = invert_3x3_f32(cam2prophoto).expect("invertible prophoto-to-camera matrix");

    let grey_pro = [0.18f32, 0.18, 0.18];
    let (_grey_cam, grey_u16) = prophoto_to_sensor(grey_pro, prophoto2cam, wb, black, white);

    // Patches arranged so at least one patch lies partly in the columns outside the active area (width 78).
    // Columns outside active area are 78..90.
    let patches = [
        (
            "Patch 1 (warm)",
            [0.35f32, 0.22, 0.16],
            (12..36, 12..36),
            (24, 24),
        ),
        (
            "Patch 2 (green straddling)",
            [0.16f32, 0.32, 0.18],
            (12..36, 66..90),
            (24, 78),
        ),
        (
            "Patch 3 (blue)",
            [0.15f32, 0.20, 0.38],
            (42..66, 12..36),
            (54, 24),
        ),
        (
            "Patch 4 (yellow straddling)",
            [0.38f32, 0.32, 0.14],
            (42..66, 66..90),
            (54, 78),
        ),
    ];

    let mut patch_u16_map = Vec::new();
    for (name, pro, _, _) in &patches {
        let (cam, u16s) = prophoto_to_sensor(*pro, prophoto2cam, wb, black, white);
        for (c, &val) in cam.iter().enumerate() {
            assert!(
                (0.05..=0.80).contains(&val),
                "{} channel {} sensor value {} outside 5%..80%",
                name,
                c,
                val
            );
        }
        patch_u16_map.push(u16s);
    }

    let xtrans_color_at = |r: usize, c: usize| -> usize {
        let pr = r % 6;
        let pc = c % 6;
        XTRANS_PATTERN[pr * 6 + pc] as usize
    };

    let mut raw_pixels = vec![0u16; (width * height) as usize];
    for r in 0..(height as usize) {
        for c in 0..(width as usize) {
            let ch = xtrans_color_at(r, c);
            let mut pixel_val = grey_u16[ch];
            for (idx, (_, _, (r_range, c_range), _)) in patches.iter().enumerate() {
                if r_range.contains(&r) && c_range.contains(&c) {
                    pixel_val = patch_u16_map[idx][ch];
                    break;
                }
            }
            raw_pixels[r * width as usize + c] = pixel_val;
        }
    }

    let mut builder = DngBuilder::new_xtrans(width, height);
    // Active area is 78 columns wide: [top=0, left=0, bottom=72, right=78]
    builder.active_area = Some([0, 0, height, 78]);
    // Crop is 90 columns wide: origin [0, 0], size [90, 72]
    builder.default_crop_origin = Some([0, 0]);
    builder.default_crop_size = Some([90, height]);

    let dng_bytes = builder.build(&raw_pixels);
    let res = develop_linear(&dng_bytes, &DevelopOptions::default())
        .expect("develop_linear X-Trans DNG with wide crop");

    assert_eq!(res.demosaic, Demosaic::XTrans);
    assert_eq!(res.width, 90);
    assert_eq!(res.height, height);

    // Verify grey surround at (10, 50)
    let grey_dev = res.pixels[10 * 90 + 50];
    for c in 0..3 {
        let rel_err = (grey_dev[c] - grey_pro[c]).abs() / grey_pro[c];
        assert!(
            rel_err <= 0.01,
            "X-Trans Grey surround ch {}: got {}, expected {}, rel err {}",
            c,
            grey_dev[c],
            grey_pro[c],
            rel_err
        );
    }

    // Verify patches at centre (including those straddling columns outside the active area)
    for (name, pro, _, (cr, cc)) in &patches {
        let dev = res.pixels[cr * 90 + cc];
        for c in 0..3 {
            let rel_err = (dev[c] - pro[c]).abs() / pro[c];
            assert!(
                rel_err <= 0.01,
                "X-Trans {} ch {}: got {}, expected {}, rel err {}",
                name,
                c,
                dev[c],
                pro[c],
                rel_err
            );
        }
    }
}

#[test]
fn a_crop_outside_the_sensor_is_refused_not_panicked() {
    let width = 96u32;
    let height = 64u32;
    let raw_pixels = vec![1000u16; (width * height) as usize];

    let mut builder = DngBuilder::new_bayer(width, height);
    // Crop origin [10, 0], size [90, 64] -> right edge 10 + 90 = 100 > width (96)
    builder.default_crop_origin = Some([10, 0]);
    builder.default_crop_size = Some([90, height]);

    let dng_bytes = builder.build(&raw_pixels);
    let err = develop_linear(&dng_bytes, &DevelopOptions::default())
        .expect_err("crop extending beyond sensor must return Err");

    match err {
        Error::Refused(msg) => {
            // rawler's DNG decoder catches its own panic when calculating crop offsets
            // beyond image bounds and returns DecoderFailed ("Caught a panic while decoding"),
            // which develop_linear maps to Error::Refused. If rawler had decoded it, our
            // demosaic sensor boundary check would also return Error::Refused naming the crop.
            assert!(
                msg.contains("crop") || msg.contains("RAW decode failed"),
                "expected crop or decode failure in Error::Refused: {msg}"
            );
        }
        other => panic!("expected Error::Refused, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// RP-1 Develop screen & preview tests
// ---------------------------------------------------------------------------

#[test]
fn a_preview_fits_the_long_edge_and_keeps_the_aspect_ratio() {
    let width = 240u32;
    let height = 160u32; // 3:2 aspect ratio
    let pixels = vec![2000u16; (width * height) as usize];
    let dng = DngBuilder::new_bayer(width, height).build(&pixels);

    let max_edge = 120u32;
    let preview = rollchrome_core::media::develop::DevelopPreview::new(&dng, 0.0, max_edge)
        .expect("develop preview");
    let rendered = preview.render(None).expect("render preview");

    assert_eq!(rendered.width, 120);
    assert_eq!(rendered.height, 80);
    assert_eq!(rendered.extension, "jpg");
}

#[test]
fn a_preview_is_upright_for_a_rotated_raw() {
    let width = 240u32;
    let height = 160u32; // Landscape stored: 240x160
    let pixels = vec![2000u16; (width * height) as usize];
    let mut builder = DngBuilder::new_bayer(width, height);
    builder.orientation = Some(6); // 90 degrees clockwise -> Upright portrait: 160x240
    let dng = builder.build(&pixels);

    let max_edge = 120u32;
    let preview = rollchrome_core::media::develop::DevelopPreview::new(&dng, 0.0, max_edge)
        .expect("develop preview");
    let rendered = preview.render(None).expect("render preview");

    // Upright should be portrait: width 80, height 120
    assert_eq!(rendered.width, 80);
    assert_eq!(rendered.height, 120);
}

#[test]
fn a_preview_with_a_look_matches_the_full_render_within_two_levels() {
    let width = 96u32;
    let height = 64u32;
    let pixels = vec![4000u16; (width * height) as usize];
    let dng = DngBuilder::new_bayer(width, height).build(&pixels);

    let look = rollchrome_core::media::looks::builtin::builtin_look("fujifilm-classic-chrome")
        .expect("builtin look");

    // Full render
    let full_dev = develop(&dng, &DevelopOptions::default()).expect("develop");
    let full_rendered =
        render(&full_dev, Some(&look), OutputFormat::Jpeg { quality: 90 }).expect("full render");
    let full_img = image::load_from_memory(&full_rendered.bytes)
        .expect("decode full jpeg")
        .to_rgb8();

    // Preview render (reduced to max edge 48)
    let preview = rollchrome_core::media::develop::DevelopPreview::new(&dng, 0.0, 48)
        .expect("preview develop");
    let prev_rendered = preview.render(Some(&look)).expect("preview render");
    let prev_img = image::load_from_memory(&prev_rendered.bytes)
        .expect("decode preview jpeg")
        .to_rgb8();

    // Sample center pixel of both uniform images
    let full_px = full_img.get_pixel(full_img.width() / 2, full_img.height() / 2);
    let prev_px = prev_img.get_pixel(prev_img.width() / 2, prev_img.height() / 2);

    for c in 0..3 {
        let diff = (full_px[c] as i32 - prev_px[c] as i32).abs();
        assert!(
            diff <= 2,
            "Channel {c} diff {diff} > 2 levels: full={}, preview={}",
            full_px[c],
            prev_px[c]
        );
    }
}

#[test]
fn the_builtin_look_list_has_twelve_entries_that_all_resolve() {
    let list = rollchrome_core::media::looks::builtin::builtin_look_list();
    assert_eq!(list.len(), 12, "expected exactly 12 built-in looks");

    for info in &list {
        assert!(
            info.id.starts_with("builtin:"),
            "id must start with 'builtin:': {}",
            info.id
        );
        assert!(!info.name.is_empty());
        assert!(!info.maker.is_empty());

        let raw_id = info.id.strip_prefix("builtin:").unwrap();
        let look = rollchrome_core::media::looks::builtin::builtin_look(raw_id);
        assert!(
            look.is_ok(),
            "built-in look '{}' failed to resolve: {:?}",
            info.id,
            look.err()
        );
    }
}

#[test]
fn a_folder_lists_only_its_raw_files_sorted() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path();

    // Create a mix of RAW files, non-RAW files, and a directory
    std::fs::write(folder.join("IMG_0003.CR2"), b"raw").unwrap();
    std::fs::write(folder.join("DSCF0002.dng"), b"raw").unwrap();
    std::fs::write(folder.join("DSCF0001.RAF"), b"raw").unwrap();
    std::fs::write(folder.join("preview.jpg"), b"jpeg").unwrap();
    std::fs::write(folder.join("notes.txt"), b"text").unwrap();
    std::fs::create_dir(folder.join("subfolder")).unwrap();

    let names = rollchrome_core::tools::develop_folder::raw_names(folder).expect("list raw names");

    assert_eq!(
        names,
        vec![
            "DSCF0001.RAF".to_string(),
            "DSCF0002.dng".to_string(),
            "IMG_0003.CR2".to_string(),
        ]
    );
}

struct TestResolveStore {
    custom_look: Look,
}

impl rollchrome_core::tools::develop::DevelopStore for TestResolveStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        Ok(BTreeSet::new())
    }
    fn read(&self, _name: &str) -> Result<Vec<u8>, Error> {
        Ok(Vec::new())
    }
    fn look(&self, name: &str) -> Result<Look, Error> {
        if name == "custom.cube" {
            Ok(self.custom_look.clone())
        } else {
            Err(Error::Refused(format!("LUT '{name}' not found")))
        }
    }
    fn create_new(&mut self, _output: &str, _bytes: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn carry_metadata(
        &mut self,
        _source: &str,
        _output: &str,
        _width: u32,
        _height: u32,
        _format: OutputFormat,
    ) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn resolve_look_finds_builtin_and_library_and_refuses_an_unknown_name() {
    let custom_look = rollchrome_core::media::looks::builtin::builtin_look("ricoh-standard")
        .expect("builtin look");
    let store = TestResolveStore { custom_look };

    // 1. Builtin look resolves directly
    let builtin =
        rollchrome_core::tools::develop::resolve_look("builtin:fujifilm-classic-chrome", &store);
    assert!(builtin.is_ok(), "failed to resolve builtin look");

    // 2. Library look resolves through store
    let library = rollchrome_core::tools::develop::resolve_look("custom.cube", &store);
    assert!(library.is_ok(), "failed to resolve library look");

    // 3. Unknown builtin refused
    let unknown_builtin =
        rollchrome_core::tools::develop::resolve_look("builtin:nonexistent-look", &store);
    assert!(unknown_builtin.is_err());

    // 4. Unknown library look refused
    let unknown_library = rollchrome_core::tools::develop::resolve_look("unknown.cube", &store);
    assert!(unknown_library.is_err());
}
