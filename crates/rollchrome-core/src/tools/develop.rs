//! Develop folder planning and execution (DV-2b).
//!
//! Takes a folder's RAW files and plans non-overwriting outputs beside them
//! in a `Developed` folder, then executes the plan sequentially.
//!
//! Conforms to rule R2: zero filesystem types in this module. All filesystem
//! operations are abstracted behind the `DevelopStore` trait.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{mpsc, Mutex};

use phototools_core::error::Error;
use phototools_core::jobs::Progress;
use phototools_core::media::raw::RAW_EXTENSIONS;
use phototools_core::tools::Skip;
use serde::{Deserialize, Serialize};

use crate::media::develop::par_map_rgb;
use crate::media::develop::preview::resize_linear;
use crate::media::develop::{
    correct_lens, develop_linear, linear_to_developed_with, reduce_noise, render_sized,
    DevelopOptions, Look, OutputFormat, ToneCurve,
};
use crate::media::film::{render_film, FilmBackend, FilmOutput, Grain};

/// A single file in the folder presented for development.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopItem {
    /// File name in the folder (no directory).
    pub name: String,
    /// LUT identifier in the library (by name or sha256). None leaves photograph untouched.
    pub look: Option<String>,
    /// Exposure adjustment in stops (-4.0..=4.0 EV).
    pub exposure_ev: f32,
    /// Film grain setting (DV-4).
    #[serde(default)]
    pub grain: Grain,
    /// Noise reduction amount (0.0..=100.0, default 0.0) (DV-5).
    #[serde(default)]
    pub noise_reduction: f32,
    /// Correct distortion, lateral CA and vignetting from the lens database (DV-9).
    #[serde(default)]
    pub lens_correction: bool,
    /// Capture sharpening strength, 0–100 (DV-10). Absent means none.
    #[serde(default)]
    pub capture_sharpening: f32,
}

/// The reserved look id that develops a photograph with no look applied (DV-7).
///
/// Distinct from `look: None`, which leaves the photograph out of the run: "develop it plainly" and
/// "not this one" are different requests, and before DV-7 the second was the only one reachable.
pub const NEUTRAL_LOOK: &str = "neutral";

/// Where and how a run writes its outputs (DV-7). The default is what Develop always did: one
/// format, full size, into `Developed` beside the photographs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopOutput {
    /// Also write a 16-bit TIFF beside each JPEG, from the same development.
    #[serde(default)]
    pub also_tiff: bool,
    /// Downscale so the long edge is at most this many pixels; `None` keeps the full size.
    #[serde(default)]
    pub long_edge: Option<u32>,
    /// The output folder. `None` is `Developed` beside the photographs. Callers resolve and check it
    /// against the roots (G6) before planning; the plan records the resolved path.
    #[serde(default)]
    pub folder: Option<String>,
}

impl DevelopOutput {
    /// Refuses settings that cannot mean anything, before any planning.
    pub fn check(&self, format: OutputFormat) -> Result<(), Error> {
        if self.long_edge == Some(0) {
            return Err(Error::Refused(
                "the long edge must be at least 1 pixel".to_string(),
            ));
        }
        if self.also_tiff && matches!(format, OutputFormat::Tiff) {
            return Err(Error::Refused(
                "the format is already TIFF; JPEG + TIFF needs the JPEG format".to_string(),
            ));
        }
        Ok(())
    }
}

/// A planned development step for one photograph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopAction {
    pub source: String,
    pub output: String,
    pub look: String,
    pub exposure_ev: f32,
    /// Film grain setting (DV-4).
    #[serde(default)]
    pub grain: Grain,
    /// Noise reduction amount (0.0..=100.0, default 0.0) (DV-5).
    #[serde(default)]
    pub noise_reduction: f32,
    /// The second output's file name, when the plan writes JPEG + TIFF (DV-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub also_output: Option<String>,
    /// Lens correction (DV-9). Skipped when off, so a plan without it serialises as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lens_correction: bool,
    /// Capture sharpening strength (DV-10). Skipped when 0, for the same reason.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub capture_sharpening: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Result of planning a folder's development.
///
/// The DV-7 fields are skipped when unset, so a plan made with the default output serialises
/// exactly as before, and an automation's reviewed `plan_sha256` stays valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopPlan {
    pub format: OutputFormat,
    pub actions: Vec<DevelopAction>,
    pub skipped: Vec<Skip>,
    /// A second format written beside the first (always TIFF, after a JPEG) (DV-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub also: Option<OutputFormat>,
    /// Long edge the outputs are downscaled to (DV-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_edge: Option<u32>,
    /// The output folder, when it is not `Developed` beside the photographs (DV-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_folder: Option<String>,
}

/// How a message names an output: `Developed/x.jpg`, as it always has, or `x.jpg in <folder>`.
fn destination(folder: Option<&str>, output: &str) -> String {
    match folder {
        None => format!("Developed/{output}"),
        Some(folder) => format!("{output} in {folder}"),
    }
}

/// Summary of a development run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DevelopSummary {
    /// Output file names written.
    pub developed: Vec<String>,
    /// Items skipped during planning plus anything skipped while running.
    pub skipped: Vec<Skip>,
    /// Items that failed development or writing, with the reason.
    pub failures: Vec<Skip>,
    /// Items written whose metadata could not be carried.
    pub metadata_skipped: Vec<Skip>,
    /// Items developed without the lens correction they asked for, and why (DV-9).
    #[serde(default)]
    pub lens_skipped: Vec<Skip>,
}

