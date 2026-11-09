//! Automatic subject, people and sky masks (ED-23, RP-11d).
//!
//! Four models: BiRefNet for the main subject, BiRefNet portrait for people, UperNet ConvNeXt
//! tiny scene segmentation to guard sky detection, and U²-Net trained on skies. All are
//! MIT-licensed, downloaded once into `rollchrome_core::config::models_dir` and checked
//! against the SHA-256 recorded here before every use, so a truncated or substituted file
//! is refused rather than run.
//!
//! Inference runs on the CPU through ONNX Runtime. ED-22 found CoreML hung with its
//! defaults, could not compile BiRefNet, and ran it some thirty times slower on the GPU.
//! The session is built for one mask and dropped with it, with ONNX Runtime's arena off,
//! so the subject model's transient 10 GB is returned as soon as the mask is made.
//!
//! The models expect upright photographs — a sky model shown a sideways frame looks for
//! sky at the side — so the stored pixels are turned upright for inference and the mask
//! turned back, to be stored in the stored frame like every mask (`edit::masks`).
//!
//! Only `run_model` needs ONNX Runtime and sits behind the `segment` feature, which the
//! desktop application enables. Fetching, verifying, orienting and finishing a mask do
//! not, so `core`'s own tests cover them without the runtime (G2).

use super::edit::masks::{AutoTarget, ObjectClick};
use super::edit::raster::{StoredRaster, STORED_MASK_EDGE};
/// Re-exported so the desktop application can hold a photograph's pixels without linking
/// `image` itself.
pub use image::RgbImage;
use image::{imageops, GrayImage};
use phototools_core::error::Error;
use phototools_core::jobs::Progress;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// One model: where it comes from, what it must hash to, and what it expects.
#[derive(Debug, PartialEq)]
pub struct ModelSpec {
    pub target: AutoTarget,
    /// For the person: what is being downloaded, and its licence.
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    /// Square input side the model was exported for.
    pub side: u32,
}

pub const SUBJECT: ModelSpec = ModelSpec {
    target: AutoTarget::Subject,
    name: "BiRefNet (MIT), 973 MB",
    file: "birefnet_general.onnx",
    url: "https://huggingface.co/onnx-community/BiRefNet-ONNX/resolve/main/onnx/model.onnx",
    sha256: "58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c",
    bytes: 972_666_916,
    side: 1024,
};

pub const PEOPLE: ModelSpec = ModelSpec {
    target: AutoTarget::People,
    name: "BiRefNet portrait (MIT), 973 MB",
    file: "birefnet_portrait.onnx",
    url:
        "https://huggingface.co/onnx-community/BiRefNet-portrait-ONNX/resolve/main/onnx/model.onnx",
    sha256: "1ba1c8ff5a7bbfadc8d8d13fb11d7be793f91f23d9d466549e37a854f6668f99",
    bytes: 972_666_916,
    side: 1024,
};

pub const SKY_GUARD: ModelSpec = ModelSpec {
    target: AutoTarget::Sky,
    name: "UperNet ConvNeXt tiny (MIT), 238 MB",
    file: "sky_guard.onnx",
    url: "https://huggingface.co/JElvis/upernet-convnext-tiny-ade-onnx/resolve/main/model.onnx",
    sha256: "fea729ab6c5d338bc79ad3d986b073e6faa12c97255eafd78a1f7145326e534e",
    bytes: 238_434_293,
    side: 512,
};

pub const SKY: ModelSpec = ModelSpec {
    target: AutoTarget::Sky,
    name: "U²-Net sky segmentation (MIT), 176 MB",
    file: "skyseg.onnx",
    url: "https://huggingface.co/JianyuanWang/skyseg/resolve/main/skyseg.onnx",
    sha256: "ab9c34c64c3d821220a2886a4a06da4642ffa14d5b30e8d5339056a089aa1d39",
    bytes: 175_997_079,
    side: 320,
};

pub fn spec(target: AutoTarget) -> &'static ModelSpec {
    match target {
        AutoTarget::Subject => &SUBJECT,
        AutoTarget::People => &PEOPLE,
        AutoTarget::Sky => &SKY,
    }
}

pub fn required_models(target: AutoTarget) -> &'static [&'static ModelSpec] {
    match target {
        AutoTarget::Subject => &[&SUBJECT],
        AutoTarget::People => &[&PEOPLE],
        AutoTarget::Sky => &[&SKY_GUARD, &SKY],
    }
}

pub fn model_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file)
}

/// True when the model is on disk at its full size. The hash is checked when it is used.
pub fn model_present(dir: &Path, spec: &ModelSpec) -> bool {
    std::fs::metadata(model_path(dir, spec)).is_ok_and(|m| m.len() == spec.bytes)
}

/// True only when every required file for `target` is on disk at full size.
pub fn target_present(dir: &Path, target: AutoTarget) -> bool {
    required_models(target)
        .iter()
        .all(|spec| model_present(dir, spec))
}

/// Sum of byte sizes of models required for `target` that are not yet on disk at full size.
pub fn missing_bytes(dir: &Path, target: AutoTarget) -> u64 {
    required_models(target)
        .iter()
        .filter(|spec| !model_present(dir, spec))
        .map(|spec| spec.bytes)
        .sum()
}

