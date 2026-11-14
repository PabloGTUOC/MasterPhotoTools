//! Times a Develop run over a folder of RAW files (DV-8).
//!
//! ```sh
//! cargo run --release -p rollchrome-core --features film --example develop_bench -- \
//!     <raw folder> <look> [jpeg|tiff|both] [jobs] [lens|nolens] [sharpen 0..100]
//! ```
//!
//! The RAW files are copied into a temporary folder first, so the source folder is never written
//! to, and nothing reads the user's configuration: the film data lives in its own temporary folder.
//! Run it under `/usr/bin/time -l` for the peak memory.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::media::film::{ensure_film_data, Grain};
use rollchrome_core::tools::develop::{
    execute_develop_with, plan_develop_with, Concurrency, DevelopItem, DevelopOutput,
};
use rollchrome_core::tools::develop_folder::FolderStore;

/// Records when each photograph started, to report time per photograph.
struct Timing {
    start: Instant,
    marks: Mutex<Vec<(String, f64)>>,
}

impl phototools_core::jobs::Progress for Timing {
    fn report(&self, _done: u64, _total: u64, current: &str) {
        if let Ok(mut marks) = self.marks.lock() {
            marks.push((current.to_string(), self.start.elapsed().as_secs_f64()));
        }
    }
    fn cancelled(&self) -> bool {
        false
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [folder, look, rest @ ..] = args.as_slice() else {
        eprintln!("usage: develop_bench <raw folder> <look> [jpeg|tiff|both] [jobs]");
        std::process::exit(2);
    };
    if look == "--stages" {
        return stages(PathBuf::from(folder));
    }
    let format_arg = rest.first().map(String::as_str).unwrap_or("jpeg");
    let jobs: usize = rest.get(1).map(|j| j.parse()).transpose()?.unwrap_or(1);
    let lens = rest.get(2).is_some_and(|l| l == "lens");
    let sharpen: f32 = rest.get(3).map(|s| s.parse()).transpose()?.unwrap_or(0.0);

    let temp = tempfile::tempdir()?;
    let raw_dir = temp.path().join("raws");
    let lut_dir = temp.path().join("luts");
    std::fs::create_dir_all(&raw_dir)?;
    std::fs::create_dir_all(&lut_dir)?;
    let film_dir = ensure_film_data(&temp.path().join("film"))?;

    let mut names = Vec::new();
    for entry in std::fs::read_dir(PathBuf::from(folder))? {
        let path = entry?.path();
        if path.is_file() {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            std::fs::copy(&path, raw_dir.join(&name))?;
            names.push(name);
        }
    }

    let (format, output) = match format_arg {
        "tiff" => (OutputFormat::Tiff, DevelopOutput::default()),
        "both" => (
            OutputFormat::Jpeg { quality: 95 },
            DevelopOutput {
                also_tiff: true,
                ..Default::default()
            },
        ),
        _ => (OutputFormat::Jpeg { quality: 95 }, DevelopOutput::default()),
    };
    let items: Vec<DevelopItem> = names
        .iter()
        .map(|name| DevelopItem {
            name: name.clone(),
            look: Some(look.clone()),
            exposure_ev: 0.0,
            grain: Grain::Fine,
            noise_reduction: 25.0,
            lens_correction: lens,
            capture_sharpening: sharpen,
        })
        .collect();

    let mut store = FolderStore::with_film_data(raw_dir, lut_dir, Some(film_dir));
    let plan = plan_develop_with(&items, format, &output, &store.existing_outputs()?);
    let photos = plan.actions.len();

    let timing = Timing {
        start: Instant::now(),
        marks: Mutex::new(Vec::new()),
    };
    let summary = execute_develop_with(
        plan,
        &mut store,
        &timing,
        Concurrency::for_this_machine(jobs),
    );
    let total = timing.start.elapsed().as_secs_f64();

    println!(
        "look={look} format={format_arg} jobs={jobs} lens={lens} photos={photos} developed={} failures={}",
        summary.developed.len(),
        summary.failures.len()
    );
    for f in summary.failures.iter().chain(&summary.lens_skipped) {
        println!("  failed {}: {}", f.file, f.reason);
    }
    println!(
        "total {total:.2} s, {:.2} s per photograph, {:.1} photographs per minute",
        total / photos.max(1) as f64,
        photos as f64 / total * 60.0
    );
    Ok(())
}

/// Times each stage of one photograph's neutral development.
fn stages(file: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    use rollchrome_core::media::develop::{
        develop_linear, linear_to_developed, reduce_noise, render, DevelopOptions,
    };
    let t = Instant::now();
    let raw = std::fs::read(&file)?;
    let read = t.elapsed().as_secs_f64();
    let linear = develop_linear(&raw, &DevelopOptions { exposure_ev: 0.0 })?;
    let decode = t.elapsed().as_secs_f64();
    let denoised = reduce_noise(linear.width, linear.height, &linear.pixels, 25.0, 1.0);
    let denoise = t.elapsed().as_secs_f64();
    let developed = linear_to_developed(
        linear.width,
        linear.height,
        &denoised,
        linear.orientation,
        linear.demosaic,
    )?;
    let to16 = t.elapsed().as_secs_f64();
    let jpeg = render(&developed, None, OutputFormat::Jpeg { quality: 95 })?;
    let enc = t.elapsed().as_secs_f64();
    println!(
        "{}x{}: read {read:.2}  decode+demosaic {:.2}  denoise {:.2}  to 16-bit {:.2}  jpeg {:.2} ({} KB)  total {enc:.2} s",
        linear.width,
        linear.height,
        decode - read,
        denoise - decode,
        to16 - denoise,
        enc - to16,
        jpeg.bytes.len() / 1024
    );
    Ok(())
}
