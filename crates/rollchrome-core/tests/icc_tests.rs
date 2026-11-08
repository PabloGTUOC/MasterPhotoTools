//! Acceptance tests for ICC profile handling and sRGB conversion (RP-2).

#[path = "../../core/tests/fixtures/mod.rs"]
mod fixtures;

use fixtures::Fixtures;
use phototools_core::error::Error;
use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::meta::{exiftool_program, ExifWriter};
use phototools_core::tools::{carry_metadata_correcting, Derived, Tool};
use rollchrome_core::media::develop::colour::{mul_3x3_vec, srgb_to_prophoto_matrix};
use rollchrome_core::media::develop::render::{prophoto_icc_profile, render, OutputFormat};
use rollchrome_core::media::develop::tone::prophoto_encode;
use rollchrome_core::media::develop::{Demosaic, Developed};
use rollchrome_core::media::edit::icc::srgb_metadata_corrections;
use rollchrome_core::media::edit::pipeline::{
    decode_image, srgb_to_linear, AdjustmentRecipe, ImageBuffer,
};
use rollchrome_core::media::looks::builtin::builtin_look;
use rollchrome_core::tools::edit::export_edited_image;
use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};
use std::fs;
use std::process::Command;

/// Encodes an RGB8 image as a JPEG, optionally embedding an ICC profile.
fn encode_jpeg_with_icc(
    pixels: &[u8],
    width: u32,
    height: u32,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use image::ImageEncoder;
    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 100);
    if let Some(icc_data) = icc {
        encoder.set_icc_profile(icc_data.to_vec())?;
    }
    encoder.write_image(pixels, width, height, image::ExtendedColorType::Rgb8)?;
    Ok(buf)
}

/// Encodes an RGB8 or RGB16 image as a TIFF with an embedded ICC profile.
fn encode_tiff_with_icc_u8(
    pixels: &[u8],
    width: u32,
    height: u32,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut encoder = tiff::encoder::TiffEncoder::new(&mut cursor)?;
    let mut image_encoder = encoder.new_image::<tiff::encoder::colortype::RGB8>(width, height)?;
    if let Some(icc_bytes) = icc {
        image_encoder
            .encoder()
            .write_tag(tiff::tags::Tag::IccProfile, icc_bytes)?;
    }
    image_encoder.write_data(pixels)?;
    Ok(cursor.into_inner())
}

fn encode_tiff_with_icc_u16(
    pixels: &[u16],
    width: u32,
    height: u32,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut encoder = tiff::encoder::TiffEncoder::new(&mut cursor)?;
    let mut image_encoder = encoder.new_image::<tiff::encoder::colortype::RGB16>(width, height)?;
    if let Some(icc_bytes) = icc {
        image_encoder
            .encoder()
            .write_tag(tiff::tags::Tag::IccProfile, icc_bytes)?;
    }
    image_encoder.write_data(pixels)?;
    Ok(cursor.into_inner())
}

