//! Measuring tool for a camera's neutral calibration (`media::develop::neutral`).
//!
//! Develops RAW files through Develop's own neutral path (`develop_for_edit_with`) at a grid of
//! exposure and highlight-lift values, so the result can be compared with a reference rendering of the
//! same files — Apple's, from `sips -s format jpeg <raw> --out <jpg>` — and the values that match chosen.
//!
//! cargo run --release -p rollchrome-core --example fit_neutral -- <out dir> <ev,ev,...> <hl,hl,...> <raw files...>
//!
//! Writes `<out dir>/<ev>_<hl>/<stem>.jpg` and prints each file's camera make and model and its current
//! calibration.

use rollchrome_core::media::develop::neutral::{calibration_for, NeutralCalibration};
use rollchrome_core::tools::develop::develop_for_edit_with;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: fit_neutral <out dir> <ev,ev,...> <hl,hl,...> <raw files...>");
        std::process::exit(2);
    }
    let out = Path::new(&args[1]);
    let evs: Vec<f32> = args[2]
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()?;
    let hls: Vec<f32> = args[3]
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()?;

    for file in &args[4..] {
        let path = Path::new(file);
        let raw = std::fs::read(path)?;
        let current = match rollchrome_core::media::lens::lens_info(&raw) {
            Some(info) => {
                let cal = calibration_for(&info.make, &info.model);
                println!(
                    "{}: make {:?}, model {:?}, current calibration {:?}",
                    path.display(),
                    info.make,
                    info.model,
                    cal
                );
                cal
            }
            None => {
                println!("{}: camera not identified", path.display());
                NeutralCalibration::default()
            }
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("a file without a name")?;
        for (ev, hl) in evs.iter().flat_map(|e| hls.iter().map(move |h| (e, h))) {
            // The values replace the camera's exposure and highlight lift and keep its colour matrix,
            // so the best match is the calibration's exposure_ev and highlight_ev as they stand.
            let cal = NeutralCalibration {
                exposure_ev: *ev,
                colour: current.colour,
                highlight_ev: *hl,
            };
            let (img16, _warnings) = develop_for_edit_with(&raw, &cal)?;
            let (w, h) = img16.dimensions();
            let img8 = image::RgbImage::from_fn(w, h, |x, y| {
                let p = img16.get_pixel(x, y);
                image::Rgb([(p[0] >> 8) as u8, (p[1] >> 8) as u8, (p[2] >> 8) as u8])
            });
            let dir = out.join(format!("{ev:+.2}_{hl:+.2}"));
            std::fs::create_dir_all(&dir)?;
            img8.save(dir.join(format!("{stem}.jpg")))?;
        }
    }
    Ok(())
}
