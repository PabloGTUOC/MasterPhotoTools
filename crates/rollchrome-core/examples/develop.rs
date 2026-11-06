//! Measuring tool for RAW development performance (DV-1).
//!
//! Synthetic mode:
//! cargo run --release -p rollchrome-core --example develop -- --synthetic <W>x<H> <bayer|xtrans>
//!
//! Directory mode:
//! cargo run --release -p rollchrome-core --example develop -- <out dir> <raw files...>

#[path = "../tests/support/dng.rs"]
mod dng;

use rollchrome_core::media::develop::{develop, prophoto_to_srgb8, DevelopOptions};
use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }

    if args[1] == "--synthetic" {
        if args.len() < 4 {
            print_usage();
            std::process::exit(2);
        }
        run_synthetic(&args[2], &args[3])?;
    } else {
        if args.len() < 3 {
            print_usage();
            std::process::exit(2);
        }
        run_directory(&args[1], &args[2..])?;
    }

    Ok(())
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  develop --synthetic <W>x<H> <bayer|xtrans>");
    eprintln!("  develop <out dir> <raw files...>");
}

fn run_synthetic(dim_str: &str, pattern: &str) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = dim_str.split('x').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid dimension '{}', expected <W>x<H>", dim_str).into());
    }
    let w: u32 = parts[0].parse()?;
    let h: u32 = parts[1].parse()?;

    println!("Generating synthetic {}x{} {} raw...", w, h, pattern);
    let total_pixels = (w * h) as usize;
    // 0.18 grey sensor raw value with black=512, white=16383
    let grey_sensor_val = (512.0f32 + 0.18f32 * (16383.0f32 - 512.0f32)).round() as u16;
    let raw_pixels = vec![grey_sensor_val; total_pixels];

    let builder = match pattern.to_lowercase().as_str() {
        "bayer" => dng::DngBuilder::new_bayer(w, h),
        "xtrans" => dng::DngBuilder::new_xtrans(w, h),
        other => {
            return Err(format!("Unknown CFA pattern '{}', expected bayer or xtrans", other).into())
        }
    };

    let dng_bytes = builder.build(&raw_pixels);
    println!(
        "DNG size: {:.2} MB. Developing 3 times...",
        dng_bytes.len() as f64 / 1_048_576.0
    );

    let mut times = Vec::with_capacity(3);
    for i in 0..3 {
        let t0 = Instant::now();
        let _developed = develop(&dng_bytes, &DevelopOptions::default())?;
        let elapsed = t0.elapsed();
        println!("Run {}: {:.2?}", i + 1, elapsed);
        times.push(elapsed);
    }

    times.sort();
    let median = times[1];
    println!("Median: {:.2?}", median);

    Ok(())
}

fn run_directory(
    out_dir_str: &str,
    raw_files: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new(out_dir_str);
    std::fs::create_dir_all(out_dir)?;

    for file_str in raw_files {
        let path = Path::new(file_str);
        let raw_bytes = std::fs::read(path)?;

        let t0 = Instant::now();
        let developed = develop(&raw_bytes, &DevelopOptions::default())?;
        let srgb8 = prophoto_to_srgb8(&developed);
        let elapsed = t0.elapsed();

        println!("{}: {:.2?}", path.display(), elapsed);

        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let out_path = out_dir.join(format!("{}.png", stem));
        srgb8.save(&out_path)?;
    }

    Ok(())
}
