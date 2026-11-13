//! Integration tests for simulated film stocks (DV-4).

#[allow(dead_code)]
mod support;

use rollchrome_core::automation::{Automation, DevelopRequest};
use rollchrome_core::media::develop::preview::PreviewLook;
use rollchrome_core::media::develop::{
    linear_to_developed, prophoto_icc_profile, prophoto_to_srgb8, Demosaic, DevelopPreview,
    OutputFormat, PREVIEW_MAX_EDGE,
};
use rollchrome_core::media::film::{
    ensure_film_data, render_film, FilmBackend, FilmOutput, Grain, FILM_STOCKS,
};
use rollchrome_core::tools::develop::{execute_develop, plan_develop, DevelopItem};
use rollchrome_core::tools::develop_folder::FolderStore;
use std::fs;
use std::path::PathBuf;
use support::dng::DngBuilder;

fn test_data_dir() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_dir = ensure_film_data(temp.path()).expect("ensure_film_data");
    (temp, data_dir)
}

fn synthetic_gradient(width: usize, height: usize) -> Vec<[f32; 3]> {
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let ny = y as f32 / (height - 1) as f32;
        for x in 0..width {
            let nx = x as f32 / (width - 1) as f32;
            let r = nx * 0.7 + 0.1;
            let g = ny * 0.7 + 0.1;
            let b = ((nx + ny) * 0.5) * 0.7 + 0.1;
            pixels.push([r, g, b]);
        }
    }
    pixels
}

// ---------------------------------------------------------------------------
// 1. Every stock renders a gradient that differs from the neutral development
// ---------------------------------------------------------------------------