/// Storage abstraction for developing a folder (R2).
pub trait DevelopStore {
    /// File names already in Developed/, lowercased.
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error>;
    /// Reads file bytes by name.
    fn read(&self, name: &str) -> Result<Vec<u8>, Error>;
    /// Resolves a LUT by name or sha256 from the library.
    fn look(&self, look: &str) -> Result<Look, Error>;
    /// Directory where film simulation data is materialised (DV-4).
    fn film_data_dir(&self) -> Option<&Path> {
        None
    }
    /// Writes `bytes` as a new file. Must fail with `Error::Refused` naming the file if it already exists.
    fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error>;
    /// Carries metadata from source to output with format-specific corrections.
    fn carry_metadata(
        &mut self,
        source: &str,
        output: &str,
        width: u32,
        height: u32,
        format: OutputFormat,
    ) -> Result<(), Error>;
    /// Called when the batch completes (e.g. closes persistent exiftool process).
    fn finish(&mut self) {}
}

/// Plans folder development without performing I/O, with the default output.
///
/// Items are evaluated in byte order of `name`.
pub fn plan_develop(
    items: &[DevelopItem],
    format: OutputFormat,
    existing_outputs: &BTreeSet<String>,
) -> DevelopPlan {
    plan_develop_with(items, format, &DevelopOutput::default(), existing_outputs)
}

/// Plans folder development with output settings (DV-7). `output` is assumed to have passed
/// [`DevelopOutput::check`]; `also_tiff` with a TIFF format writes the one TIFF.
///
/// A photograph is skipped when **either** of its outputs already exists: writing the other alone
/// would leave a pair that came from two different runs, possibly with two different looks.
pub fn plan_develop_with(
    items: &[DevelopItem],
    format: OutputFormat,
    output: &DevelopOutput,
    existing_outputs: &BTreeSet<String>,
) -> DevelopPlan {
    let mut sorted_items = items.to_vec();
    sorted_items.sort_by(|a, b| a.name.cmp(&b.name));

    let mut actions = Vec::new();
    let mut skipped = Vec::new();
    let mut planned_outputs: HashMap<String, String> = HashMap::new();
    let folder = output.folder.as_deref();

    let ext = match format {
        OutputFormat::Jpeg { .. } => "jpg",
        OutputFormat::Tiff => "tif",
    };
    let also = match format {
        OutputFormat::Jpeg { .. } if output.also_tiff => Some(OutputFormat::Tiff),
        _ => None,
    };

    for item in sorted_items {
        // Name check: must be a bare filename without path separators and not empty
        if item.name.is_empty() || item.name.contains('/') || item.name.contains('\\') {
            skipped.push(Skip {
                file: item.name,
                reason: "not a file name".to_string(),
            });
            continue;
        }

        // Look check: a photograph given no look is left alone
        let Some(look) = &item.look else {
            skipped.push(Skip {
                file: item.name,
                reason: "no look chosen".to_string(),
            });
            continue;
        };

        // Extension check against supported RAW extensions
        let raw_ext = match item.name.rfind('.') {
            Some(idx) => &item.name[idx + 1..],
            None => "",
        };
        if !RAW_EXTENSIONS.contains(&raw_ext.to_lowercase().as_str()) {
            skipped.push(Skip {
                file: item.name,
                reason: "not a RAW file Develop can read".to_string(),
            });
            continue;
        }

        // Exposure range check: finite and within [-4.0, 4.0] EV
        if !item.exposure_ev.is_finite() || !(-4.0..=4.0).contains(&item.exposure_ev) {
            skipped.push(Skip {
                file: item.name,
                reason: "exposure must be between \u{2212}4 and +4 EV".to_string(),
            });
            continue;
        }

        // Noise reduction range check: finite and within [0.0, 100.0] (DV-5)
        if !item.noise_reduction.is_finite() || !(0.0..=100.0).contains(&item.noise_reduction) {
            skipped.push(Skip {
                file: item.name,
                reason: "noise reduction must be between 0 and 100".to_string(),
            });
            continue;
        }

        // Capture sharpening range check: finite and within [0.0, 100.0] (DV-10)
        if !item.capture_sharpening.is_finite() || !(0.0..=100.0).contains(&item.capture_sharpening)
        {
            skipped.push(Skip {
                file: item.name,
                reason: "capture sharpening must be between 0 and 100".to_string(),
            });
            continue;
        }

        // Compute output filename: <stem>.<ext>
        let stem = match item.name.rfind('.') {
            Some(idx) => &item.name[..idx],
            None => &item.name,
        };
        let output_name = format!("{stem}.{ext}");
        let also_output = also.map(|_| format!("{stem}.tif"));
        let names: Vec<&String> = std::iter::once(&output_name)
            .chain(also_output.as_ref())
            .collect();

        // Check if an output file already exists on disk
        if let Some(taken) = names
            .iter()
            .find(|n| existing_outputs.contains(&n.to_lowercase()))
        {
            skipped.push(Skip {
                file: item.name,
                reason: format!("{} already exists", destination(folder, taken)),
            });
            continue;
        }

        // Check if an earlier planned item produces the same output
        if let Some((taken, earlier_source)) = names.iter().find_map(|n| {
            planned_outputs
                .get(&n.to_lowercase())
                .map(|earlier| (n, earlier))
        }) {
            skipped.push(Skip {
                file: item.name,
                reason: format!(
                    "{earlier_source} already develops to {}",
                    destination(folder, taken)
                ),
            });
            continue;
        }

        for n in &names {
            planned_outputs.insert(n.to_lowercase(), item.name.clone());
        }
        actions.push(DevelopAction {
            source: item.name,
            output: output_name,
            look: look.clone(),
            exposure_ev: item.exposure_ev,
            grain: item.grain,
            noise_reduction: item.noise_reduction,
            also_output,
            lens_correction: item.lens_correction,
            capture_sharpening: item.capture_sharpening,
        });
    }

    DevelopPlan {
        format,
        actions,
        skipped,
        also,
        long_edge: output.long_edge,
        output_folder: output.folder.clone(),
    }
}

/// A resolved look for development: either a 3D LUT look or a film stock (DV-4).
#[derive(Debug, Clone)]
pub enum ResolvedLook {
    /// No look: the neutral development itself (DV-7).
    Neutral,
    Look(Look),
    Film {
        stock: String,
    },
}