/// Human-facing name of the model(s) required for `target`, naming only what is missing for Sky.
pub fn display_name(dir: &Path, target: AutoTarget) -> String {
    let missing: Vec<&ModelSpec> = required_models(target)
        .iter()
        .copied()
        .filter(|s| !model_present(dir, s))
        .collect();
    if missing.is_empty() {
        spec(target).name.to_string()
    } else {
        missing
            .iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

/// Downloads only the missing files for `target` into `dir`, reporting one running total.
pub fn download_target(
    dir: &Path,
    target: AutoTarget,
    progress: &dyn Progress,
) -> Result<Vec<PathBuf>, Error> {
    download_specs(dir, required_models(target), progress)
}

/// Downloads only the missing specs into `dir` with a single running byte total.
pub fn download_specs(
    dir: &Path,
    specs: &[&ModelSpec],
    progress: &dyn Progress,
) -> Result<Vec<PathBuf>, Error> {
    let missing: Vec<&ModelSpec> = specs
        .iter()
        .copied()
        .filter(|s| !model_present(dir, s))
        .collect();
    if missing.is_empty() {
        return Ok(specs.iter().map(|s| model_path(dir, s)).collect());
    }
    let total: u64 = missing.iter().map(|s| s.bytes).sum();
    let mut offset = 0u64;
    let mut downloaded = Vec::new();
    for spec in missing {
        let p = OffsetProgress {
            inner: progress,
            offset,
            total,
        };
        let path = fetch(
            spec.url,
            &model_path(dir, spec),
            spec.sha256,
            spec.bytes,
            &p,
        )?;
        offset += spec.bytes;
        downloaded.push(path);
    }
    Ok(specs.iter().map(|s| model_path(dir, s)).collect())
}

/// Downloads `spec` into `dir`, reporting bytes, stopping when cancelled, and keeping the
/// file only if it hashes to what is recorded.
pub fn download_model(
    dir: &Path,
    spec: &ModelSpec,
    progress: &dyn Progress,
) -> Result<PathBuf, Error> {
    fetch(
        spec.url,
        &model_path(dir, spec),
        spec.sha256,
        spec.bytes,
        progress,
    )
}

/// Fetches `url` to `dest` through a `.part` file, renamed into place only once its length
/// and SHA-256 match: an interrupted download never leaves something that looks like a
/// model.
pub fn fetch(
    url: &str,
    dest: &Path,
    sha256: &str,
    bytes: u64,
    progress: &dyn Progress,
) -> Result<PathBuf, Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("part");
    let result = (|| {
        let client = reqwest::blocking::Client::builder()
            .timeout(None)
            .build()
            .map_err(|e| Error::Internal(format!("could not start the download: {e}")))?;
        let mut response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| Error::Refused(format!("could not download the model: {e}")))?;
        let mut file = std::fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut done = 0u64;
        loop {
            if progress.cancelled() {
                return Err(Error::Refused("the download was cancelled".into()));
            }
            let n = response
                .read(&mut buf)
                .map_err(|e| Error::Refused(format!("the download was interrupted: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            done += n as u64;
            progress.report(done, bytes, "Downloading the model");
        }
        file.sync_all()?;
        let got = phototools_core::ingest::scanner::hex(&hasher.finalize());
        if done != bytes || got != sha256 {
            return Err(Error::Refused(format!(
                "the downloaded model is not the one expected ({done} bytes, SHA-256 {got}; \
                 expected {bytes} bytes, {sha256}), so it was discarded"
            )));
        }
        std::fs::rename(&part, dest)?;
        Ok(dest.to_path_buf())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Refuses a model file that does not hash to what is recorded.
pub fn verify_model(path: &Path, spec: &ModelSpec) -> Result<(), Error> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| Error::Refused(format!("the {} model is not downloaded ({e})", spec.name)))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = phototools_core::ingest::scanner::hex(&hasher.finalize());
    if got != spec.sha256 {
        return Err(Error::Refused(format!(
            "the {} model on disk is not the one expected (SHA-256 {got}); download it again",
            spec.name
        )));
    }
    Ok(())
}

/// Turns stored pixels upright for EXIF `orientation` (1–8).
pub fn to_upright<P: image::Pixel + 'static>(
    img: &image::ImageBuffer<P, Vec<P::Subpixel>>,
    orientation: u32,
) -> image::ImageBuffer<P, Vec<P::Subpixel>> {
    match orientation {
        2 => imageops::flip_horizontal(img),
        3 => imageops::rotate180(img),
        4 => imageops::flip_vertical(img),
        5 => imageops::flip_horizontal(&imageops::rotate90(img)),
        6 => imageops::rotate90(img),
        7 => imageops::flip_horizontal(&imageops::rotate270(img)),
        8 => imageops::rotate270(img),
        _ => img.clone(),
    }
}

/// The inverse of `to_upright`: an upright mask back into the stored frame.
pub fn from_upright<P: image::Pixel + 'static>(
    img: &image::ImageBuffer<P, Vec<P::Subpixel>>,
    orientation: u32,
) -> image::ImageBuffer<P, Vec<P::Subpixel>> {
    match orientation {
        // Mirrors and the two diagonal reflections are their own inverses.
        2 | 3 | 4 | 5 | 7 => to_upright(img, orientation),
        6 => imageops::rotate270(img),
        8 => imageops::rotate90(img),
        _ => img.clone(),
    }
}

/// An upright photograph as the model's input: squashed to `side` × `side` (how both models
/// were trained), scaled to [0, 1] and normalised by the ImageNet mean and deviation, NCHW.
pub fn prepare_input(upright: &RgbImage, side: u32) -> Vec<f32> {
    let small = imageops::resize(upright, side, side, imageops::FilterType::Triangle);
    let (mean, std) = ([0.485f32, 0.456, 0.406], [0.229f32, 0.224, 0.225]);
    let plane = (side * side) as usize;
    let mut out = vec![0.0f32; 3 * plane];
    for (i, p) in small.pixels().enumerate() {
        for c in 0..3 {
            out[c * plane + i] = (p[c] as f32 / 255.0 - mean[c]) / std[c];
        }
    }
    out
}

pub const SKY_GUARD_MIN_SHARE: f32 = 0.005;
pub const SKY_GUARD_CLASSES: usize = 150;
pub const SKY_GUARD_CLASS_SKY: usize = 2;
pub const SKY_GUARD_WIDTH: usize = 128;
pub const SKY_GUARD_HEIGHT: usize = 128;

/// True when at least `SKY_GUARD_MIN_SHARE` (0.005) of the scene model's cells are sky.
pub fn scene_has_sky(
    logits: &[f32],
    classes: usize,
    width: usize,
    height: usize,
) -> Result<bool, Error> {
    let cells = width * height;
    let expected = classes * cells;
    if logits.len() != expected || expected == 0 {
        return Err(Error::Internal(format!(
            "the sky guard model returned {} values, not {}",
            logits.len(),
            expected
        )));
    }
    if classes <= SKY_GUARD_CLASS_SKY {
        return Err(Error::Internal(
            "the sky guard model has no sky class".into(),
        ));
    }
    let mut sky_cells = 0usize;
    for cell in 0..cells {
        let mut best_class = 0usize;
        let mut best_val = logits[cell];
        for c in 1..classes {
            let val = logits[c * cells + cell];
            if val > best_val {
                best_val = val;
                best_class = c;
            }
        }
        if best_class == SKY_GUARD_CLASS_SKY {
            sky_cells += 1;
        }
    }
    let share = sky_cells as f32 / cells as f32;
    Ok(share >= SKY_GUARD_MIN_SHARE)
}

/// The model's raw output as a stored mask: to [0, 1] as each model's reference code does
/// (BiRefNet gives logits; the sky model a map normalised by its own range), back to the
/// photograph's proportions at most `STORED_MASK_EDGE` on the long edge, then into the
/// stored frame.
pub fn finish_output(
    raw: &[f32],
    spec: &ModelSpec,
    upright_w: u32,
    upright_h: u32,
    orientation: u32,
) -> Result<StoredRaster, Error> {
    let side = spec.side;
    if raw.len() != (side * side) as usize {
        return Err(Error::Internal(format!(
            "the {} model returned {} values, not {}",
            spec.name,
            raw.len(),
            side * side
        )));
    }
    let values: Vec<f32> = match spec.target {
        AutoTarget::Subject | AutoTarget::People => {
            raw.iter().map(|v| 1.0 / (1.0 + (-v).exp())).collect()
        }
        AutoTarget::Sky => {
            let (lo, hi) = raw
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            if hi - lo < 1e-6 {
                // A uniform map carries no information: no sky found.
                vec![0.0; raw.len()]
            } else {
                raw.iter().map(|v| (v - lo) / (hi - lo)).collect()
            }
        }
    };
    let square = GrayImage::from_fn(side, side, |x, y| {
        image::Luma([(values[(y * side + x) as usize] * 255.0 + 0.5) as u8])
    });
    let long = upright_w.max(upright_h).max(1) as f32;
    let k = (STORED_MASK_EDGE as f32 / long).min(1.0);
    let (w, h) = (
        ((upright_w as f32 * k).round() as u32).max(1),
        ((upright_h as f32 * k).round() as u32).max(1),
    );
    let upright = imageops::resize(&square, w, h, imageops::FilterType::Triangle);
    let stored = from_upright(&upright, orientation);
    StoredRaster::encode(
        stored.width(),
        stored.height(),
        stored.as_raw(),
        spec.sha256,
    )
}

/// Runs a model on a prepared input. The only code that needs ONNX Runtime.
#[cfg(feature = "segment")]
pub fn run_model(model_path: &Path, spec: &ModelSpec, input: Vec<f32>) -> Result<Vec<f32>, Error> {
    use ort::ep::CPU;
    use ort::session::Session;
    use ort::value::Tensor;

    let failed = |e: ort::Error| Error::Internal(format!("the {} model failed: {e}", spec.name));
    let mut session = Session::builder()
        .map_err(failed)?
        .with_memory_pattern(false)
        .map_err(|e| failed(e.into()))?
        .with_execution_providers([CPU::default().with_arena_allocator(false).build()])
        .map_err(|e| failed(e.into()))?
        .commit_from_file(model_path)
        .map_err(failed)?;
    let name = session.inputs()[0].name().to_string();
    let side = spec.side as usize;
    let tensor = Tensor::from_array(([1usize, 3, side, side], input)).map_err(failed)?;
    let outputs = session
        .run(ort::inputs![name.as_str() => tensor])
        .map_err(failed)?;
    let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(failed)?;
    Ok(data.to_vec())
}

/// The automatic mask of a photograph given as its stored pixels: verify the model, turn
/// the photograph upright, run, finish, store.
#[cfg(feature = "segment")]
pub fn auto_mask(
    stored: &RgbImage,
    orientation: u32,
    target: AutoTarget,
    models_dir: &Path,
) -> Result<StoredRaster, Error> {
    let upright = to_upright(stored, orientation);
    if target == AutoTarget::Sky {
        let guard_path = model_path(models_dir, &SKY_GUARD);
        let sky_path = model_path(models_dir, &SKY);
        verify_model(&guard_path, &SKY_GUARD)?;
        verify_model(&sky_path, &SKY)?;
        let guard_input = prepare_input(&upright, SKY_GUARD.side);
        let guard_raw = run_model(&guard_path, &SKY_GUARD, guard_input)?;
        if !scene_has_sky(
            &guard_raw,
            SKY_GUARD_CLASSES,
            SKY_GUARD_WIDTH,
            SKY_GUARD_HEIGHT,
        )? {
            return Err(Error::Refused("no sky was found in this photograph".into()));
        }
        let input = prepare_input(&upright, SKY.side);
        let raw = run_model(&sky_path, &SKY, input)?;
        finish_output(&raw, &SKY, upright.width(), upright.height(), orientation)
    } else {
        let spec = spec(target);
        let path = model_path(models_dir, spec);
        verify_model(&path, spec)?;
        let input = prepare_input(&upright, spec.side);
        let raw = run_model(&path, spec, input)?;
        finish_output(&raw, spec, upright.width(), upright.height(), orientation)
    }
}

/// One of the two files that make up the interactive object segmentation model (RP-11c).
#[derive(Debug, Clone, Copy)]
pub struct ObjectModelSpec {
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// Human-facing name and licence of the object segmentation model pair.
pub const OBJECT_MODEL_NAME: &str = "MobileSAM (Apache-2.0 / MIT), 45 MB";

pub const OBJECT_ENCODER: ObjectModelSpec = ObjectModelSpec {
    name: "MobileSAM image encoder (Apache-2.0 / MIT), 28 MB",
    file: "mobile_sam_image_encoder.onnx",
    url: "https://huggingface.co/Acly/MobileSAM/resolve/main/mobile_sam_image_encoder.onnx",
    sha256: "580f5fb648ea1062c0aabc26217aed56921985f03f0cbbd852bba81d760cc749",
    bytes: 28_157_093,
};

pub const OBJECT_DECODER: ObjectModelSpec = ObjectModelSpec {
    name: "MobileSAM mask decoder (Apache-2.0 / MIT), 16 MB",
    file: "sam_mask_decoder_multi.onnx",
    url: "https://huggingface.co/Acly/MobileSAM/resolve/main/sam_mask_decoder_multi.onnx",
    sha256: "8976b90a87ba50a6a72217a5ff994f7d25ce16f2229fcc1ed259e1294c622ffe",
    bytes: 16_496_559,
};

pub const OBJECT_MODEL_BYTES: u64 = OBJECT_ENCODER.bytes + OBJECT_DECODER.bytes;

pub const OBJECT_INPUT_EDGE: u32 = 1024;

pub fn object_model_path(dir: &Path, spec: &ObjectModelSpec) -> PathBuf {
    dir.join(spec.file)
}

/// True when both object model files are on disk at their full sizes.
pub fn object_model_present(dir: &Path) -> bool {
    std::fs::metadata(object_model_path(dir, &OBJECT_ENCODER))
        .is_ok_and(|m| m.len() == OBJECT_ENCODER.bytes)
        && std::fs::metadata(object_model_path(dir, &OBJECT_DECODER))
            .is_ok_and(|m| m.len() == OBJECT_DECODER.bytes)
}

/// Refuses an object model file that does not hash to what is recorded.
pub fn verify_object_model(path: &Path, spec: &ObjectModelSpec) -> Result<(), Error> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| Error::Refused(format!("the {} model is not downloaded ({e})", spec.name)))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = phototools_core::ingest::scanner::hex(&hasher.finalize());
    if got != spec.sha256 {
        return Err(Error::Refused(format!(
            "the {} model on disk is not the one expected (SHA-256 {got}); download it again",
            spec.name
        )));
    }
    Ok(())
}

struct OffsetProgress<'a> {
    inner: &'a dyn Progress,
    offset: u64,
    total: u64,
}