/// 1. A developed TIFF opened for editing matches the developed JPEG within two levels.
///
/// One `Developed` of uniform colour patches including saturated ones inside sRGB,
/// rendered with `render` to TIFF and to JPEG, with no look and with `builtin:fujifilm-provia`.
/// The TIFF through `decode_image`, reduced to 8 bits, against the JPEG decoded.
/// This test is the claim of this step.
#[test]
fn a_developed_tiff_opened_for_editing_matches_the_developed_jpeg_within_two_levels() {
    let f = Fixtures::new();
    let patch_size = 16u32;
    let w = patch_size * 3;
    let h = patch_size * 2;

    let m_srgb_to_pro = srgb_to_prophoto_matrix();

    // 6 uniform colour patches inside sRGB, including neutral, pastel, and saturated colours:
    let srgb_patches = [
        [128u8, 128, 128], // Neutral mid-grey
        [200, 150, 120],   // Skin tone
        [100, 160, 230],   // Sky blue
        [220, 40, 40],     // Saturated red
        [40, 200, 50],     // Saturated green
        [40, 50, 220],     // Saturated blue
    ];

    let mut pro_u16_patches = [[0u16; 3]; 6];
    for (i, srgb) in srgb_patches.iter().enumerate() {
        let s_lin = [
            srgb_to_linear(srgb[0] as f32 / 255.0),
            srgb_to_linear(srgb[1] as f32 / 255.0),
            srgb_to_linear(srgb[2] as f32 / 255.0),
        ];
        let pro_lin = mul_3x3_vec(m_srgb_to_pro, s_lin);
        pro_u16_patches[i] = [
            (prophoto_encode(pro_lin[0]) * 65535.0)
                .round()
                .clamp(0.0, 65535.0) as u16,
            (prophoto_encode(pro_lin[1]) * 65535.0)
                .round()
                .clamp(0.0, 65535.0) as u16,
            (prophoto_encode(pro_lin[2]) * 65535.0)
                .round()
                .clamp(0.0, 65535.0) as u16,
        ];
    }

    let mut raw = vec![0u16; (w * h * 3) as usize];
    for y in 0..h {
        let row_patch = (y / patch_size) * 3;
        for x in 0..w {
            let col_patch = x / patch_size;
            let patch_idx = (row_patch + col_patch) as usize;
            let offset = ((y * w + x) * 3) as usize;
            raw[offset] = pro_u16_patches[patch_idx][0];
            raw[offset + 1] = pro_u16_patches[patch_idx][1];
            raw[offset + 2] = pro_u16_patches[patch_idx][2];
        }
    }

    let dev_img = image::ImageBuffer::from_raw(w, h, raw).expect("valid buffer");
    let developed = Developed {
        image: dev_img,
        orientation: 1,
        demosaic: Demosaic::Bayer,
    };

    let provia = builtin_look("fujifilm-provia").expect("provia look");

    for (test_name, look) in [("no look", None), ("fujifilm-provia", Some(&provia))] {
        let tiff_out = render(&developed, look, OutputFormat::Tiff).expect("render tiff");
        let jpeg_out =
            render(&developed, look, OutputFormat::Jpeg { quality: 100 }).expect("render jpeg");

        let tiff_path = f
            .path()
            .join(format!("test_{}.tif", test_name.replace(' ', "_")));
        fs::write(&tiff_path, &tiff_out.bytes).expect("write tiff");

        // The TIFF through decode_image (which performs the ProPhoto -> sRGB conversion)
        let decoded_tiff = decode_image(&tiff_path).expect("decode tiff");
        let ImageBuffer::Rgb16 {
            width: tw,
            height: th,
            data: tdata,
        } = decoded_tiff
        else {
            panic!("expected ImageBuffer::Rgb16 from 16-bit TIFF");
        };
        assert_eq!(tw, w);
        assert_eq!(th, h);

        // Reduce TIFF from 16-bit to 8-bit
        let tiff_u8: Vec<u8> = tdata
            .iter()
            .map(|&v| ((v as u32 * 255 + 32767) / 65535) as u8)
            .collect();

        // The JPEG decoded directly
        let decoded_jpeg = image::load_from_memory(&jpeg_out.bytes)
            .expect("load jpeg")
            .to_rgb8();
        let jpeg_u8 = decoded_jpeg.into_raw();

        assert_eq!(tiff_u8.len(), jpeg_u8.len());
        for i in 0..tiff_u8.len() {
            let diff = (tiff_u8[i] as i16 - jpeg_u8[i] as i16).abs();
            assert!(
                diff <= 2,
                "Under {test_name}: byte {i} differs by {diff} > 2 levels (TIFF: {}, JPEG: {})",
                tiff_u8[i],
                jpeg_u8[i]
            );
        }
    }
}

