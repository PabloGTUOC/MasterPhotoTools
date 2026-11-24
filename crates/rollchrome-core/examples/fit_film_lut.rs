//! Measuring tool for Edit's film LUTs (`media::film::LutInput::NeutralRendering`).
//!
//! For each RAW file, writes what the two screens make of one film stock, so they can be compared:
//! - `<out>/develop/<stem>.jpg`: Develop's film render, as Develop runs it with its default settings,
//!   grain off (a LUT has none);
//! - `<out>/edit_srgb/<stem>.jpg`: Edit's neutral through the LUT built the DV-6 way (sRGB in);
//! - `<out>/edit_<ev>/<stem>.jpg`: Edit's neutral through the LUT built for it, at each exposure.
//!
//! cargo run --release -p rollchrome-core --features film --example fit_film_lut -- \
//!     <out dir> <film data dir> <stock id> <ev,ev,...> <raw files...>

use rollchrome_core::media::develop::{
    correct_lens, develop_linear, effective_noise_reduction, noise_level, reduce_noise,
    DevelopOptions,
};
use rollchrome_core::media::edit::lut::Lut;
use rollchrome_core::media::film::{
    edit_balance, film_lut_with, render_film, FilmBackend, FilmOutput, Grain, LutInput, FILM_STOCKS,
};
use rollchrome_core::tools::develop::{
    develop_for_edit, DEFAULT_CAPTURE_SHARPENING, DEFAULT_NOISE_REDUCTION,
};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "usage: fit_film_lut <out dir> <film data dir> <stock id> <ev,ev,...> <raw files...>"
        );
        std::process::exit(2);
    }
    let out = Path::new(&args[1]);
    let data_dir = Path::new(&args[2]);
    let stock = args[3].as_str();
    let evs: Vec<f32> = args[4]
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()?;
    let info = FILM_STOCKS
        .iter()
        .find(|s| s.id == stock)
        .ok_or("unknown stock")?;
    let balance = edit_balance(info.tungsten);

    let mut luts: Vec<(String, Lut)> = Vec::new();
    let srgb = film_lut_with(stock, balance, data_dir, LutInput::Srgb)?;
    luts.push((
        "edit_srgb".into(),
        Lut::from_bytes("srgb.cube", srgb.cube.as_bytes())?,
    ));
    for ev in &evs {
        let lut = film_lut_with(
            stock,
            balance,
            data_dir,
            LutInput::NeutralRendering { exposure_ev: *ev },
        )?;
        luts.push((
            format!("edit_{ev:+.2}"),
            Lut::from_bytes("edit.cube", lut.cube.as_bytes())?,
        ));
    }

    for file in &args[5..] {
        let path = Path::new(file);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("a file without a name")?;
        let raw = std::fs::read(path)?;

        // Develop, as `tools::develop` runs a film look with the default settings.
        let mut linear = develop_linear(&raw, &DevelopOptions { exposure_ev: 0.0 })?;
        let _ = correct_lens(&mut linear, &raw);
        rollchrome_core::media::develop::frame::apply_camera_geometry(&mut linear, &raw);
        let level = noise_level(&linear.pixels, linear.width, linear.height);
        let nr = effective_noise_reduction(DEFAULT_NOISE_REDUCTION, level);
        let denoised = reduce_noise(linear.width, linear.height, &linear.pixels, nr, 1.0);
        let denoised = rollchrome_core::media::develop::sharpen::capture_sharpen(
            linear.width,
            linear.height,
            &denoised,
            DEFAULT_CAPTURE_SHARPENING,
            rollchrome_core::media::develop::sharpen::sigma_for(matches!(
                linear.demosaic,
                rollchrome_core::media::develop::Demosaic::XTrans
            )),
        );
        let film = render_film(
            &denoised,
            linear.width,
            linear.height,
            stock,
            Grain::Off,
            0.0,
            FilmOutput::Srgb,
            FilmBackend::Auto,
            data_dir,
        )?;
        save(out, "develop", stem, linear.width, linear.height, |i| {
            film[i]
        })?;

        // Edit: the neutral, then each LUT as Edit samples it.
        let (neutral, _) = develop_for_edit(&raw)?;
        let (w, h) = neutral.dimensions();
        let px: Vec<[f32; 3]> = neutral
            .as_raw()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| {
                [
                    f32::from(c[0]) / 65535.0,
                    f32::from(c[1]) / 65535.0,
                    f32::from(c[2]) / 65535.0,
                ]
            })
            .collect();
        for (name, lut) in &luts {
            save(out, name, stem, w, h, |i| lut.sample(px[i]))?;
        }
        println!("{stem}: done");
    }
    Ok(())
}

fn save(
    out: &Path,
    dir: &str,
    stem: &str,
    w: u32,
    h: u32,
    pixel: impl Fn(usize) -> [f32; 3],
) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let p = pixel((y * w + x) as usize);
        image::Rgb(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
    });
    let d = out.join(dir);
    std::fs::create_dir_all(&d)?;
    img.save(d.join(format!("{stem}.jpg")))?;
    Ok(())
}
