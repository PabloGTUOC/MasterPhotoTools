//! Measuring tool for the slide filters (`media::film::SLIDE_FILTERS`): renders a grey wedge — 0.045,
//! 0.18 and 0.72 in linear ProPhoto, two stops either side of mid-grey — through Develop's film render
//! and prints each step's mean sRGB. A filter is right when the middle step's three values match.
//!
//! cargo run --release -p rollchrome-core --features film --example film_grey_wedge -- \
//!     <film data dir> <stock id>

use rollchrome_core::media::film::{render_film, FilmBackend, FilmOutput, Grain};
use std::path::Path;

const STEP: u32 = 32;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: film_grey_wedge <film data dir> <stock id>");
        std::process::exit(2);
    }
    let levels = [0.045f32, 0.18, 0.72];
    let (w, h) = (STEP * 3, STEP);
    let pixels: Vec<[f32; 3]> = (0..w * h)
        .map(|i| [levels[((i % w) / STEP) as usize]; 3])
        .collect();
    let out = render_film(
        &pixels,
        w,
        h,
        &args[2],
        Grain::Off,
        0.0,
        FilmOutput::Srgb,
        FilmBackend::Auto,
        Path::new(&args[1]),
    )?;
    // Each step's middle, away from the scanner's sharpening at the edges between steps.
    for step in 0..3 {
        let mut sum = [0.0f64; 3];
        let mut n = 0.0;
        for y in STEP / 4..STEP * 3 / 4 {
            for x in step * STEP + STEP / 4..step * STEP + STEP * 3 / 4 {
                let p = out[(y * w + x) as usize];
                for c in 0..3 {
                    sum[c] += f64::from(p[c]);
                }
                n += 1.0;
            }
        }
        print!("{:.4} {:.4} {:.4}  ", sum[0] / n, sum[1] / n, sum[2] / n);
    }
    println!();
    Ok(())
}