impl Progress for OffsetProgress<'_> {
    fn report(&self, done: u64, _total: u64, phase: &str) {
        self.inner.report(self.offset + done, self.total, phase);
    }

    fn cancelled(&self) -> bool {
        self.inner.cancelled()
    }
}

/// Downloads both object model files sequentially through `fetch`, reporting one running
/// total over both files' bytes, stopping on cancel, and keeping each file only if it
/// verifies. If the second fails, the first stays on disk.
pub fn download_object_model(
    dir: &Path,
    progress: &dyn Progress,
) -> Result<(PathBuf, PathBuf), Error> {
    download_object_specs(dir, &OBJECT_ENCODER, &OBJECT_DECODER, progress)
}

/// Downloads `encoder` then `decoder` into `dir` with a single running byte total.
pub fn download_object_specs(
    dir: &Path,
    encoder: &ObjectModelSpec,
    decoder: &ObjectModelSpec,
    progress: &dyn Progress,
) -> Result<(PathBuf, PathBuf), Error> {
    let total = encoder.bytes + decoder.bytes;
    let first_progress = OffsetProgress {
        inner: progress,
        offset: 0,
        total,
    };
    let enc_path = fetch(
        encoder.url,
        &object_model_path(dir, encoder),
        encoder.sha256,
        encoder.bytes,
        &first_progress,
    )?;
    let second_progress = OffsetProgress {
        inner: progress,
        offset: encoder.bytes,
        total,
    };
    let dec_path = fetch(
        decoder.url,
        &object_model_path(dir, decoder),
        decoder.sha256,
        decoder.bytes,
        &second_progress,
    )?;
    Ok((enc_path, dec_path))
}