/// 2. An sRGB-tagged JPEG decodes to the same pixels as an untagged one.
#[test]
fn an_srgb_tagged_jpeg_decodes_to_the_same_pixels_as_an_untagged_one() {
    let f = Fixtures::new();
    let w = 8u32;
    let h = 8u32;
    let mut pixels = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            pixels.push(((x * 31) % 256) as u8);
            pixels.push(((y * 37) % 256) as u8);
            pixels.push((((x + y) * 23) % 256) as u8);
        }
    }

    let untagged_bytes = encode_jpeg_with_icc(&pixels, w, h, None).expect("encode untagged");
    let srgb_icc = moxcms::ColorProfile::new_srgb()
        .encode()
        .expect("encode srgb icc");
    let tagged_bytes = encode_jpeg_with_icc(&pixels, w, h, Some(&srgb_icc)).expect("encode tagged");

    let p_untagged = f.path().join("untagged.jpg");
    let p_tagged = f.path().join("tagged.jpg");
    fs::write(&p_untagged, untagged_bytes).expect("write untagged");
    fs::write(&p_tagged, tagged_bytes).expect("write tagged");

    let dec_untagged = decode_image(&p_untagged).expect("decode untagged");
    let dec_tagged = decode_image(&p_tagged).expect("decode tagged");

    let ImageBuffer::Rgb8 { data: d1, .. } = dec_untagged else {
        panic!("expected Rgb8");
    };
    let ImageBuffer::Rgb8 { data: d2, .. } = dec_tagged else {
        panic!("expected Rgb8");
    };

    assert_eq!(
        d1, d2,
        "sRGB-tagged JPEG must decode to exact same pixels as untagged JPEG (no re-quantisation)"
    );
}

/// 3. An Adobe RGB file is converted to the expected sRGB values.
///
/// Expected values computed in the test from the published matrices, not from `moxcms`.
#[test]
fn an_adobe_rgb_file_is_converted_to_the_expected_srgb_values() {
    let f = Fixtures::new();

    // Published formulas for Adobe RGB (1998) to sRGB conversion (both D65 white point):
    fn published_adobe_rgb_to_srgb(r: u8, g: u8, b: u8) -> [u8; 3] {
        let gamma = 563.0f32 / 256.0f32; // Adobe RGB published gamma (563/256)
        let r_lin = (r as f32 / 255.0).powf(gamma);
        let g_lin = (g as f32 / 255.0).powf(gamma);
        let b_lin = (b as f32 / 255.0).powf(gamma);

        // Published linear Adobe RGB -> linear sRGB 3x3 matrix:
        let r_srgb_lin = 1.398356 * r_lin - 0.398356 * g_lin;
        let g_srgb_lin = g_lin;
        let b_srgb_lin = -0.042929 * g_lin + 1.042929 * b_lin;

        fn to_srgb_ch(c: f32) -> u8 {
            let c = c.clamp(0.0, 1.0);
            let s = if c <= 0.0031308 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (s * 255.0).round().clamp(0.0, 255.0) as u8
        }

        [
            to_srgb_ch(r_srgb_lin),
            to_srgb_ch(g_srgb_lin),
            to_srgb_ch(b_srgb_lin),
        ]
    }

    let test_colours: [[u8; 3]; 6] = [
        [128, 128, 128], // Neutral grey
        [180, 120, 80],  // Warm tone
        [60, 140, 200],  // Blue-cyan tone
        [200, 60, 60],   // Red
        [60, 180, 60],   // Green
        [60, 60, 180],   // Blue
    ];

    let w = test_colours.len() as u32;
    let h = 1u32;
    let mut raw_pixels = Vec::with_capacity((w * 3) as usize);
    for col in &test_colours {
        raw_pixels.extend_from_slice(col);
    }

    let adobe_icc = moxcms::ColorProfile::new_adobe_rgb()
        .encode()
        .expect("encode adobe icc");
    let tiff_bytes =
        encode_tiff_with_icc_u8(&raw_pixels, w, h, Some(&adobe_icc)).expect("encode tiff");

    let p = f.path().join("adobe_rgb.tif");
    fs::write(&p, tiff_bytes).expect("write file");

    let decoded = decode_image(&p).expect("decode image");
    let ImageBuffer::Rgb8 { data, .. } = decoded else {
        panic!("expected Rgb8");
    };

    for (i, col) in test_colours.iter().enumerate() {
        let expected = published_adobe_rgb_to_srgb(col[0], col[1], col[2]);
        let actual = [data[i * 3], data[i * 3 + 1], data[i * 3 + 2]];

        for ch in 0..3 {
            let diff = (actual[ch] as i16 - expected[ch] as i16).abs();
            assert!(
                diff <= 1,
                "Colour {i} channel {ch}: actual {} differs from published expected {} by {diff} > 1 (input Adobe RGB: {:?})",
                actual[ch],
                expected[ch],
                col
            );
        }
    }
}