/// Resolves a look identifier into a `ResolvedLook` (RP-1, DV-4).
///
/// Resolves `film:<stock>`, `builtin:<id>`, and custom LUT identifiers from `DevelopStore::look`.
pub fn resolve_look(id: &str, store: &dyn DevelopStore) -> Result<ResolvedLook, Error> {
    // Before the library: a LUT the owner happened to call "neutral" must not change what the
    // reserved id means on another machine.
    if id == NEUTRAL_LOOK {
        Ok(ResolvedLook::Neutral)
    } else if let Some(stock) = id.strip_prefix("film:") {
        #[cfg(feature = "film")]
        {
            if crate::media::film::FILM_STOCKS
                .iter()
                .any(|s| s.id == stock)
            {
                Ok(ResolvedLook::Film {
                    stock: stock.to_string(),
                })
            } else {
                Err(Error::Refused(format!("unknown film stock: '{stock}'")))
            }
        }
        #[cfg(not(feature = "film"))]
        {
            let _ = stock;
            Err(Error::Refused(
                "film simulation is not built into this program".to_string(),
            ))
        }
    } else if let Some(builtin_id) = id.strip_prefix("builtin:") {
        crate::media::looks::builtin::builtin_look(builtin_id).map(ResolvedLook::Look)
    } else {
        store.look(id).map(ResolvedLook::Look)
    }
}