/// Maps a stored-normalised click `(u, v)` to upright-normalised `(u, v)` for EXIF
/// `orientation` (1–8), matching `to_upright`.
pub fn upright_click(u: f32, v: f32, orientation: u32) -> (f32, f32) {
    match orientation {
        2 => (1.0 - u, v),
        3 => (1.0 - u, 1.0 - v),
        4 => (u, 1.0 - v),
        5 => (v, u),
        6 => (1.0 - v, u),
        7 => (1.0 - v, 1.0 - u),
        8 => (v, 1.0 - u),
        _ => (u, v),
    }
}

/// Prepared encoder input for MobileSAM: HWC, RGB, f32 in 0..=255, long edge 1024.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectInput {
    pub data: Vec<f32>,
    pub width: u32,
    pub height: u32,
}

/// Resizes an upright photograph with `FilterType::Triangle` so its long edge is 1024 and
/// returns HWC RGB f32 values in 0..=255 (neither padded nor normalised; the encoder graph
/// does both).
pub fn prepare_object_input(upright: &RgbImage) -> ObjectInput {
    let (uw, uh) = (upright.width().max(1), upright.height().max(1));
    let (width, height) = if uw >= uh {
        (
            OBJECT_INPUT_EDGE,
            ((uh as f64 * OBJECT_INPUT_EDGE as f64 / uw as f64).round() as u32).max(1),
        )
    } else {
        (
            ((uw as f64 * OBJECT_INPUT_EDGE as f64 / uh as f64).round() as u32).max(1),
            OBJECT_INPUT_EDGE,
        )
    };
    let resized = imageops::resize(upright, width, height, imageops::FilterType::Triangle);
    let data = resized.as_raw().iter().map(|&b| b as f32).collect();
    ObjectInput {
        data,
        width,
        height,
    }
}

/// Builds the decoder's `point_coords` and `point_labels` tensors (with the trailing
/// `(0, 0)` / `-1` sentinel) from stored-normalised clicks.
pub fn object_prompt(
    clicks: &[ObjectClick],
    orientation: u32,
    width: u32,
    height: u32,
) -> Result<(Vec<f32>, Vec<f32>), Error> {
    if clicks.is_empty() {
        return Err(Error::Refused("click on an object to select it".into()));
    }
    if clicks.len() > 32 {
        return Err(Error::Refused(
            "an object mask can take at most 32 clicks".into(),
        ));
    }
    for c in clicks {
        if !c.u.is_finite()
            || !c.v.is_finite()
            || !(0.0..=1.0).contains(&c.u)
            || !(0.0..=1.0).contains(&c.v)
        {
            return Err(Error::Refused(
                "the click was outside the photograph".into(),
            ));
        }
    }
    if !clicks.iter().any(|c| c.add) {
        return Err(Error::Refused(
            "click on the object to add before taking away".into(),
        ));
    }

    let mut coords = Vec::with_capacity((clicks.len() + 1) * 2);
    let mut labels = Vec::with_capacity(clicks.len() + 1);
    let w = width as f32;
    let h = height as f32;
    for c in clicks {
        let (u, v) = upright_click(c.u, c.v, orientation);
        coords.push(u * w);
        coords.push(v * h);
        labels.push(if c.add { 1.0 } else { 0.0 });
    }
    coords.push(0.0);
    coords.push(0.0);
    labels.push(-1.0);
    Ok((coords, labels))
}

/// Chooses which of the 4 candidate masks to use: for 1 click, the highest `iou` among
/// indices 1, 2, 3; for 2 or more clicks, index 0.
pub fn choose_object_mask(iou: &[f32], clicks: usize) -> usize {
    if clicks >= 2 {
        0
    } else {
        (1..4.min(iou.len()))
            .max_by(|&a, &b| iou[a].total_cmp(&iou[b]))
            .unwrap_or(0)
    }
}

/// Maps decoder mask logits at `(width, height)` through the logistic function to 0..=255,
/// turns the upright mask back into the stored frame, and encodes it with the decoder's
/// SHA-256. Refuses if no pixel reaches 128.
pub fn finish_object_mask(
    logits: &[f32],
    width: u32,
    height: u32,
    orientation: u32,
) -> Result<StoredRaster, Error> {
    if logits.len() != (width * height) as usize {
        return Err(Error::Internal(format!(
            "the {} model returned {} values, not {}",
            OBJECT_DECODER.name,
            logits.len(),
            width * height
        )));
    }
    let bytes: Vec<u8> = logits
        .iter()
        .map(|&v| {
            let s = 1.0 / (1.0 + (-v).exp());
            (s * 255.0 + 0.5) as u8
        })
        .collect();
    if !bytes.iter().any(|&b| b >= 128) {
        return Err(Error::Refused(
            "nothing was found there; click on the object itself".into(),
        ));
    }
    let Some(upright) = GrayImage::from_raw(width, height, bytes) else {
        return Err(Error::Internal(
            "could not build the object mask image".into(),
        ));
    };
    let stored = from_upright(&upright, orientation);
    StoredRaster::encode(
        stored.width(),
        stored.height(),
        stored.as_raw(),
        OBJECT_DECODER.sha256,
    )
}

/// Holds one photograph's MobileSAM image embedding and the decoder session so clicks run
/// in tens of milliseconds without re-encoding the photograph.
#[cfg(feature = "segment")]
pub struct ObjectSelector {
    decoder: ort::session::Session,
    embedding: Vec<f32>,
    width: u32,
    height: u32,
    orientation: u32,
}