/// 4. A conversion keeps the bit depth of the source: an 8-bit JPEG and a 16-bit TIFF.
#[test]
fn a_conversion_keeps_the_bit_depth_of_the_source() {
    let f = Fixtures::new();
    let adobe_icc = moxcms::ColorProfile::new_adobe_rgb()
        .encode()
        .expect("encode adobe icc");

    // 8-bit JPEG with Adobe RGB
    let pixels_u8 = vec![128u8; 4 * 4 * 3];
    let jpeg_bytes = encode_jpeg_with_icc(&pixels_u8, 4, 4, Some(&adobe_icc)).expect("encode jpeg");
    let p_jpeg = f.path().join("eight_bit.jpg");
    fs::write(&p_jpeg, jpeg_bytes).expect("write jpeg");

    let dec_8 = decode_image(&p_jpeg).expect("decode 8-bit");
    assert!(
        matches!(dec_8, ImageBuffer::Rgb8 { .. }),
        "8-bit JPEG must decode as ImageBuffer::Rgb8"
    );

    // 16-bit TIFF with ProPhoto
    let prophoto_icc = prophoto_icc_profile().expect("prophoto icc");
    let pixels_u16 = vec![32768u16; 4 * 4 * 3];
    let tiff_bytes =
        encode_tiff_with_icc_u16(&pixels_u16, 4, 4, Some(&prophoto_icc)).expect("encode tiff");
    let p_tiff = f.path().join("sixteen_bit.tif");
    fs::write(&p_tiff, &tiff_bytes).expect("write tiff");

    let dec_16 = decode_image(&p_tiff).expect("decode 16-bit");
    assert!(
        matches!(dec_16, ImageBuffer::Rgb16 { .. }),
        "16-bit TIFF must decode as ImageBuffer::Rgb16"
    );

    // A 16-bit TIFF named with a .jpg extension must decode via content guessing
    let p_misnamed = f.path().join("misnamed_tiff.jpg");
    fs::write(&p_misnamed, tiff_bytes).expect("write misnamed tiff");
    let dec_misnamed = decode_image(&p_misnamed).expect("decode misnamed TIFF");
    assert!(
        matches!(dec_misnamed, ImageBuffer::Rgb16 { .. }),
        "16-bit TIFF named .jpg must decode as ImageBuffer::Rgb16 via guessed format"
    );
}

/// 5. A file with an unreadable profile is refused and says why.
#[test]
fn a_file_with_an_unreadable_profile_is_refused_and_says_why() {
    let f = Fixtures::new();
    let corrupt_icc = vec![0xba, 0xad, 0xf0, 0x0d, 0x01, 0x02, 0x03, 0x04];
    let pixels = vec![128u8; 4 * 4 * 3];
    let tiff_bytes =
        encode_tiff_with_icc_u8(&pixels, 4, 4, Some(&corrupt_icc)).expect("encode tiff");

    let p = f.path().join("corrupt_icc.tif");
    fs::write(&p, tiff_bytes).expect("write file");

    let err = decode_image(&p).expect_err("corrupt icc must be refused");
    let msg = match err {
        Error::Refused(m) => m,
        other => panic!("expected Error::Refused, got {other:?}"),
    };

    assert!(
        msg.contains("colour profile could not be read") && msg.contains("colours would be wrong"),
        "Error message must explain profile could not be read and colours would be wrong, got: '{msg}'"
    );
}