#[test]
fn every_stock_renders_a_gradient_that_differs_from_the_neutral_development() {
    let (_temp, data_dir) = test_data_dir();
    let width = 64;
    let height = 48;
    let linear = synthetic_gradient(width, height);

    // Compute neutral development in sRGB [0.0, 1.0]
    let dev = linear_to_developed(width as u32, height as u32, &linear, 1, Demosaic::Bayer)
        .expect("linear_to_developed");
    let neutral_img = prophoto_to_srgb8(&dev);
    let neutral_floats: Vec<[f32; 3]> = neutral_img
        .pixels()
        .map(|p| {
            [
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
            ]
        })
        .collect();

    assert_eq!(FILM_STOCKS.len(), 18);

    for stock in FILM_STOCKS {
        let rendered = render_film(
            &linear,
            width as u32,
            height as u32,
            stock.id,
            Grain::Off,
            0.0,
            FilmOutput::Srgb,
            FilmBackend::Cpu,
            &data_dir,
        )
        .unwrap_or_else(|e| panic!("failed to render stock {}: {e}", stock.id));

        assert_eq!(
            rendered.len(),
            width * height,
            "stock {} rendered wrong size",
            stock.id
        );

        for (i, p) in rendered.iter().enumerate() {
            assert!(
                p[0].is_finite() && p[1].is_finite() && p[2].is_finite(),
                "stock {} pixel {} had non-finite value: {:?}",
                stock.id,
                i,
                p
            );
        }

        // Compare with neutral development: must differ
        let mut diff_sum = 0.0f32;
        for (r, n) in rendered.iter().zip(neutral_floats.iter()) {
            diff_sum += (r[0] - n[0]).abs() + (r[1] - n[1]).abs() + (r[2] - n[2]).abs();
        }
        let mad = diff_sum / (width * height * 3) as f32;
        assert!(
            mad > 1.0 / 255.0,
            "stock {} did not differ enough from neutral development (MAD: {mad})",
            stock.id
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Two stocks render the same input differently (Portra 160 vs Ektar 100)
// ---------------------------------------------------------------------------

#[test]
fn two_stocks_render_the_same_input_differently() {
    let (_temp, data_dir) = test_data_dir();
    let width = 64;
    let height = 48;
    let linear = synthetic_gradient(width, height);

    let portra = render_film(
        &linear,
        width as u32,
        height as u32,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("render portra");

    let ektar = render_film(
        &linear,
        width as u32,
        height as u32,
        "kodak_ektar_100",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("render ektar");

    let diff_sum: f32 = portra
        .iter()
        .zip(ektar.iter())
        .map(|(p, e)| (p[0] - e[0]).abs() + (p[1] - e[1]).abs() + (p[2] - e[2]).abs())
        .sum();
    let mad = diff_sum / (width * height * 3) as f32;

    assert!(
        mad > 1.0 / 255.0,
        "Portra 160 vs Ektar 100 MAD was {mad}, expected > 1/255"
    );
}

// ---------------------------------------------------------------------------
// 3. The CPU render is identical run to run
// ---------------------------------------------------------------------------

#[test]
fn the_cpu_render_is_identical_run_to_run() {
    let (_temp, data_dir) = test_data_dir();
    let width = 32;
    let height = 32;
    let linear = synthetic_gradient(width, height);

    let run1 = render_film(
        &linear,
        width as u32,
        height as u32,
        "kodak_portra_400",
        Grain::Fine,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("run 1");

    let run2 = render_film(
        &linear,
        width as u32,
        height as u32,
        "kodak_portra_400",
        Grain::Fine,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("run 2");

    assert_eq!(
        run1, run2,
        "CPU render must be bit-identical run to run in f64 precision"
    );
}

// ---------------------------------------------------------------------------
// 4. Exposure compensation brightens
// ---------------------------------------------------------------------------

#[test]
fn exposure_compensation_brightens() {
    let (_temp, data_dir) = test_data_dir();
    let width = 64;
    let height = 48;
    let linear = vec![[0.18f32, 0.18, 0.18]; width * height];

    let r_base = render_film(
        &linear,
        width as u32,
        height as u32,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("render 0 EV");

    let r_plus = render_film(
        &linear,
        width as u32,
        height as u32,
        "kodak_portra_160",
        Grain::Off,
        1.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("render +1 EV");

    let mean_base: f32 = r_base
        .iter()
        .map(|p| (p[0] + p[1] + p[2]) / 3.0)
        .sum::<f32>()
        / (width * height) as f32;
    let mean_plus: f32 = r_plus
        .iter()
        .map(|p| (p[0] + p[1] + p[2]) / 3.0)
        .sum::<f32>()
        / (width * height) as f32;

    assert!(
        mean_plus > mean_base,
        "expected +1 EV mean ({mean_plus}) > 0 EV mean ({mean_base})"
    );
}

// ---------------------------------------------------------------------------
// 5. Grain off, fine, and standard order the noise
// ---------------------------------------------------------------------------

#[test]
fn grain_off_fine_and_standard_order_the_noise() {
    let (_temp, data_dir) = test_data_dir();
    let size = 256;
    let linear = vec![[0.18f32, 0.18, 0.18]; size * size];

    let calc_std_dev = |grain: Grain| -> f64 {
        let out = render_film(
            &linear,
            size as u32,
            size as u32,
            "kodak_portra_400",
            grain,
            0.0,
            FilmOutput::Srgb,
            FilmBackend::Cpu,
            &data_dir,
        )
        .expect("render film");

        let n = (size * size * 3) as f64;
        let sum: f64 = out
            .iter()
            .map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64)
            .sum();
        let mean = sum / n;
        let var: f64 = out
            .iter()
            .map(|p| {
                (p[0] as f64 - mean).powi(2)
                    + (p[1] as f64 - mean).powi(2)
                    + (p[2] as f64 - mean).powi(2)
            })
            .sum::<f64>()
            / n;
        var.sqrt()
    };

    let sd_off = calc_std_dev(Grain::Off);
    let sd_fine = calc_std_dev(Grain::Fine);
    let sd_standard = calc_std_dev(Grain::Standard);

    println!(
        "Grain standard deviations on flat 256x256 mid-grey: Off = {:.6}, Fine = {:.6}, Standard = {:.6}",
        sd_off, sd_fine, sd_standard
    );

    assert!(
        sd_off < 0.5 / 255.0,
        "Off std dev ({sd_off}) must be below 0.5/255 ({})",
        0.5 / 255.0
    );
    assert!(
        sd_off < sd_fine,
        "expected Off ({sd_off}) < Fine ({sd_fine})"
    );
    assert!(
        sd_fine < sd_standard,
        "expected Fine ({sd_fine}) < Standard ({sd_standard})"
    );
}

// ---------------------------------------------------------------------------
// 6. Slides are scanned without paper
// ---------------------------------------------------------------------------

#[test]
fn slides_are_scanned_without_paper() {
    let (_temp, data_dir) = test_data_dir();
    let width = 32;
    let height = 32;
    let linear = synthetic_gradient(width, height);

    let slide_stocks = [
        "kodak_ektachrome_100",
        "kodak_kodachrome_64",
        "fujifilm_provia_100f",
        "fujifilm_velvia_100",
    ];

    for stock in slide_stocks {
        let rendered = render_film(
            &linear,
            width as u32,
            height as u32,
            stock,
            Grain::Off,
            0.0,
            FilmOutput::Srgb,
            FilmBackend::Cpu,
            &data_dir,
        )
        .unwrap_or_else(|e| panic!("slide stock {stock} failed to render: {e}"));

        assert_eq!(rendered.len(), width * height);
        for p in &rendered {
            assert!(p[0].is_finite() && p[1].is_finite() && p[2].is_finite());
        }

        // The claim, not a proxy: the render is the engine's own scan of the film, and is not
        // what printing the film on paper would give. Both references are built here directly
        // from spektrafilm, with the settings the brief fixes, on the deterministic CPU backend.
        let scanned = engine_reference(&data_dir, stock, true, &linear, width, height);
        let printed = engine_reference(&data_dir, stock, false, &linear, width, height);
        assert_eq!(
            rendered, scanned,
            "{stock} must be the engine's direct scan of the slide"
        );
        assert_ne!(
            rendered, printed,
            "{stock} must not be printed: the two paths must differ for this test to mean anything"
        );
    }
}

/// spektrafilm run directly, without Rollchrome's film module: grain off, 0 EV, sRGB out, CPU.
/// A slide is its own print, as in spektrafilm's command-line `--scan-film`.
fn engine_reference(
    data_dir: &std::path::Path,
    stock: &str,
    scan_film: bool,
    linear: &[[f32; 3]],
    width: usize,
    height: usize,
) -> Vec<[f32; 3]> {
    let film =
        spektrafilm_core::profile::load_profile_by_name(data_dir, stock).expect("film profile");
    let print =
        spektrafilm_core::profile::load_profile_by_name(data_dir, stock).expect("print profile");
    let mut params = spektrafilm_core::params::RuntimeParams::default();
    params.io.scan_film = scan_film;
    params.io.input_color_space = "ProPhoto RGB".to_string();
    params.io.input_cctf_decoding = false;
    params.io.output_color_space = "sRGB".to_string();
    params.io.output_cctf_encoding = true;
    params.film_render.grain.active = false;
    let pipeline =
        spektrafilm_core::pipeline::Pipeline::new_with_spectral(film, print, params, data_dir)
            .expect("reference pipeline");
    let input: Vec<f64> = linear
        .iter()
        .flat_map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();
    let image = spektrafilm_math::image::ImageBuf::from_data(width as u32, height as u32, input);
    let out = pipeline.process(image, &spektrafilm_gpu::cpu_backend::CpuBackend);
    out.data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32])
        .collect()
}

// ---------------------------------------------------------------------------
// 7. An unknown stock is refused by name
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_stock_is_refused_by_name() {
    let (_temp, data_dir) = test_data_dir();
    let linear = vec![[0.18f32, 0.18, 0.18]; 16];

    let err = render_film(
        &linear,
        4,
        4,
        "nonexistent_stock_name",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent_stock_name"),
        "error message must name the unknown stock, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 8. The data directory is written once and repaired
// ---------------------------------------------------------------------------

#[test]
fn the_data_directory_is_written_once_and_repaired() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    // 1. First use fills an empty directory
    let data_dir1 = ensure_film_data(root).expect("first ensure");
    assert!(data_dir1.exists());
    assert!(data_dir1.join("profiles").exists());
    assert!(data_dir1.join("filters").exists());
    assert!(data_dir1.join("luts").exists());

    // Record mtimes of a profile and filter
    let test_file = data_dir1.join("filters/neutral_print_filters.json");
    assert!(test_file.exists());
    let original_meta = fs::metadata(&test_file).expect("original meta");
    let original_mtime = original_meta.modified().expect("mtime");
    let original_size = original_meta.len();
    assert!(original_size > 0);

    // 2. Second use rewrites nothing (modification times unchanged)
    let data_dir2 = ensure_film_data(root).expect("second ensure");
    assert_eq!(data_dir1, data_dir2);
    let second_meta = fs::metadata(&test_file).expect("second meta");
    assert_eq!(
        second_meta.modified().expect("second mtime"),
        original_mtime,
        "second use must not rewrite existing valid files"
    );

    // 3. Truncated file is rewritten and repaired
    fs::write(&test_file, b"").expect("truncate file");
    assert_eq!(fs::metadata(&test_file).unwrap().len(), 0);

    let data_dir3 = ensure_film_data(root).expect("repair ensure");
    assert_eq!(data_dir1, data_dir3);
    let repaired_meta = fs::metadata(&test_file).expect("repaired meta");
    assert_eq!(
        repaired_meta.len(),
        original_size,
        "repaired file must match original size"
    );
}

// ---------------------------------------------------------------------------
// 9. Film TIFF is 16-bit ProPhoto with profile, and Film JPEG is 8-bit sRGB
// ---------------------------------------------------------------------------

#[test]
fn a_film_tiff_is_16_bit_prophoto_with_its_profile() {
    let temp = tempfile::tempdir().unwrap();
    let raw_dir = temp.path().join("raws");
    let lut_dir = temp.path().join("luts");
    fs::create_dir_all(&raw_dir).unwrap();
    fs::create_dir_all(&lut_dir).unwrap();

    let (_dtemp, film_dir) = test_data_dir();

    let width = 64u32;
    let height = 48u32;
    let pixels = vec![3000u16; (width * height) as usize];
    let dng_bytes = DngBuilder::new_bayer(width, height).build(&pixels);
    fs::write(raw_dir.join("photo.dng"), dng_bytes).unwrap();

    let mut store = FolderStore::with_film_data(raw_dir.clone(), lut_dir, Some(film_dir.clone()));
    let store_film_dir = film_dir;

    let items = vec![DevelopItem {
        name: "photo.dng".to_string(),
        look: Some("film:kodak_portra_160".to_string()),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Tiff,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["photo.tif"]);
    assert!(summary.failures.is_empty());

    let output_path = store.output_dir().join("photo.tif");
    assert!(output_path.exists());

    let file_bytes = fs::read(&output_path).expect("read TIFF bytes");
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&file_bytes)).unwrap();

    let (w, h) = decoder.dimensions().unwrap();
    assert_eq!(w, width);
    assert_eq!(h, height);

    let color_type = decoder.colortype().unwrap();
    assert_eq!(
        color_type,
        tiff::ColorType::RGB(16),
        "TIFF must be 16-bit RGB"
    );

    let tag_val = decoder
        .find_tag(tiff::tags::Tag::IccProfile)
        .unwrap()
        .expect("IccProfile tag present");
    let icc_bytes: Vec<u8> = match tag_val {
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

    let expected_profile = prophoto_icc_profile().expect("expected profile");
    assert_eq!(icc_bytes.len(), expected_profile.len());
    let mut actual_normalized = icc_bytes.clone();
    actual_normalized[24..36].copy_from_slice(&expected_profile[24..36]);
    assert_eq!(
        actual_normalized, expected_profile,
        "TIFF ICC profile must match ProPhoto RGB profile"
    );

    // The pixels, not only the container: decoding the TIFF's ROMM values must give the engine's
    // linear ProPhoto output. The export may run on the GPU, so this compares with the CPU render
    // within a tolerance far below the gap a display-encoded (sRGB) output would leave.
    let decoded: Vec<f32> = match decoder.read_image().expect("decode TIFF pixels") {
        tiff::decoder::DecodingResult::U16(v) => v
            .into_iter()
            .map(|c| rollchrome_core::media::develop::tone::prophoto_decode(c as f32 / 65535.0))
            .collect(),
        _ => panic!("expected 16-bit samples"),
    };
    let raw = fs::read(raw_dir.join("photo.dng")).expect("read DNG");
    let linear = rollchrome_core::media::develop::develop_linear(
        &raw,
        &rollchrome_core::media::develop::DevelopOptions { exposure_ev: 0.0 },
    )
    .expect("develop linear");
    let reference = render_film(
        &linear.pixels,
        linear.width,
        linear.height,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::ProPhotoLinear,
        FilmBackend::Cpu,
        &store_film_dir,
    )
    .expect("reference render");
    let reference: Vec<f32> = reference.iter().flatten().map(|v| v.max(0.0)).collect();
    assert_eq!(decoded.len(), reference.len());
    let mean_abs = decoded
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / decoded.len() as f32;
    assert!(
        mean_abs < 0.01,
        "TIFF pixels must be the engine's linear ProPhoto output (mean difference {mean_abs})"
    );
}

#[test]
fn a_film_jpeg_is_8_bit_srgb() {
    let temp = tempfile::tempdir().unwrap();
    let raw_dir = temp.path().join("raws");
    let lut_dir = temp.path().join("luts");
    fs::create_dir_all(&raw_dir).unwrap();
    fs::create_dir_all(&lut_dir).unwrap();

    let (_dtemp, film_dir) = test_data_dir();

    let width = 64u32;
    let height = 48u32;
    let pixels = vec![3000u16; (width * height) as usize];
    let dng_bytes = DngBuilder::new_bayer(width, height).build(&pixels);
    fs::write(raw_dir.join("photo.dng"), dng_bytes).unwrap();

    let mut store = FolderStore::with_film_data(raw_dir.clone(), lut_dir, Some(film_dir));

    let items = vec![DevelopItem {
        name: "photo.dng".to_string(),
        look: Some("film:kodak_portra_160".to_string()),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Jpeg { quality: 90 },
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);

    assert_eq!(summary.developed, vec!["photo.jpg"]);
    assert!(summary.failures.is_empty());

    let output_path = store.output_dir().join("photo.jpg");
    assert!(output_path.exists());

    let jpeg_bytes = fs::read(&output_path).expect("read JPEG bytes");
    assert!(
        jpeg_bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "output must be valid JPEG"
    );

    let img = image::load_from_memory(&jpeg_bytes).expect("decode JPEG");
    assert_eq!(img.width(), width);
    assert_eq!(img.height(), height);
    assert_eq!(
        img.color(),
        image::ColorType::Rgb8,
        "JPEG must be 8-bit RGB"
    );
}

// ---------------------------------------------------------------------------
// 10. Grain is carried through the plan
// ---------------------------------------------------------------------------

#[test]
fn grain_is_carried_through_the_plan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(&root).unwrap();

    let dng = DngBuilder::new_bayer(16, 16).build(&vec![2000u16; 256]);
    fs::write(root.join("photo.dng"), dng).unwrap();

    let config = phototools_core::config::Config {
        roots: vec![root.canonicalize().unwrap()],
        staging_dir: temp.path().join("staging"),
        publishing_dir: None,
        thresholds: phototools_core::config::Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };

    let engine = Automation::new(config);

    // 1. Plan develop preserves Grain setting
    let items_std = vec![DevelopItem {
        name: "photo.dng".to_string(),
        look: Some("film:kodak_portra_400".to_string()),
        exposure_ev: 0.0,
        grain: Grain::Standard,
        noise_reduction: 0.0,
    }];
    let plan_std = plan_develop(
        &items_std,
        OutputFormat::Jpeg { quality: 90 },
        &std::collections::BTreeSet::new(),
    );
    assert_eq!(plan_std.actions.len(), 1);
    assert_eq!(plan_std.actions[0].grain, Grain::Standard);

    let items_off = vec![DevelopItem {
        name: "photo.dng".to_string(),
        look: Some("film:kodak_portra_400".to_string()),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 0.0,
    }];
    let plan_off = plan_develop(
        &items_off,
        OutputFormat::Jpeg { quality: 90 },
        &std::collections::BTreeSet::new(),
    );
    assert_eq!(plan_off.actions.len(), 1);
    assert_eq!(plan_off.actions[0].grain, Grain::Off);

    // 2. Automation plan hash changes when only the grain changes
    let req1 = DevelopRequest {
        folder: root.to_string_lossy().to_string(),
        look: "film:kodak_portra_400".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Fine,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 90 },
        only: vec!["photo.dng".to_string()],
        output: Default::default(),
    };
    let rev1 = engine.plan_develop(&req1).expect("plan 1");

    let req2 = DevelopRequest {
        folder: root.to_string_lossy().to_string(),
        look: "film:kodak_portra_400".to_string(),
        exposure_ev: 0.0,
        grain: Grain::Standard,
        noise_reduction: 0.0,
        format: OutputFormat::Jpeg { quality: 90 },
        only: vec!["photo.dng".to_string()],
        output: Default::default(),
    };
    let rev2 = engine.plan_develop(&req2).expect("plan 2");

    assert_ne!(
        rev1.plan_sha256, rev2.plan_sha256,
        "plan hash must change when only the grain changes"
    );
}

// ---------------------------------------------------------------------------
// 11. The preview renders a film look upright at preview size
// ---------------------------------------------------------------------------

#[test]
fn the_preview_renders_a_film_look_upright_at_preview_size() {
    let (_temp, data_dir) = test_data_dir();

    let width = 120u32;
    let height = 80u32;
    let pixels = vec![3000u16; (width * height) as usize];

    // Orientation 6: 90 deg clockwise rotation
    let mut builder = DngBuilder::new_bayer(width, height);
    builder.orientation = Some(6);
    let dng = builder.build(&pixels);

    let preview = DevelopPreview::new(&dng, PREVIEW_MAX_EDGE).expect("develop preview");
    let rendered = preview
        .render_with_data_dir(
            Some(PreviewLook::Film("kodak_portra_160")),
            0.0,
            Grain::Fine,
            0.0,
            false,
            Some(&data_dir),
        )
        .expect("preview render");

    assert!(
        rendered.bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "preview must be valid JPEG"
    );

    // Orientation 6: dimensions swap, so 120x80 becomes 80x120 upright
    assert_eq!(rendered.width, 80);
    assert_eq!(rendered.height, 120);
    assert!(rendered.width <= PREVIEW_MAX_EDGE);
    assert!(rendered.height <= PREVIEW_MAX_EDGE);
}

// ---------------------------------------------------------------------------
// 12. GPU meets the preview and export budgets (Mac release build, ignored)
// ---------------------------------------------------------------------------

#[ignore]
#[test]
fn the_gpu_meets_the_preview_and_export_budgets() {
    let (_temp, data_dir) = test_data_dir();

    // Warmup run on GPU
    let warmup_pixels = vec![[0.18f32, 0.18, 0.18]; 800 * 600];
    let _ = render_film(
        &warmup_pixels,
        800,
        600,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Auto,
        &data_dir,
    )
    .expect("GPU warmup");

    // 1600x1200 preview budget (warm <= 300 ms)
    let prev_w = 1600;
    let prev_h = 1200;
    let prev_pixels = synthetic_gradient(prev_w, prev_h);

    let start_prev = std::time::Instant::now();
    let gpu_prev = render_film(
        &prev_pixels,
        prev_w as u32,
        prev_h as u32,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Auto,
        &data_dir,
    )
    .expect("GPU preview render");
    let prev_time = start_prev.elapsed();

    println!("Warm 1600x1200 GPU preview time: {:.2?}", prev_time);
    assert!(
        prev_time <= std::time::Duration::from_millis(300),
        "warm 1600x1200 GPU preview time {:?} exceeded 300 ms budget",
        prev_time
    );

    // 7728x5152 (40 MP) export budget (warm <= 4 s)
    let exp_w = 7728;
    let exp_h = 5152;
    let exp_pixels = vec![[0.18f32, 0.18, 0.18]; exp_w * exp_h];

    let start_exp = std::time::Instant::now();
    let _gpu_exp = render_film(
        &exp_pixels,
        exp_w as u32,
        exp_h as u32,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::ProPhotoLinear,
        FilmBackend::Auto,
        &data_dir,
    )
    .expect("GPU 40MP export render");
    let exp_time = start_exp.elapsed();

    println!("Warm 7728x5152 (40 MP) GPU export time: {:.2?}", exp_time);
    assert!(
        exp_time <= std::time::Duration::from_secs(4),
        "warm 7728x5152 GPU export time {:?} exceeded 4 s budget",
        exp_time
    );

    // Grain off GPU vs CPU difference on 1600x1200 <= 3/255
    let cpu_prev = render_film(
        &prev_pixels,
        prev_w as u32,
        prev_h as u32,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Cpu,
        &data_dir,
    )
    .expect("CPU preview render");

    let diff_sum: f32 = gpu_prev
        .iter()
        .zip(cpu_prev.iter())
        .map(|(g, c)| (g[0] - c[0]).abs() + (g[1] - c[1]).abs() + (g[2] - c[2]).abs())
        .sum();
    let mad = diff_sum / (prev_w * prev_h * 3) as f32;

    println!(
        "Grain off GPU vs CPU mean absolute difference: {:.6} (budget: <= {:.6})",
        mad,
        3.0 / 255.0
    );
    assert!(
        mad <= 3.0 / 255.0,
        "GPU vs CPU mean absolute difference {mad} exceeded 3/255 ({})",
        3.0 / 255.0
    );
}

#[derive(Default)]
struct NoopProgress;

impl phototools_core::jobs::Progress for NoopProgress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// DV-5 Tests 9–10: Film Noise Reduction
// ---------------------------------------------------------------------------

struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16777216.0
    }
    fn next_gaussian(&mut self, sigma: f32) -> f32 {
        let u1 = self.next_f32().max(1e-7);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos() * sigma
    }
}

/// 9. `film_noise_reduction_happens_before_the_film`
#[test]
fn film_noise_reduction_happens_before_the_film() {
    let temp = tempfile::tempdir().unwrap();
    let raw_dir = temp.path().join("raws");
    let lut_dir = temp.path().join("luts");
    fs::create_dir_all(&raw_dir).unwrap();
    fs::create_dir_all(&lut_dir).unwrap();

    let (_dtemp, film_dir) = test_data_dir();

    let width = 64u32;
    let height = 48u32;
    let mut rng = Lcg(112233);
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for _ in 0..(width * height) {
        let v = (3000.0 + rng.next_gaussian(400.0)).clamp(512.0, 16000.0) as u16;
        pixels.push(v);
    }
    let dng_bytes = DngBuilder::new_bayer(width, height).build(&pixels);
    fs::write(raw_dir.join("photo.dng"), &dng_bytes).unwrap();

    let mut store = FolderStore::with_film_data(raw_dir.clone(), lut_dir, Some(film_dir.clone()));

    let items = vec![DevelopItem {
        name: "photo.dng".to_string(),
        look: Some("film:kodak_portra_160".to_string()),
        exposure_ev: 0.0,
        grain: Grain::Off,
        noise_reduction: 50.0,
    }];

    let plan = plan_develop(
        &items,
        OutputFormat::Tiff,
        &store.existing_outputs().unwrap(),
    );
    let summary = execute_develop(plan, &mut store, &NoopProgress);
    assert_eq!(summary.developed, vec!["photo.tif"]);
    assert!(summary.failures.is_empty());

    let output_path = store.output_dir().join("photo.tif");
    let file_bytes = fs::read(&output_path).expect("read TIFF bytes");
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&file_bytes)).unwrap();
    let decoded: Vec<f32> = match decoder.read_image().expect("decode TIFF pixels") {
        tiff::decoder::DecodingResult::U16(v) => v
            .into_iter()
            .map(|c| rollchrome_core::media::develop::tone::prophoto_decode(c as f32 / 65535.0))
            .collect(),
        _ => panic!("expected 16-bit samples"),
    };

    let raw = fs::read(raw_dir.join("photo.dng")).expect("read DNG");
    let linear = rollchrome_core::media::develop::develop_linear(
        &raw,
        &rollchrome_core::media::develop::DevelopOptions { exposure_ev: 0.0 },
    )
    .expect("develop linear");

    let denoised_pixels = rollchrome_core::media::develop::reduce_noise(
        linear.width,
        linear.height,
        &linear.pixels,
        50.0,
        1.0,
    );

    let oracle_with_nr = render_film(
        &denoised_pixels,
        linear.width,
        linear.height,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::ProPhotoLinear,
        FilmBackend::Cpu,
        &film_dir,
    )
    .expect("oracle with nr");
    let oracle_flat: Vec<f32> = oracle_with_nr
        .iter()
        .flatten()
        .map(|v| v.max(0.0))
        .collect();

    let oracle_without_nr = render_film(
        &linear.pixels,
        linear.width,
        linear.height,
        "kodak_portra_160",
        Grain::Off,
        0.0,
        FilmOutput::ProPhotoLinear,
        FilmBackend::Cpu,
        &film_dir,
    )
    .expect("oracle without nr");
    let oracle_no_nr_flat: Vec<f32> = oracle_without_nr
        .iter()
        .flatten()
        .map(|v| v.max(0.0))
        .collect();

    let mad_with = decoded
        .iter()
        .zip(&oracle_flat)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / decoded.len() as f32;

    let mad_without = decoded
        .iter()
        .zip(&oracle_no_nr_flat)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / decoded.len() as f32;

    assert!(
        mad_with < 0.01,
        "must match CPU oracle with noise reduction within 0.01 (got {mad_with})"
    );
    assert!(
        mad_without > 0.001,
        "must not match CPU oracle without noise reduction (got {mad_without})"
    );
}

/// 10. `noise_reduction_preview_cost_is_printed`
#[test]
#[ignore]
fn noise_reduction_preview_cost_is_printed() {
    let w = 1600u32;
    let h = 1067u32;
    let mut rng = Lcg(4444);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for _ in 0..(w * h) {
        let r = (0.18 + rng.next_gaussian(0.02)).max(0.0);
        let g = (0.18 + rng.next_gaussian(0.02)).max(0.0);
        let b = (0.18 + rng.next_gaussian(0.02)).max(0.0);
        pixels.push([r, g, b]);
    }

    let scale = 1.0f32;
    let _ = rollchrome_core::media::develop::reduce_noise(w, h, &pixels, 50.0, scale);

    let mut times = Vec::with_capacity(5);
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let _ = rollchrome_core::media::develop::reduce_noise(w, h, &pixels, 50.0, scale);
        times.push(start.elapsed());
    }
    times.sort();
    let median = times[2];
    println!("noise_reduction_preview_cost: {:?}", median);
    assert!(
        median.as_millis() <= 100,
        "median cost {:?} exceeded 100ms",
        median
    );
}