#[cfg(feature = "segment")]
impl ObjectSelector {
    /// Verifies both files, runs the encoder once, and drops it: its memory is not held.
    pub fn new(stored: &RgbImage, orientation: u32, models_dir: &Path) -> Result<Self, Error> {
        use ort::ep::CPU;
        use ort::session::Session;
        use ort::value::Tensor;

        let enc_path = object_model_path(models_dir, &OBJECT_ENCODER);
        let dec_path = object_model_path(models_dir, &OBJECT_DECODER);
        verify_object_model(&enc_path, &OBJECT_ENCODER)?;
        verify_object_model(&dec_path, &OBJECT_DECODER)?;

        let upright = to_upright(stored, orientation);
        let ObjectInput {
            data,
            width,
            height,
        } = prepare_object_input(&upright);

        let embedding = {
            let failed = |e: ort::Error| {
                Error::Internal(format!("the {} model failed: {e}", OBJECT_ENCODER.name))
            };
            let mut encoder = Session::builder()
                .map_err(failed)?
                .with_memory_pattern(false)
                .map_err(|e| failed(e.into()))?
                .with_execution_providers([CPU::default().with_arena_allocator(false).build()])
                .map_err(|e| failed(e.into()))?
                .commit_from_file(&enc_path)
                .map_err(failed)?;
            let input_tensor =
                Tensor::from_array(([height as usize, width as usize, 3usize], data))
                    .map_err(failed)?;
            let outputs = encoder
                .run(ort::inputs!["input_image" => input_tensor])
                .map_err(failed)?;
            let (_, emb) = outputs["image_embeddings"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            emb.to_vec()
        };

        let dec_failed = |e: ort::Error| {
            Error::Internal(format!("the {} model failed: {e}", OBJECT_DECODER.name))
        };
        let decoder = Session::builder()
            .map_err(dec_failed)?
            .with_memory_pattern(false)
            .map_err(|e| dec_failed(e.into()))?
            .with_execution_providers([CPU::default().with_arena_allocator(false).build()])
            .map_err(|e| dec_failed(e.into()))?
            .commit_from_file(&dec_path)
            .map_err(dec_failed)?;

        Ok(Self {
            decoder,
            embedding,
            width,
            height,
            orientation,
        })
    }

    /// One decoder run. Tens of milliseconds, so the person can click freely.
    pub fn select(&mut self, clicks: &[ObjectClick]) -> Result<StoredRaster, Error> {
        use ort::value::Tensor;

        let (coords, labels) = object_prompt(clicks, self.orientation, self.width, self.height)?;
        let n_points = labels.len();

        let failed = |e: ort::Error| {
            Error::Internal(format!("the {} model failed: {e}", OBJECT_DECODER.name))
        };
        let emb_tensor =
            Tensor::from_array(([1usize, 256, 64, 64], self.embedding.clone())).map_err(failed)?;
        let coords_tensor = Tensor::from_array(([1usize, n_points, 2], coords)).map_err(failed)?;
        let labels_tensor = Tensor::from_array(([1usize, n_points], labels)).map_err(failed)?;
        let mask_input_tensor =
            Tensor::from_array(([1usize, 1, 256, 256], vec![0.0f32; 256 * 256])).map_err(failed)?;
        let has_mask_tensor = Tensor::from_array(([1usize], vec![0.0f32])).map_err(failed)?;
        let orig_size_tensor =
            Tensor::from_array(([2usize], vec![self.height as f32, self.width as f32]))
                .map_err(failed)?;

        let outputs = self
            .decoder
            .run(ort::inputs![
                "image_embeddings" => emb_tensor,
                "point_coords" => coords_tensor,
                "point_labels" => labels_tensor,
                "mask_input" => mask_input_tensor,
                "has_mask_input" => has_mask_tensor,
                "orig_im_size" => orig_size_tensor,
            ])
            .map_err(failed)?;

        let (_, iou) = outputs["iou_predictions"]
            .try_extract_tensor::<f32>()
            .map_err(failed)?;
        let (_, masks) = outputs["masks"]
            .try_extract_tensor::<f32>()
            .map_err(failed)?;

        let idx = choose_object_mask(iou, clicks.len());
        let plane = (self.width as usize) * (self.height as usize);
        let start = idx * plane;
        let end = start + plane;
        let Some(slice) = masks.get(start..end) else {
            return Err(Error::Internal(format!(
                "the {} model returned {} mask values, not at least {}",
                OBJECT_DECODER.name,
                masks.len(),
                end
            )));
        };
        finish_object_mask(slice, self.width, self.height, self.orientation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phototools_core::jobs::InMemoryProgress;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// Serves `body` once over HTTP on a local port and returns its URL.
    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 2 {
                    line.clear();
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{addr}/model.onnx")
    }

    fn sha(bytes: &[u8]) -> String {
        phototools_core::ingest::scanner::hex(&Sha256::digest(bytes))
    }

    #[test]
    fn a_model_whose_bytes_match_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let dest = dir.path().join("m.onnx");
        let url = serve_once(body.clone());
        let progress = InMemoryProgress::default();
        fetch(&url, &dest, &sha(&body), body.len() as u64, &progress).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!dest.with_extension("part").exists());
    }

    #[test]
    fn a_model_that_does_not_hash_as_recorded_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let body = vec![7u8; 4096];
        let dest = dir.path().join("m.onnx");
        let url = serve_once(body.clone());
        let err = fetch(
            &url,
            &dest,
            &sha(b"something else"),
            4096,
            &InMemoryProgress::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("not the one expected"),
            "got: {err}"
        );
        assert!(!dest.exists() && !dest.with_extension("part").exists());
    }