/// 6. A greyscale file with a grey profile still opens.
#[test]
fn a_greyscale_file_with_a_grey_profile_still_opens() {
    let f = Fixtures::new();
    let gray_icc = moxcms::ColorProfile::new_gray_with_gamma(2.2)
        .encode()
        .expect("encode gray icc");

    // Write a greyscale TIFF with an embedded Gray ICC profile
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut encoder = tiff::encoder::TiffEncoder::new(&mut cursor).expect("create encoder");
    let mut image_encoder = encoder
        .new_image::<tiff::encoder::colortype::Gray8>(4, 4)
        .expect("new image");
    image_encoder
        .encoder()
        .write_tag(tiff::tags::Tag::IccProfile, gray_icc.as_slice())
        .expect("write icc tag");
    let gray_pixels = vec![128u8; 4 * 4];
    image_encoder
        .write_data(&gray_pixels)
        .expect("write gray data");
    let tiff_bytes = cursor.into_inner();

    let p = f.path().join("gray.tif");
    fs::write(&p, tiff_bytes).expect("write file");

    let decoded = decode_image(&p).expect("greyscale file with grey profile must open");
    assert!(
        decoded.width() == 4 && decoded.height() == 4,
        "greyscale image must be decoded successfully"
    );
}

/// 7. An edit export of a ProPhoto TIFF says sRGB and carries no ProPhoto profile.
///
/// Real exiftool; read the output's tags back.
#[test]
fn an_edit_export_of_a_prophoto_tiff_says_srgb_and_carries_no_prophoto_profile() {
    let f = Fixtures::new();

    // 1. Create a 16-bit ProPhoto TIFF with embedded ProPhoto profile and wide-gamut tags
    let prophoto_icc = prophoto_icc_profile().expect("prophoto icc");
    let pixels = vec![32768u16; 8 * 8 * 3];
    let tiff_bytes =
        encode_tiff_with_icc_u16(&pixels, 8, 8, Some(&prophoto_icc)).expect("encode tiff");

    let src_path = f.path().join("prophoto_source.tif");
    fs::write(&src_path, tiff_bytes).expect("write source tiff");

    // Tag the source with ColorSpace=65535 and InteropIndex=R98
    let mut writer = ExifWriter::start().expect("start exiftool");
    writer
        .set_tags(
            &src_path,
            &[
                "-ExifIFD:ColorSpace#=65535".to_string(),
                "-InteropIFD:InteropIndex=R98".to_string(),
            ],
        )
        .expect("tag source");
    writer.close().expect("close writer");

    // 2. Export through Edit with non-identity recipe (triggers render_and_write + metadata carrying)
    let out_dir = f.path().join("exports");
    let recipe = AdjustmentRecipe {
        contrast: 0.05,
        ..AdjustmentRecipe::default()
    };
    let result = export_edited_image(&src_path, &recipe, None, &out_dir).expect("export edited");
    assert!(result.path.exists());

    // 3. Read output tags back with real exiftool
    let exiftool = exiftool_program().expect("exiftool installed");
    let output = Command::new(&exiftool)
        .args([
            "-s",
            "-s",
            "-s",
            "-ColorSpace#",
            "-InteropIndex",
            "-ProfileDescription",
        ])
        .arg(&result.path)
        .output()
        .expect("run exiftool");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // ColorSpace# must be 1 (sRGB)
    assert!(
        lines.contains(&"1"),
        "Output ColorSpace must be 1 (sRGB), got exiftool output:\n{stdout}"
    );

    // InteropIndex must not be present
    assert!(
        !lines.contains(&"R98"),
        "Output InteropIndex must be absent, got:\n{stdout}"
    );

    // ProfileDescription must not be ProPhoto (it was stripped via -ICC_Profile=)
    for line in &lines {
        assert!(
            !line.contains("ProPhoto"),
            "Output must carry no ProPhoto profile, got: {line}"
        );
    }
}

