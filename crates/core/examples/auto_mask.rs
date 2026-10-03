//! Makes the automatic subject and sky masks of photographs through `core`'s own code path,
//! and reports the time each takes: the measuring tool for MV-22 on the machine the
//! application runs on (ED-23). Not part of the application.
//!
//! cargo run --release -p phototools-core --features segment --example auto_mask -- \
//!     <models dir> <out dir> <photo>...
//!
//! Models missing from <models dir> are downloaded and verified first.

use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::edit::{AutoTarget, ImageBuffer};
use phototools_core::media::segment;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: auto_mask <models dir> <out dir> <photo>...");
        std::process::exit(2);
    }
    let (models, out) = (
        std::path::Path::new(&args[1]),
        std::path::Path::new(&args[2]),
    );
    std::fs::create_dir_all(out)?;
    for target in [AutoTarget::Subject, AutoTarget::Sky] {
        let spec = segment::spec(target);
        if !segment::model_present(models, spec) {
            println!("downloading {}", spec.name);
            segment::download_model(models, spec, &InMemoryProgress::default())?;
        }
    }
    for photo in &args[3..] {
        let path = std::path::Path::new(photo);
        let decoded = phototools_core::media::edit::decode_image(path)?;
        let ImageBuffer::Rgb8 {
            width,
            height,
            data,
        } = decoded.clone()
        else {
            let rgb = image::DynamicImage::ImageRgb16(
                image::ImageBuffer::from_raw(
                    decoded.width(),
                    decoded.height(),
                    decoded.as_rgb16().unwrap().to_vec(),
                )
                .unwrap(),
            )
            .to_rgb8();
            run(path, &rgb, models, out)?;
            continue;
        };
        let rgb = image::RgbImage::from_raw(width, height, data).unwrap();
        run(path, &rgb, models, out)?;
    }
    Ok(())
}

fn run(
    path: &std::path::Path,
    rgb: &image::RgbImage,
    models: &std::path::Path,
    out: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // As `PreviewSession::open` reads it.
    let orientation = phototools_core::media::read_meta(path)
        .map(|m| m.orientation as u32)
        .unwrap_or(1);
    let stem = path.file_stem().unwrap().to_string_lossy();
    for target in [AutoTarget::Subject, AutoTarget::Sky] {
        let t = Instant::now();
        let stored = segment::auto_mask(rgb, orientation, target, models)?;
        let took = t.elapsed();
        let mask = stored.decode()?;
        let name = format!("{stem}_{target:?}.png").to_lowercase();
        image::GrayImage::from_raw(mask.w, mask.h, mask.data)
            .unwrap()
            .save(out.join(&name))?;
        println!(
            "{} {target:?}: {:.2} s, mask {}x{}, {} KB in the sidecar",
            path.display(),
            took.as_secs_f32(),
            stored.width,
            stored.height,
            stored.png.len() / 1024
        );
    }
    Ok(())
}