    #[test]
    fn a_cancelled_download_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let body = vec![1u8; 2_000_000];
        let dest = dir.path().join("m.onnx");
        let url = serve_once(body.clone());
        let progress = InMemoryProgress::default();
        progress.cancel();
        let err = fetch(&url, &dest, &sha(&body), body.len() as u64, &progress).unwrap_err();
        assert!(err.to_string().contains("cancelled"));
        assert!(!dest.exists() && !dest.with_extension("part").exists());
    }

    #[test]
    fn a_tampered_model_on_disk_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = model_path(dir.path(), &SKY);
        std::fs::write(&path, b"not the model").unwrap();
        let err = verify_model(&path, &SKY).unwrap_err();
        assert!(
            err.to_string().contains("not the one expected"),
            "got: {err}"
        );
        assert!(
            !model_present(dir.path(), &SKY),
            "a wrong-sized file is not present"
        );
    }

    /// Every EXIF orientation round-trips, and a marker in the stored frame lands where the
    /// geometry engine (ED-13) says the upright frame shows it.
    #[test]
    fn orientation_round_trips_and_agrees_with_the_geometry() {
        use crate::media::edit::geometry::Geometry;
        // The geometry refuses frames under 16 px on a side.
        let (w, h) = (30u32, 20u32);
        let mut stored = GrayImage::new(w, h);
        stored.put_pixel(5, 2, image::Luma([255]));
        for o in 1..=8 {
            let up = to_upright(&stored, o);
            assert_eq!(from_upright(&up, o), stored, "orientation {o}");
            let plan = Geometry::default().plan(w, h, o).unwrap();
            let (mx, my) = up
                .enumerate_pixels()
                .find(|(_, _, p)| p[0] == 255)
                .map(|(x, y, _)| (x, y))
                .unwrap();
            let (sx, sy) = plan.map(mx as f32 + 0.5, my as f32 + 0.5);
            assert_eq!(
                (sx.floor() as u32, sy.floor() as u32),
                (5, 2),
                "orientation {o}"
            );
        }
    }

    #[test]
    fn the_input_is_normalised_nchw_at_the_model_side() {
        let img = RgbImage::from_pixel(50, 30, image::Rgb([255, 0, 128]));
        let input = prepare_input(&img, 8);
        assert_eq!(input.len(), 3 * 64);
        assert!((input[0] - (1.0 - 0.485) / 0.229).abs() < 1e-5);
        assert!((input[64] - (0.0 - 0.456) / 0.224).abs() < 1e-5);
    }

    #[test]
    fn output_is_finished_into_the_stored_frame_at_most_1024_on_the_long_edge() {
        // A sky model output: high at the top of the upright frame.
        let side = SKY.side as usize;
        let raw: Vec<f32> = (0..side * side)
            .map(|i| if i / side < side / 2 { 5.0 } else { -5.0 })
            .collect();
        // Orientation 6: the upright frame is the stored one turned a quarter clockwise,
        // so the upright top is the stored left.
        let stored = finish_output(&raw, &SKY, 2000, 3000, 6).unwrap();
        assert_eq!((stored.width, stored.height), (1024, 683));
        let r = stored.decode().unwrap();
        assert!(r.sample(0.1, 0.5) > 0.99, "sky at the stored left");
        assert!(r.sample(0.9, 0.5) < 0.01, "none at the stored right");
        assert_eq!(stored.model_sha256, SKY.sha256);

        let flat = vec![0.3f32; side * side];
        let none = finish_output(&flat, &SKY, 100, 100, 1)
            .unwrap()
            .decode()
            .unwrap();
        assert!(
            none.data.iter().all(|&v| v == 0),
            "a uniform map finds no sky"
        );
    }

    fn leak_str(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn test_object_spec(
        file: &'static str,
        url: String,
        sha256: String,
        bytes: u64,
    ) -> ObjectModelSpec {
        ObjectModelSpec {
            name: "test object model",
            file,
            url: leak_str(url),
            sha256: leak_str(sha256),
            bytes,
        }
    }

    #[test]
    fn object_model_present_requires_both_files_at_their_exact_byte_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let enc_path = object_model_path(dir.path(), &OBJECT_ENCODER);
        let dec_path = object_model_path(dir.path(), &OBJECT_DECODER);

        assert!(!object_model_present(dir.path()));

        let enc_file = std::fs::File::create(&enc_path).unwrap();
        enc_file.set_len(OBJECT_ENCODER.bytes).unwrap();
        assert!(
            !object_model_present(dir.path()),
            "encoder alone is not present"
        );

        let dec_file = std::fs::File::create(&dec_path).unwrap();
        dec_file.set_len(OBJECT_DECODER.bytes - 1).unwrap();
        assert!(
            !object_model_present(dir.path()),
            "wrong decoder size is not present"
        );

        dec_file.set_len(OBJECT_DECODER.bytes).unwrap();
        assert!(object_model_present(dir.path()));
    }

    #[test]
    fn download_object_model_keeps_both_files_and_reports_one_running_total() {
        let dir = tempfile::tempdir().unwrap();
        let enc_body: Vec<u8> = (0..120_000u32).map(|i| (i % 251) as u8).collect();
        let dec_body: Vec<u8> = (0..80_000u32).map(|i| ((i * 3) % 251) as u8).collect();
        let enc_spec = test_object_spec(
            OBJECT_ENCODER.file,
            serve_once(enc_body.clone()),
            sha(&enc_body),
            enc_body.len() as u64,
        );
        let dec_spec = test_object_spec(
            OBJECT_DECODER.file,
            serve_once(dec_body.clone()),
            sha(&dec_body),
            dec_body.len() as u64,
        );
        #[derive(Default)]
        struct CollectingProgress {
            events: std::sync::Mutex<Vec<(u64, u64)>>,
        }
        impl phototools_core::jobs::Progress for CollectingProgress {
            fn report(&self, done: u64, total: u64, _msg: &str) {
                self.events.lock().unwrap().push((done, total));
            }
            fn cancelled(&self) -> bool {
                false
            }
        }
        let progress = CollectingProgress::default();
        let (enc_path, dec_path) =
            download_object_specs(dir.path(), &enc_spec, &dec_spec, &progress).unwrap();
        assert_eq!(std::fs::read(&enc_path).unwrap(), enc_body);
        assert_eq!(std::fs::read(&dec_path).unwrap(), dec_body);
        let events = progress.events.lock().unwrap().clone();
        assert!(!events.is_empty());
        let sum = (enc_body.len() + dec_body.len()) as u64;
        for &(_done, total) in &events {
            assert_eq!(total, sum);
        }
        assert_eq!(events.last().unwrap().0, sum);
    }

    #[test]
    fn download_object_model_keeps_the_first_file_when_the_second_fails() {
        let dir = tempfile::tempdir().unwrap();
        let enc_body: Vec<u8> = (0..50_000u32).map(|i| (i % 239) as u8).collect();
        let dec_body = vec![9u8; 40_000];
        let enc_spec = test_object_spec(
            OBJECT_ENCODER.file,
            serve_once(enc_body.clone()),
            sha(&enc_body),
            enc_body.len() as u64,
        );
        let dec_spec = test_object_spec(
            OBJECT_DECODER.file,
            serve_once(dec_body.clone()),
            sha(b"wrong decoder hash"),
            dec_body.len() as u64,
        );
        let progress = InMemoryProgress::default();
        let err = download_object_specs(dir.path(), &enc_spec, &dec_spec, &progress).unwrap_err();
        assert!(err.to_string().contains("not the one expected"));
        let enc_dest = object_model_path(dir.path(), &OBJECT_ENCODER);
        let dec_dest = object_model_path(dir.path(), &OBJECT_DECODER);
        assert_eq!(std::fs::read(&enc_dest).unwrap(), enc_body);
        assert!(!dec_dest.exists());
        assert!(!dec_dest.with_extension("part").exists());
    }

    #[test]
    fn scene_has_sky_checks_threshold_rankings_and_length() {
        let classes = 150;
        let (w, h) = (10, 100);
        let cells = w * h; // 1000 cells

        // 1. A different class winning everywhere is false
        let mut logits = vec![0.0f32; classes * cells];
        // Class 0 gets 10.0 everywhere
        for val in logits.iter_mut().take(cells) {
            *val = 10.0;
        }
        assert!(!scene_has_sky(&logits, classes, w, h).unwrap());

        // 2. A cell where sky (class 2) is second-highest is not sky
        // Set sky to 9.0 in all cells, but class 0 is 10.0
        for cell in 0..cells {
            logits[2 * cells + cell] = 9.0;
        }
        assert!(!scene_has_sky(&logits, classes, w, h).unwrap());

        // 3. Just under 0.5% (4 cells out of 1000 = 0.004 < 0.005)
        for cell in 0..4 {
            logits[2 * cells + cell] = 20.0;
        }
        assert!(!scene_has_sky(&logits, classes, w, h).unwrap());

        // 4. Just at 0.5% (5 cells out of 1000 = 0.005 >= 0.005)
        logits[2 * cells + 4] = 20.0;
        assert!(scene_has_sky(&logits, classes, w, h).unwrap());

        // 5. Wrong length is an error
        let wrong = vec![0.0f32; classes * cells - 1];
        let err = scene_has_sky(&wrong, classes, w, h).unwrap_err();
        assert!(matches!(err, Error::Internal(_)));
    }

    #[test]
    fn required_models_and_target_present_for_each_target() {
        assert_eq!(required_models(AutoTarget::Subject), &[&SUBJECT]);
        assert_eq!(required_models(AutoTarget::People), &[&PEOPLE]);
        assert_eq!(required_models(AutoTarget::Sky), &[&SKY_GUARD, &SKY]);

        let dir = tempfile::tempdir().unwrap();
        assert!(!target_present(dir.path(), AutoTarget::Sky));
        assert_eq!(
            missing_bytes(dir.path(), AutoTarget::Sky),
            SKY_GUARD.bytes + SKY.bytes
        );
        assert_eq!(
            display_name(dir.path(), AutoTarget::Sky),
            format!("{} + {}", SKY_GUARD.name, SKY.name)
        );

        // Only SKY present: target_present false, missing_bytes is SKY_GUARD.bytes, display_name is SKY_GUARD.name
        let sky_file = dir.path().join(SKY.file);
        std::fs::File::create(&sky_file)
            .unwrap()
            .set_len(SKY.bytes)
            .unwrap();
        assert!(
            !target_present(dir.path(), AutoTarget::Sky),
            "sky guard is missing"
        );
        assert_eq!(missing_bytes(dir.path(), AutoTarget::Sky), SKY_GUARD.bytes);
        assert_eq!(display_name(dir.path(), AutoTarget::Sky), SKY_GUARD.name);

        // Only SKY_GUARD present: target_present false
        std::fs::remove_file(&sky_file).unwrap();
        let guard_file = dir.path().join(SKY_GUARD.file);
        std::fs::File::create(&guard_file)
            .unwrap()
            .set_len(SKY_GUARD.bytes)
            .unwrap();
        assert!(
            !target_present(dir.path(), AutoTarget::Sky),
            "skyseg is missing"
        );
        assert_eq!(missing_bytes(dir.path(), AutoTarget::Sky), SKY.bytes);
        assert_eq!(display_name(dir.path(), AutoTarget::Sky), SKY.name);

        // Both present: target_present true, missing_bytes 0
        std::fs::File::create(&sky_file)
            .unwrap()
            .set_len(SKY.bytes)
            .unwrap();
        assert!(target_present(dir.path(), AutoTarget::Sky));
        assert_eq!(missing_bytes(dir.path(), AutoTarget::Sky), 0);
        assert_eq!(display_name(dir.path(), AutoTarget::Sky), SKY.name);
    }

    fn test_spec(file: &'static str, url: String, sha256: String, bytes: u64) -> ModelSpec {
        ModelSpec {
            target: AutoTarget::Sky,
            name: "test model",
            file,
            url: leak_str(url),
            sha256: leak_str(sha256),
            bytes,
            side: 512,
        }
    }

    #[test]
    fn download_target_requests_only_missing_files_and_keeps_first_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let body1: Vec<u8> = (0..60_000u32).map(|i| (i % 251) as u8).collect();
        let body2: Vec<u8> = (0..40_000u32).map(|i| ((i * 5) % 251) as u8).collect();

        // 1. One file already present at full size: only the other is requested
        let spec1 = test_spec(
            "m1.onnx",
            "http://127.0.0.1:9/unreachable".into(),
            sha(&body1),
            body1.len() as u64,
        );
        std::fs::write(model_path(dir.path(), &spec1), &body1).unwrap();

        let spec2 = test_spec(
            "m2.onnx",
            serve_once(body2.clone()),
            sha(&body2),
            body2.len() as u64,
        );

        #[derive(Default)]
        struct CollectingProgress {
            events: std::sync::Mutex<Vec<(u64, u64)>>,
        }
        impl phototools_core::jobs::Progress for CollectingProgress {
            fn report(&self, done: u64, total: u64, _msg: &str) {
                self.events.lock().unwrap().push((done, total));
            }
            fn cancelled(&self) -> bool {
                false
            }
        }
        let progress = CollectingProgress::default();
        let paths = download_specs(dir.path(), &[&spec1, &spec2], &progress).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            std::fs::read(model_path(dir.path(), &spec2)).unwrap(),
            body2
        );

        let events = progress.events.lock().unwrap().clone();
        assert!(!events.is_empty());
        for &(_done, total) in &events {
            // Total should only be spec2's bytes!
            assert_eq!(total, body2.len() as u64);
        }

        // 2. A bad second file leaves the first
        let dir2 = tempfile::tempdir().unwrap();
        let spec1_fresh = test_spec(
            "m1.onnx",
            serve_once(body1.clone()),
            sha(&body1),
            body1.len() as u64,
        );
        let spec2_bad = test_spec(
            "m2.onnx",
            serve_once(body2.clone()),
            sha(b"invalid hash"),
            body2.len() as u64,
        );
        let err = download_specs(
            dir2.path(),
            &[&spec1_fresh, &spec2_bad],
            &InMemoryProgress::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not the one expected"));
        assert_eq!(
            std::fs::read(model_path(dir2.path(), &spec1_fresh)).unwrap(),
            body1
        );
        assert!(!model_path(dir2.path(), &spec2_bad).exists());
        assert!(!model_path(dir2.path(), &spec2_bad)
            .with_extension("part")
            .exists());

        // 3. Cancel leaves no .part
        let dir3 = tempfile::tempdir().unwrap();
        let spec_cancel = test_spec(
            "m_cancel.onnx",
            serve_once(vec![3u8; 1_000_000]),
            sha(&vec![3u8; 1_000_000]),
            1_000_000,
        );
        let prog_cancel = InMemoryProgress::default();
        prog_cancel.cancel();
        let err = download_specs(dir3.path(), &[&spec_cancel], &prog_cancel).unwrap_err();
        assert!(err.to_string().contains("cancelled"));
        assert!(!model_path(dir3.path(), &spec_cancel).exists());
        assert!(!model_path(dir3.path(), &spec_cancel)
            .with_extension("part")
            .exists());
    }

    #[test]
    fn people_target_roundtrips_and_saved_masks_render_identically() {
        use crate::media::edit::masks::{LocalAdjustments, Mask, MaskKind};
        use crate::media::edit::{AdjustmentRecipe, PreviewSession};
        use crate::media::PreviewStage;

        // AutoTarget::People roundtrips as "people"
        let serialized = serde_json::to_string(&AutoTarget::People).unwrap();
        assert_eq!(serialized, "\"people\"");
        let deserialized: AutoTarget = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, AutoTarget::People);

        // A saved Subject and Sky mask render the same bytes without needing models
        let (w, h) = (64u32, 48u32);
        let img = RgbImage::from_pixel(w, h, image::Rgb([100, 150, 200]));
        let dir = tempfile::tempdir().unwrap();
        let img_path = dir.path().join("frame.jpg");
        img.save(&img_path).unwrap();

        let mask_data: Vec<u8> = (0..w * h)
            .map(|i| if i % 2 == 0 { 255 } else { 0 })
            .collect();
        let raster_sub = StoredRaster::encode(w, h, &mask_data, "sub-sha").unwrap();
        let raster_sky = StoredRaster::encode(w, h, &mask_data, "sky-sha").unwrap();

        let mask_sub = Mask {
            id: "m_sub".into(),
            name: "Subject".into(),
            kind: MaskKind::Auto {
                target: AutoTarget::Subject,
                mask: raster_sub,
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure: 1.0,
                ..Default::default()
            },
            strokes: Vec::new(),
            components: Vec::new(),
            detail: Default::default(),
        };

        let mask_sky = Mask {
            id: "m_sky".into(),
            name: "Sky".into(),
            kind: MaskKind::Auto {
                target: AutoTarget::Sky,
                mask: raster_sky,
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure: -1.0,
                ..Default::default()
            },
            strokes: Vec::new(),
            components: Vec::new(),
            detail: Default::default(),
        };

        let recipe = AdjustmentRecipe {
            masks: vec![mask_sub, mask_sky],
            ..Default::default()
        };

        let session = PreviewSession::open(&img_path).unwrap();
        let rendered1 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
        let rendered2 = session.render(&recipe, None, PreviewStage::Drag).unwrap();
        assert_eq!(rendered1.bytes, rendered2.bytes);
        assert!(!rendered1.bytes.is_empty());
    }

    #[test]
    fn model_spec_constants_have_exact_bytes_and_hashes() {
        assert_eq!(SUBJECT.bytes, 972_666_916);
        assert_eq!(
            SUBJECT.sha256,
            "58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c"
        );
        assert_eq!(SUBJECT.file, "birefnet_general.onnx");
        assert_eq!(SUBJECT.side, 1024);

        assert_eq!(PEOPLE.bytes, 972_666_916);
        assert_eq!(
            PEOPLE.sha256,
            "1ba1c8ff5a7bbfadc8d8d13fb11d7be793f91f23d9d466549e37a854f6668f99"
        );
        assert_eq!(PEOPLE.file, "birefnet_portrait.onnx");
        assert_eq!(PEOPLE.side, 1024);

        assert_eq!(SKY_GUARD.bytes, 238_434_293);
        assert_eq!(
            SKY_GUARD.sha256,
            "fea729ab6c5d338bc79ad3d986b073e6faa12c97255eafd78a1f7145326e534e"
        );
        assert_eq!(SKY_GUARD.file, "sky_guard.onnx");
        assert_eq!(SKY_GUARD.side, 512);

        assert_eq!(SKY.bytes, 175_997_079);
        assert_eq!(
            SKY.sha256,
            "ab9c34c64c3d821220a2886a4a06da4642ffa14d5b30e8d5339056a089aa1d39"
        );
        assert_eq!(SKY.file, "skyseg.onnx");
        assert_eq!(SKY.side, 320);
    }

    #[test]
    #[ignore]
    #[cfg(feature = "segment")]
    fn the_real_models_find_people_subject_and_only_real_sky() {
        use std::time::Instant;

        let models_dir_str = std::env::var("ROLLCHROME_MASK_MODELS")
            .expect("ROLLCHROME_MASK_MODELS must be set to the folder containing model files");
        let photos_dir_str = std::env::var("ROLLCHROME_MASK_PHOTOS")
            .expect("ROLLCHROME_MASK_PHOTOS must be set to the folder containing test photographs");

        let models_dir = Path::new(&models_dir_str);
        let photos_dir = Path::new(&photos_dir_str);

        let load_photo = |name: &str| -> (RgbImage, u32) {
            let path = photos_dir.join(name);
            let orientation = phototools_core::media::read_meta(&path)
                .map(|m| m.orientation as u32)
                .unwrap_or(1);
            let decoded = crate::media::edit::decode_image(&path)
                .unwrap_or_else(|e| panic!("failed to decode {name}: {e}"));
            let rgb = match decoded {
                crate::media::edit::ImageBuffer::Rgb8 {
                    width,
                    height,
                    data,
                } => RgbImage::from_raw(width, height, data).unwrap(),
                _ => {
                    let rgb16 = decoded.as_rgb16().unwrap();
                    let dyn_img = image::DynamicImage::ImageRgb16(
                        image::ImageBuffer::from_raw(
                            decoded.width(),
                            decoded.height(),
                            rgb16.to_vec(),
                        )
                        .unwrap(),
                    );
                    dyn_img.to_rgb8()
                }
            };
            (rgb, orientation)
        };

        let coverage = |raster: &StoredRaster| -> f64 {
            let cov = raster.decode().unwrap();
            let sum: u64 = cov.data.iter().map(|&b| b as u64).sum();
            sum as f64 / (cov.data.len() as f64 * 255.0)
        };

        // 1. sky.jpg: Sky covers more than 40%.
        let (sky_img, sky_orient) = load_photo("sky.jpg");
        let t0 = Instant::now();
        let sky_raster = auto_mask(&sky_img, sky_orient, AutoTarget::Sky, models_dir).unwrap();
        let sky_elapsed = t0.elapsed();
        let sky_cov = coverage(&sky_raster);
        println!(
            "sky.jpg Sky: {:.2}s, coverage: {:.1}%",
            sky_elapsed.as_secs_f64(),
            sky_cov * 100.0
        );
        assert!(
            sky_cov > 0.40,
            "sky.jpg: Sky coverage must be > 40%, got {:.1}%",
            sky_cov * 100.0
        );

        // 2. nosky.jpg: Sky is refused with "no sky was found in this photograph".
        let (nosky_img, nosky_orient) = load_photo("nosky.jpg");
        let t0 = Instant::now();
        let nosky_err =
            auto_mask(&nosky_img, nosky_orient, AutoTarget::Sky, models_dir).unwrap_err();
        let nosky_elapsed = t0.elapsed();
        println!(
            "nosky.jpg Sky: {:.2}s, refused: {nosky_err}",
            nosky_elapsed.as_secs_f64()
        );
        assert_eq!(nosky_err.to_string(), "no sky was found in this photograph");

        // 3. person.jpg: People covers between 5% and 70%, and Subject covers at least as much as 5%.
        let (person_img, person_orient) = load_photo("person.jpg");
        let t0 = Instant::now();
        let people_raster =
            auto_mask(&person_img, person_orient, AutoTarget::People, models_dir).unwrap();
        let people_elapsed = t0.elapsed();
        let people_cov = coverage(&people_raster);
        println!(
            "person.jpg People: {:.2}s, coverage: {:.1}%",
            people_elapsed.as_secs_f64(),
            people_cov * 100.0
        );
        assert!(
            (0.05..=0.70).contains(&people_cov),
            "person.jpg: People coverage must be between 5% and 70%, got {:.1}%",
            people_cov * 100.0
        );

        let t1 = Instant::now();
        let subject_raster =
            auto_mask(&person_img, person_orient, AutoTarget::Subject, models_dir).unwrap();
        let subject_elapsed = t1.elapsed();
        let subject_cov = coverage(&subject_raster);
        println!(
            "person.jpg Subject: {:.2}s, coverage: {:.1}%",
            subject_elapsed.as_secs_f64(),
            subject_cov * 100.0
        );
        assert!(
            subject_cov >= 0.05,
            "person.jpg: Subject coverage must be at least 5%, got {:.1}%",
            subject_cov * 100.0
        );
    }
}