/// 8. A batch output of a ProPhoto TIFF says sRGB and carries no ProPhoto profile.
///
/// The same through the bulk LUT tool.
#[test]
fn a_batch_output_of_a_prophoto_tiff_says_srgb_and_carries_no_prophoto_profile() {
    let f = Fixtures::new();

    // 1. Create source ProPhoto TIFF
    let prophoto_icc = prophoto_icc_profile().expect("prophoto icc");
    let pixels = vec![32768u16; 8 * 8 * 3];
    let tiff_bytes =
        encode_tiff_with_icc_u16(&pixels, 8, 8, Some(&prophoto_icc)).expect("encode tiff");

    let src_path = f.path().join("batch_source.tif");
    fs::write(&src_path, tiff_bytes).expect("write source tiff");

    let mut writer = ExifWriter::start().expect("start exiftool");
    writer
        .set_tags(
            &src_path,
            &[
                "-ExifIFD:ColorSpace#=65535".to_string(),
                "-InteropIFD:InteropIndex=R98".to_string(),
            ],
        )
        .expect("tag source");
    writer.close().expect("close writer");

    // 2. Create minimal 2x2x2 identity LUT
    let lut_path = f.path().join("identity.cube");
    fs::write(
        &lut_path,
        "TITLE \"Identity\"\nLUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n",
    )
    .expect("write lut");

    // 3. Run BulkLutTool
    let out_dir = f.path().join("bulk_out");
    let params = BulkLutParams::new(vec![src_path], lut_path, 1.0, out_dir);
    let plan = BulkLutTool.plan(&params).expect("plan bulk lut").data;
    let summary = BulkLutTool
        .apply(plan, &InMemoryProgress::default())
        .expect("apply bulk lut")
        .data;
    assert_eq!(summary.written.len(), 1);
    let output_path = &summary.written[0];

    // 4. Read output tags back with real exiftool
    let exiftool = exiftool_program().expect("exiftool installed");
    let output = Command::new(&exiftool)
        .args([
            "-s",
            "-s",
            "-s",
            "-ColorSpace#",
            "-InteropIndex",
            "-ProfileDescription",
        ])
        .arg(output_path)
        .output()
        .expect("run exiftool");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    assert!(
        lines.contains(&"1"),
        "Bulk LUT output ColorSpace must be 1 (sRGB), got exiftool output:\n{stdout}"
    );
    assert!(
        !lines.contains(&"R98"),
        "Bulk LUT output InteropIndex must be absent, got:\n{stdout}"
    );
    for line in &lines {
        assert!(
            !line.contains("ProPhoto"),
            "Bulk LUT output must carry no ProPhoto profile, got: {line}"
        );
    }
}

/// 9. Correcting metadata for a batch uses one exiftool process.
///
/// Three outputs, a shim counting starts, as `develop_folder_tests.rs::one_exiftool_process_carries_the_whole_batch` does.
#[test]
fn correcting_metadata_for_a_batch_uses_one_exiftool_process() {
    let f = Fixtures::new();

    // Create 3 sources and 3 outputs
    let mut derived = Vec::new();
    for i in 1..=3 {
        let src = f.jpeg_without_exif(&format!("src_{i}.jpg"), 16, 16);
        let dst = f.jpeg_without_exif(&format!("dst_{i}.jpg"), 16, 16);
        derived.push(Derived {
            source: src,
            output: dst,
            width: 16,
            height: 16,
        });
    }

    let real_exiftool = exiftool_program().expect("exiftool installed");
    let shim = f.path().join("exiftool-shim");
    let log = f.path().join("spawns.log");

    fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho spawn >> \"{}\"\nexec \"{}\" \"$@\"\n",
            log.display(),
            real_exiftool
        ),
    )
    .expect("write shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("chmod +x shim");
    }

    let corrections = srgb_metadata_corrections();
    let shim_str = shim.to_string_lossy().to_string();
    let skipped = carry_metadata_correcting(&derived, false, Some(&shim_str), &corrections);
    assert!(
        skipped.is_empty(),
        "carry_metadata_correcting reported skips: {skipped:?}"
    );

    let spawns = fs::read_to_string(&log).expect("read log").lines().count();
    assert_eq!(
        spawns, 1,
        "carry_metadata_correcting across 3 outputs must start exactly 1 exiftool process, got {spawns}"
    );
}