/// One encoded output of one photograph, ready to write.
struct Encoded {
    format: OutputFormat,
    output: String,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

/// Develops one photograph and encodes every output the plan asks for (DV-7).
///
/// The RAW is decoded and denoised once. A LUT or neutral look renders once per format from the
/// same development; a film look renders once per format, because spektrafilm produces sRGB for the
/// JPEG and linear ProPhoto for the TIFF. A long edge downscales after the look, so film grain is
/// rendered on the full frame and keeps its scale.
fn develop_one(
    action: &DevelopAction,
    look: &ResolvedLook,
    raw_bytes: &[u8],
    plan: &DevelopPlan,
    film_data_dir: Option<&Path>,
) -> Result<(Vec<Encoded>, Option<String>), String> {
    let outputs: Vec<(OutputFormat, String)> =
        std::iter::once((plan.format, action.output.clone()))
            .chain(plan.also.zip(action.also_output.clone()))
            .collect();

    // A film look takes exposure inside the simulation, as light reaching the negative; a LUT or
    // neutral development takes it on the linear data.
    let exposure_ev = match look {
        ResolvedLook::Film { .. } => 0.0,
        _ => action.exposure_ev,
    };
    let mut linear =
        develop_linear(raw_bytes, &DevelopOptions { exposure_ev }).map_err(|e| e.to_string())?;
    // Before noise reduction and the look: the lens models describe the sensor's image (DV-9).
    let lens_note = if action.lens_correction {
        correct_lens(&mut linear, raw_bytes).err()
    } else {
        None
    };
    let denoised = reduce_noise(
        linear.width,
        linear.height,
        &linear.pixels,
        action.noise_reduction,
        1.0,
    );
    // After the lens and the noise, before the look: grain is added later and never sharpened (DV-10).
    let denoised = if action.capture_sharpening > 0.0 {
        crate::media::develop::sharpen::capture_sharpen(
            linear.width,
            linear.height,
            &denoised,
            action.capture_sharpening,
            crate::media::develop::sharpen::sigma_for(matches!(
                linear.demosaic,
                crate::media::develop::Demosaic::XTrans
            )),
        )
    } else {
        denoised
    };

    let mut encoded = Vec::with_capacity(outputs.len());
    match look {
        ResolvedLook::Neutral | ResolvedLook::Look(_) => {
            let (look_arg, tone) = match look {
                ResolvedLook::Look(l) => (Some(l), ToneCurve::Base),
                ResolvedLook::Neutral => (None, ToneCurve::Standard),
                ResolvedLook::Film { .. } => unreachable!(),
            };
            let developed = linear_to_developed_with(
                linear.width,
                linear.height,
                &denoised,
                linear.orientation,
                linear.demosaic,
                tone,
            )
            .map_err(|e| e.to_string())?;
            for (format, output) in outputs {
                let r = render_sized(&developed, look_arg, format, plan.long_edge)
                    .map_err(|e| e.to_string())?;
                encoded.push(Encoded {
                    format,
                    output,
                    bytes: r.bytes,
                    width: r.width,
                    height: r.height,
                });
            }
        }
        ResolvedLook::Film { stock } => {
            // No silent default: a store that cannot say where film data lives must not
            // have it written somewhere the user never chose (G10).
            let Some(data_dir) = film_data_dir else {
                return Err("no folder for the film simulation data was given".to_string());
            };
            for (format, output) in outputs {
                let (film_output, quality) = match format {
                    OutputFormat::Jpeg { quality } => {
                        if !(1..=100).contains(&quality) {
                            return Err(format!("JPEG quality {quality} is outside 1..=100"));
                        }
                        (FilmOutput::Srgb, Some(quality))
                    }
                    OutputFormat::Tiff => (FilmOutput::ProPhotoLinear, None),
                };
                let rendered = render_film(
                    &denoised,
                    linear.width,
                    linear.height,
                    stock,
                    action.grain,
                    action.exposure_ev,
                    film_output,
                    FilmBackend::Auto,
                    data_dir,
                )
                .map_err(|e| e.to_string())?;
                let (width, height, pixels) = match plan.long_edge {
                    None => (linear.width, linear.height, rendered),
                    Some(edge) => resize_linear(linear.width, linear.height, &rendered, edge)
                        .map_err(|e| e.to_string())?,
                };
                let bytes = match quality {
                    Some(quality) => encode_film_jpeg(width, height, pixels, quality)?,
                    None => encode_film_tiff(width, height, pixels)?,
                };
                encoded.push(Encoded {
                    format,
                    output,
                    bytes,
                    width,
                    height,
                });
            }
        }
    }
    Ok((encoded, lens_note))
}

/// Encodes a film render's display sRGB as an 8-bit JPEG.
fn encode_film_jpeg(
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let u8_data: Vec<u8> = par_map_rgb(&pixels, |p| {
        [
            (p[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (p[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (p[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    });
    let img = image::RgbImage::from_raw(width, height, u8_data)
        .ok_or_else(|| "failed to create RgbImage buffer".to_string())?;
    let opts = phototools_core::media::jpeg::JpegOptions::deliverable(quality);
    phototools_core::media::jpeg::encode(&image::DynamicImage::ImageRgb8(img), &opts)
        .map_err(|e| e.to_string())
}

/// Encodes a film render's linear ProPhoto as a 16-bit TIFF with its ICC profile.
fn encode_film_tiff(width: u32, height: u32, pixels: Vec<[f32; 3]>) -> Result<Vec<u8>, String> {
    let u16_data: Vec<u16> = par_map_rgb(&pixels, |p| {
        let mut out = [0u16; 3];
        for c in 0..3 {
            let enc = crate::media::develop::tone::prophoto_encode(p[c].max(0.0));
            out[c] = (enc * 65535.0).round().clamp(0.0, 65535.0) as u16;
        }
        out
    });
    let icc_profile =
        crate::media::develop::render::prophoto_icc_profile().map_err(|e| e.to_string())?;
    crate::media::develop::render::encode_tiff(width, height, &u16_data, &icc_profile)
        .map_err(|e| e.to_string())
}

/// How many photographs a run develops at once (DV-8).
///
/// Each photograph already uses every core while it demosaics, denoises and converts, but reading,
/// JPEG encoding, the film simulation's GPU queue and the metadata write are serial, and a second
/// photograph fills those gaps. Memory, not cores, is the real limit: a photograph's peak is
/// estimated from its file size and the run keeps only as many in flight as fit in the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Concurrency {
    /// The most photographs in flight at once; at least 1.
    pub max_jobs: usize,
    /// The bytes of estimated peak memory the photographs in flight may hold together. One
    /// photograph always runs, however large.
    pub memory_budget: u64,
}

impl Concurrency {
    /// One photograph after another: what Develop did before DV-8.
    pub fn sequential() -> Self {
        Self {
            max_jobs: 1,
            memory_budget: u64::MAX,
        }
    }

    /// Up to `max_jobs` photographs, within half of this machine's physical memory.
    ///
    /// Half, because the system, the application and the preview need the rest, and swapping costs
    /// far more than a photograph waiting its turn. Unknown memory counts as 8 GB.
    pub fn for_this_machine(max_jobs: usize) -> Self {
        Self {
            max_jobs: max_jobs.max(1),
            memory_budget: physical_memory().unwrap_or(8 << 30) / 2,
        }
    }
}

/// The photographs a run develops at once by default (DV-8).
///
/// Measured on the owner's six XZ-1 ORFs on a 10-core Mac (2026-11-13): 1, 2, 3, 4 and 5 at once gave
/// 44, 56, 71, 90 and 81 photographs a minute developed neutrally, and 27, 36, 50, 58 and 55 through
/// Portra 400. Beyond four the photographs only compete for the same cores.
pub const DEFAULT_JOBS: usize = 4;

/// The machine's physical memory in bytes, if the platform says.
pub fn physical_memory() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: sysconf reads two system constants and has no preconditions.
        let (pages, page_size) = unsafe {
            (
                libc::sysconf(libc::_SC_PHYS_PAGES),
                libc::sysconf(libc::_SC_PAGESIZE),
            )
        };
        if pages > 0 && page_size > 0 {
            return Some(pages as u64 * page_size as u64);
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Estimated peak memory of developing one photograph, from its file size (DV-8).
///
/// Measured on 2026-11-13 (release build, peak resident size of the whole process):
/// - an XZ-1 ORF, 11 MB for 10 MP: 1.5 GB neutral, 2.1 GB with a film stock;
/// - an X-Trans RAF, 20 MB for 24 MP: 3.5 GB neutral.
///
/// Pixels are estimated at 1.25 per byte of file, the densest of those, so a compressed RAW is never
/// underestimated. Per pixel: 160 bytes developed neutrally or with a LUT, 250 through the film
/// simulation, and 50 more when a TIFF is written beside the JPEG.
pub fn estimated_peak(file_len: usize, film: bool, two_outputs: bool) -> u64 {
    let pixels = file_len as u64 * 5 / 4;
    let per_pixel = if film { 250 } else { 160 } + if two_outputs { 50 } else { 0 };
    pixels * per_pixel
}

/// Executes a development plan, one photograph after another.
pub fn execute_develop(
    plan: DevelopPlan,
    store: &mut dyn DevelopStore,
    progress: &dyn Progress,
) -> DevelopSummary {
    execute_develop_with(plan, store, progress, Concurrency::sequential())
}

/// One photograph handed to a worker.
struct Job {
    index: usize,
    look: ResolvedLook,
    raw: Vec<u8>,
}

/// A photograph read and costed, waiting for room in the memory budget.
struct Staged {
    job: Job,
    cost: u64,
}

/// Executes a development plan with several photographs in flight (DV-8).
///
/// The store is only ever used from the calling thread: reading, resolving looks, writing and
/// carrying metadata stay serial, so one persistent exiftool (G4) serves the whole run. Workers only
/// develop and encode. Outputs are written **in plan order**, so the summary, the progress and the
/// files on disk come out exactly as a sequential run's, whatever finishes first. With
/// [`Concurrency::sequential`] the store sees the same calls, in the same order, as before DV-8.
///
/// At most `max_jobs` photographs are between "read" and "written" at any moment, which bounds the
/// encoded outputs held waiting for an earlier, slower photograph.
pub fn execute_develop_with(
    plan: DevelopPlan,
    store: &mut dyn DevelopStore,
    progress: &dyn Progress,
    concurrency: Concurrency,
) -> DevelopSummary {
    let total = plan.actions.len();
    let mut summary = DevelopSummary {
        developed: Vec::new(),
        skipped: plan.skipped.clone(),
        failures: Vec::new(),
        metadata_skipped: Vec::new(),
        lens_skipped: Vec::new(),
    };
    let jobs = concurrency.max_jobs.max(1);
    let film_data_dir = store.film_data_dir().map(Path::to_path_buf);
    let plan = &plan;
    let film_data_dir = film_data_dir.as_deref();

    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let job_rx = Mutex::new(job_rx);
    let (done_tx, done_rx) = mpsc::channel::<(usize, Outcome)>();

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let job_rx = &job_rx;
            let done_tx = done_tx.clone();
            scope.spawn(move || loop {
                let next = job_rx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .recv();
                let Ok(job) = next else { break };
                let index = job.index;
                let action = &plan.actions[index];
                // A panic in one photograph must fail that photograph, not the run (G10).
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    develop_one(action, &job.look, &job.raw, plan, film_data_dir)
                }))
                .unwrap_or_else(|_| {
                    Err("developing this photograph failed unexpectedly".to_string())
                });
                // The RAW bytes go before the result is sent, so they never sit in memory twice.
                drop(job);
                if done_tx.send((index, result)).is_err() {
                    break;
                }
            });
        }
        drop(done_tx);

        let mut look_cache: HashMap<String, Result<ResolvedLook, String>> = HashMap::new();
        let mut finished: BTreeMap<usize, Outcome> = BTreeMap::new();
        let mut in_flight: HashMap<usize, u64> = HashMap::new();
        let mut reserved: u64 = 0;
        let mut staged: Option<Staged> = None;
        let mut next = 0;
        let mut written = 0;
        let mut cancelled = false;

        loop {
            // Write what is ready first, so a finished photograph's outputs leave memory before
            // another photograph is read.
            while let Some(result) = finished.remove(&written) {
                write_outcome(&plan.actions[written], result, store, &mut summary);
                written += 1;
            }

            // Start photographs while the window and the memory budget allow.
            loop {
                if progress.cancelled() {
                    cancelled = true;
                    staged = None;
                    break;
                }
                if staged.is_none() {
                    if next >= total || next >= written + jobs {
                        break;
                    }
                    let action = &plan.actions[next];
                    progress.report(written as u64, total as u64, &action.source);
                    let index = next;
                    next += 1;

                    let look = match look_cache.entry(action.look.clone()).or_insert_with(|| {
                        resolve_look(&action.look, store).map_err(|e| e.to_string())
                    }) {
                        Ok(l) => l.clone(),
                        Err(reason) => {
                            finished.insert(index, Err(reason.clone()));
                            continue;
                        }
                    };
                    let raw = match store.read(&action.source) {
                        Ok(b) => b,
                        Err(e) => {
                            finished.insert(index, Err(e.to_string()));
                            continue;
                        }
                    };
                    let cost = estimated_peak(
                        raw.len(),
                        matches!(look, ResolvedLook::Film { .. }),
                        plan.also.is_some(),
                    );
                    staged = Some(Staged {
                        job: Job { index, look, raw },
                        cost,
                    });
                }
                let Some(s) = staged.take() else { break };
                if !in_flight.is_empty()
                    && reserved.saturating_add(s.cost) > concurrency.memory_budget
                {
                    staged = Some(s);
                    break;
                }
                let index = s.job.index;
                reserved = reserved.saturating_add(s.cost);
                in_flight.insert(index, s.cost);
                if job_tx.send(s.job).is_err() {
                    reserved = reserved.saturating_sub(s.cost);
                    in_flight.remove(&index);
                    finished.insert(index, Err("the develop workers stopped".to_string()));
                }
            }

            // Photographs that failed before reaching a worker are ready at once.
            while let Some(result) = finished.remove(&written) {
                write_outcome(&plan.actions[written], result, store, &mut summary);
                written += 1;
            }

            if in_flight.is_empty() && staged.is_none() && (next >= total || cancelled) {
                break;
            }
            if in_flight.is_empty() {
                continue;
            }
            match done_rx.recv() {
                Ok((index, result)) => {
                    if let Some(cost) = in_flight.remove(&index) {
                        reserved = reserved.saturating_sub(cost);
                    }
                    finished.insert(index, result);
                }
                Err(_) => break,
            }
        }
        drop(job_tx);
    });

    store.finish();
    progress.report(total as u64, total as u64, "done");
    summary
}

/// What a worker hands back: the outputs and a lens note, or why the photograph failed.
type Outcome = Result<(Vec<Encoded>, Option<String>), String>;

/// Writes one photograph's outputs and carries its metadata, recording what happened.
fn write_outcome(
    action: &DevelopAction,
    result: Outcome,
    store: &mut dyn DevelopStore,
    summary: &mut DevelopSummary,
) {
    let encoded = match result {
        Ok((e, lens_note)) => {
            if let Some(reason) = lens_note {
                summary.lens_skipped.push(Skip {
                    file: action.source.clone(),
                    reason: format!("developed without lens correction: {reason}"),
                });
            }
            e
        }
        Err(reason) => {
            summary.failures.push(Skip {
                file: action.source.clone(),
                reason,
            });
            return;
        }
    };
    for out in encoded {
        match store.create_new(&out.output, &out.bytes) {
            Ok(()) => {}
            Err(Error::Refused(reason)) => {
                summary.skipped.push(Skip {
                    file: action.source.clone(),
                    reason,
                });
                continue;
            }
            Err(e) => {
                summary.failures.push(Skip {
                    file: action.source.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        }

        summary.developed.push(out.output.clone());

        if let Err(e) = store.carry_metadata(
            &action.source,
            &out.output,
            out.width,
            out.height,
            out.format,
        ) {
            summary.metadata_skipped.push(Skip {
                file: action.source.clone(),
                reason: e.to_string(),
            });
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../tests/support/dng.rs"]
mod dng;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::edit::lut::Lut;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::sync::Mutex;

    fn make_test_dng(w: u32, h: u32) -> Vec<u8> {
        let builder = dng::DngBuilder::new_bayer(w, h);
        let pixels = vec![3000u16; (w * h) as usize];
        builder.build(&pixels)
    }

    fn make_identity_lut() -> Lut {
        let cube = b"TITLE \"Identity\"\nLUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n";
        Lut::from_cube("identity.cube", cube).expect("valid identity LUT")
    }

    struct InMemoryStore {
        existing: BTreeSet<String>,
        files: HashMap<String, Vec<u8>>,
        luts: HashMap<String, Lut>,
        created: Vec<(String, Vec<u8>)>,
        metadata_carried: Vec<(String, String, u32, u32, OutputFormat)>,
        finish_called: usize,
        refuse_create: HashSet<String>,
        look_call_count: RefCell<HashMap<String, usize>>,
    }

    impl InMemoryStore {
        fn new() -> Self {
            Self {
                existing: BTreeSet::new(),
                files: HashMap::new(),
                luts: HashMap::new(),
                created: Vec::new(),
                metadata_carried: Vec::new(),
                finish_called: 0,
                refuse_create: HashSet::new(),
                look_call_count: RefCell::new(HashMap::new()),
            }
        }
    }

    impl DevelopStore for InMemoryStore {
        fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
            Ok(self.existing.clone())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
            self.files
                .get(name)
                .cloned()
                .ok_or_else(|| Error::Internal(format!("File not found: {name}")))
        }

        fn look(&self, look: &str) -> Result<Look, Error> {
            *self
                .look_call_count
                .borrow_mut()
                .entry(look.to_string())
                .or_insert(0) += 1;
            self.luts.get(look).cloned().map(Look::srgb).ok_or_else(|| {
                Error::Refused(format!("the look '{look}' is not in the LUT library"))
            })
        }

        fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error> {
            if self.refuse_create.contains(output) || self.existing.contains(&output.to_lowercase())
            {
                return Err(Error::Refused(format!("Developed/{output} already exists")));
            }
            self.created.push((output.to_string(), bytes.to_vec()));
            self.existing.insert(output.to_lowercase());
            Ok(())
        }

        fn carry_metadata(
            &mut self,
            source: &str,
            output: &str,
            width: u32,
            height: u32,
            format: OutputFormat,
        ) -> Result<(), Error> {
            self.metadata_carried.push((
                source.to_string(),
                output.to_string(),
                width,
                height,
                format,
            ));
            Ok(())
        }

        fn finish(&mut self) {
            self.finish_called += 1;
        }
    }

    #[derive(Default)]
    struct MockProgress {
        reports: Mutex<Vec<(u64, u64, String)>>,
        cancelled_after_n_reports: Option<usize>,
    }

    impl Progress for MockProgress {
        fn report(&self, done: u64, total: u64, current: &str) {
            self.reports
                .lock()
                .unwrap()
                .push((done, total, current.to_string()));
        }

        fn cancelled(&self) -> bool {
            if let Some(limit) = self.cancelled_after_n_reports {
                self.reports.lock().unwrap().len() >= limit
            } else {
                false
            }
        }
    }

    #[test]
    fn each_raw_with_a_look_is_planned_to_its_own_name() {
        let items = vec![DevelopItem {
            name: "DSCF1234.RAF".to_string(),
            look: Some("Astia".to_string()),
            exposure_ev: 0.5,
            grain: Grain::Fine,
            noise_reduction: 0.0,
            lens_correction: false,
            capture_sharpening: 0.0,
        }];
        let existing = BTreeSet::new();

        // JPEG
        let plan_jpeg = plan_develop(&items, OutputFormat::Jpeg { quality: 90 }, &existing);
        assert_eq!(plan_jpeg.actions.len(), 1);
        assert_eq!(plan_jpeg.skipped.len(), 0);
        assert_eq!(plan_jpeg.actions[0].source, "DSCF1234.RAF");
        assert_eq!(plan_jpeg.actions[0].output, "DSCF1234.jpg");
        assert_eq!(plan_jpeg.actions[0].look, "Astia");
        assert_eq!(plan_jpeg.actions[0].exposure_ev, 0.5);
        assert_eq!(plan_jpeg.actions[0].grain, Grain::Fine);
        assert_eq!(plan_jpeg.actions[0].noise_reduction, 0.0);

        // TIFF
        let plan_tiff = plan_develop(&items, OutputFormat::Tiff, &existing);
        assert_eq!(plan_tiff.actions.len(), 1);
        assert_eq!(plan_tiff.actions[0].output, "DSCF1234.tif");
        assert_eq!(plan_tiff.actions[0].look, "Astia");
        assert_eq!(plan_tiff.actions[0].exposure_ev, 0.5);
        assert_eq!(plan_tiff.actions[0].grain, Grain::Fine);
        assert_eq!(plan_tiff.actions[0].noise_reduction, 0.0);
    }

    #[test]
    fn a_photograph_given_no_look_is_left_alone() {
        let items = vec![DevelopItem {
            name: "DSCF1234.RAF".to_string(),
            look: None,
            exposure_ev: 0.0,
            grain: Grain::Fine,
            noise_reduction: 0.0,
            lens_correction: false,
            capture_sharpening: 0.0,
        }];
        let existing = BTreeSet::new();
        let plan = plan_develop(&items, OutputFormat::Tiff, &existing);

        assert_eq!(plan.actions.len(), 0);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].file, "DSCF1234.RAF");
        assert_eq!(plan.skipped[0].reason, "no look chosen");
    }

    #[test]
    fn a_name_already_developed_is_skipped_never_renamed() {
        let mut existing = BTreeSet::new();
        existing.insert("dscf1234.jpg".to_string());

        let items = vec![DevelopItem {
            name: "DSCF1234.RAF".to_string(),
            look: Some("Velvia".to_string()),
            exposure_ev: 0.0,
            grain: Grain::Fine,
            noise_reduction: 0.0,
            lens_correction: false,
            capture_sharpening: 0.0,
        }];
        let plan = plan_develop(&items, OutputFormat::Jpeg { quality: 90 }, &existing);

        assert_eq!(plan.actions.len(), 0);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].file, "DSCF1234.RAF");
        assert_eq!(
            plan.skipped[0].reason,
            "Developed/DSCF1234.jpg already exists"
        );
        assert!(!plan.skipped[0].reason.contains("_1"));
    }

    #[test]
    fn two_raws_sharing_a_name_develop_only_the_first() {
        // Given in reverse name order: IMG_0001.DNG before IMG_0001.CR2
        let items = vec![
            DevelopItem {
                name: "IMG_0001.DNG".to_string(),
                look: Some("ClassicChrome".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "IMG_0001.CR2".to_string(),
                look: Some("ClassicChrome".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
        ];
        let existing = BTreeSet::new();
        let plan = plan_develop(&items, OutputFormat::Jpeg { quality: 90 }, &existing);

        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].source, "IMG_0001.CR2");
        assert_eq!(plan.actions[0].output, "IMG_0001.jpg");

        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].file, "IMG_0001.DNG");
        assert_eq!(
            plan.skipped[0].reason,
            "IMG_0001.CR2 already develops to Developed/IMG_0001.jpg"
        );
    }

    #[test]
    fn a_file_that_is_not_a_raw_is_skipped_with_its_reason() {
        let items = vec![
            DevelopItem {
                name: "DSCF1234.JPG".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "notes.txt".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
        ];
        let existing = BTreeSet::new();
        let plan = plan_develop(&items, OutputFormat::Tiff, &existing);

        assert_eq!(plan.actions.len(), 0);
        assert_eq!(plan.skipped.len(), 2);
        assert_eq!(plan.skipped[0].file, "DSCF1234.JPG");
        assert_eq!(plan.skipped[0].reason, "not a RAW file Develop can read");
        assert_eq!(plan.skipped[1].file, "notes.txt");
        assert_eq!(plan.skipped[1].reason, "not a RAW file Develop can read");
    }

    #[test]
    fn an_exposure_outside_four_stops_is_skipped() {
        let items = vec![
            DevelopItem {
                name: "A.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: -4.5,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "B.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 4.5,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "C.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: f32::NAN,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "D.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: -4.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "E.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 4.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
        ];
        let existing = BTreeSet::new();
        let plan = plan_develop(&items, OutputFormat::Tiff, &existing);

        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.actions[0].source, "D.RAF");
        assert_eq!(plan.actions[1].source, "E.RAF");

        assert_eq!(plan.skipped.len(), 3);
        for s in &plan.skipped {
            assert_eq!(s.reason, "exposure must be between \u{2212}4 and +4 EV");
        }
    }

    #[test]
    fn a_name_with_a_directory_is_skipped() {
        let items = vec![
            DevelopItem {
                name: "../x.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "a/b.RAF".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
            DevelopItem {
                name: "".to_string(),
                look: Some("Look".to_string()),
                exposure_ev: 0.0,
                grain: Grain::Fine,
                noise_reduction: 0.0,
                lens_correction: false,
                capture_sharpening: 0.0,
            },
        ];
        let existing = BTreeSet::new();
        let plan = plan_develop(&items, OutputFormat::Tiff, &existing);

        assert_eq!(plan.actions.len(), 0);
        assert_eq!(plan.skipped.len(), 3);
        assert_eq!(plan.skipped[0].file, "");
        assert_eq!(plan.skipped[0].reason, "not a file name");
        assert_eq!(plan.skipped[1].file, "../x.RAF");
        assert_eq!(plan.skipped[1].reason, "not a file name");
        assert_eq!(plan.skipped[2].file, "a/b.RAF");
        assert_eq!(plan.skipped[2].reason, "not a file name");
    }

    #[test]
    fn executing_writes_each_output_once_and_carries_its_metadata() {
        let mut store = InMemoryStore::new();
        let dng1 = make_test_dng(24, 24);
        let dng2 = make_test_dng(24, 24);
        store.files.insert("raw1.dng".to_string(), dng1);
        store.files.insert("raw2.dng".to_string(), dng2);
        store
            .luts
            .insert("test_look".to_string(), make_identity_lut());

        let plan = DevelopPlan {
            format: OutputFormat::Jpeg { quality: 90 },
            actions: vec![
                DevelopAction {
                    source: "raw1.dng".to_string(),
                    output: "raw1.jpg".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
                DevelopAction {
                    source: "raw2.dng".to_string(),
                    output: "raw2.jpg".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
            ],
            skipped: Vec::new(),
            also: None,
            long_edge: None,
            output_folder: None,
        };

        let progress = MockProgress::default();
        let summary = execute_develop(plan, &mut store, &progress);

        assert_eq!(summary.developed.len(), 2);
        assert_eq!(summary.failures.len(), 0);
        assert_eq!(summary.skipped.len(), 0);
        assert_eq!(store.created.len(), 2);
        assert_eq!(store.metadata_carried.len(), 2);
        assert_eq!(store.finish_called, 1);

        assert_eq!(store.created[0].0, "raw1.jpg");
        assert_eq!(store.created[1].0, "raw2.jpg");
        assert_eq!(store.metadata_carried[0].0, "raw1.dng");
        assert_eq!(store.metadata_carried[0].1, "raw1.jpg");
        assert_eq!(
            store.metadata_carried[0].4,
            OutputFormat::Jpeg { quality: 90 }
        );
        assert_eq!(store.metadata_carried[1].0, "raw2.dng");
        assert_eq!(store.metadata_carried[1].1, "raw2.jpg");
    }

    #[test]
    fn an_output_that_appeared_after_planning_is_skipped_not_overwritten() {
        let mut store = InMemoryStore::new();
        let dng1 = make_test_dng(24, 24);
        let dng2 = make_test_dng(24, 24);
        store.files.insert("raw1.dng".to_string(), dng1);
        store.files.insert("raw2.dng".to_string(), dng2);
        store
            .luts
            .insert("test_look".to_string(), make_identity_lut());

        // raw1.jpg appeared since the plan
        store.refuse_create.insert("raw1.jpg".to_string());

        let plan = DevelopPlan {
            format: OutputFormat::Tiff,
            actions: vec![
                DevelopAction {
                    source: "raw1.dng".to_string(),
                    output: "raw1.jpg".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
                DevelopAction {
                    source: "raw2.dng".to_string(),
                    output: "raw2.jpg".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
            ],
            skipped: Vec::new(),
            also: None,
            long_edge: None,
            output_folder: None,
        };

        let progress = MockProgress::default();
        let summary = execute_develop(plan, &mut store, &progress);

        assert_eq!(summary.developed.len(), 1);
        assert_eq!(summary.developed[0], "raw2.jpg");
        assert_eq!(summary.skipped.len(), 1);
        assert_eq!(summary.skipped[0].file, "raw1.dng");
        assert_eq!(
            summary.skipped[0].reason,
            "Developed/raw1.jpg already exists"
        );
        assert_eq!(store.created.len(), 1);
    }

    #[test]
    fn a_raw_that_cannot_be_developed_fails_alone() {
        let mut store = InMemoryStore::new();
        let dng_good = make_test_dng(24, 24);
        store
            .files
            .insert("corrupt.dng".to_string(), vec![0x42u8; 100]);
        store.files.insert("good.dng".to_string(), dng_good);
        store
            .luts
            .insert("test_look".to_string(), make_identity_lut());

        let plan = DevelopPlan {
            format: OutputFormat::Tiff,
            actions: vec![
                DevelopAction {
                    source: "corrupt.dng".to_string(),
                    output: "corrupt.tif".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
                DevelopAction {
                    source: "good.dng".to_string(),
                    output: "good.tif".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
            ],
            skipped: Vec::new(),
            also: None,
            long_edge: None,
            output_folder: None,
        };

        let progress = MockProgress::default();
        let summary = execute_develop(plan, &mut store, &progress);

        assert_eq!(summary.failures.len(), 1);
        assert_eq!(summary.failures[0].file, "corrupt.dng");
        assert_eq!(summary.developed.len(), 1);
        assert_eq!(summary.developed[0], "good.tif");
    }

    #[test]
    fn cancelling_stops_before_the_next_photograph() {
        let mut store = InMemoryStore::new();
        let dng1 = make_test_dng(24, 24);
        let dng2 = make_test_dng(24, 24);
        store.files.insert("raw1.dng".to_string(), dng1);
        store.files.insert("raw2.dng".to_string(), dng2);
        store
            .luts
            .insert("test_look".to_string(), make_identity_lut());

        let plan = DevelopPlan {
            format: OutputFormat::Tiff,
            actions: vec![
                DevelopAction {
                    source: "raw1.dng".to_string(),
                    output: "raw1.tif".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
                DevelopAction {
                    source: "raw2.dng".to_string(),
                    output: "raw2.tif".to_string(),
                    look: "test_look".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
            ],
            skipped: Vec::new(),
            also: None,
            long_edge: None,
            output_folder: None,
        };

        // Cancel after the first report
        let progress = MockProgress {
            reports: Mutex::new(Vec::new()),
            cancelled_after_n_reports: Some(1),
        };

        let summary = execute_develop(plan, &mut store, &progress);

        assert_eq!(summary.developed.len(), 1);
        assert_eq!(summary.developed[0], "raw1.tif");
        assert_eq!(summary.failures.len(), 0);
        assert_eq!(store.created.len(), 1);
    }

    #[test]
    fn a_look_that_left_the_library_fails_its_photographs_with_the_reason() {
        let mut store = InMemoryStore::new();
        let dng1 = make_test_dng(24, 24);
        let dng2 = make_test_dng(24, 24);
        store.files.insert("raw1.dng".to_string(), dng1);
        store.files.insert("raw2.dng".to_string(), dng2);
        // Do not add "vanished_lut" to store.luts

        let plan = DevelopPlan {
            format: OutputFormat::Tiff,
            actions: vec![
                DevelopAction {
                    source: "raw1.dng".to_string(),
                    output: "raw1.tif".to_string(),
                    look: "vanished_lut".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
                DevelopAction {
                    source: "raw2.dng".to_string(),
                    output: "raw2.tif".to_string(),
                    look: "vanished_lut".to_string(),
                    exposure_ev: 0.0,
                    grain: Grain::Fine,
                    noise_reduction: 0.0,
                    also_output: None,
                    lens_correction: false,
                    capture_sharpening: 0.0,
                },
            ],
            skipped: Vec::new(),
            also: None,
            long_edge: None,
            output_folder: None,
        };

        let progress = MockProgress::default();
        let summary = execute_develop(plan, &mut store, &progress);

        assert_eq!(summary.failures.len(), 2);
        assert_eq!(summary.failures[0].file, "raw1.dng");
        assert_eq!(
            summary.failures[0].reason,
            "the look 'vanished_lut' is not in the LUT library"
        );
        assert_eq!(summary.failures[1].file, "raw2.dng");
        assert_eq!(
            summary.failures[1].reason,
            "the look 'vanished_lut' is not in the LUT library"
        );

        // LUT resolution was cached, called exactly once for "vanished_lut"
        assert_eq!(store.look_call_count.borrow().get("vanished_lut"), Some(&1));
    }
}
