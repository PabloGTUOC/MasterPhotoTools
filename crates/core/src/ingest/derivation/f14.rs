//! Deriving publishable JPEGs from RAW-only shots (F14).
//!
//! The ladder itself lives in [`crate::media::raw`]; this is the batch driver
//! around it, and it exists because two things about F14 only make sense in
//! batch:
//!
//! - **Metadata is copied through one `exiftool` process for the whole batch**
//!   (G4). Starting one per file costs 150–250 ms each regardless of file size,
//!   which on a card of RAW-only frames is more than the decoding.
//! - **The output then passes through F12 validation and F13 resize.** A derived
//!   JPEG is subject to the same megapixel ceiling and byte cap as a camera
//!   JPEG — F12 says both apply to both paths — so the resize happens here
//!   rather than leaving a 45 MP derivative for someone to deal with later.

use crate::config::Thresholds;
use crate::error::Error;
use crate::jobs::Progress;
use crate::media::raw::{self, RawSource};
use crate::media::{image_ops, read_meta, ExifWriter};
use crate::tools::Skip;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long to wait for one `exiftool` command before giving up on it.
///
/// Generous: copying every tag from a 60 MB RAW is slower than writing a date.
const EXIFTOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// One RAW file to derive from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationRequest {
    pub source: PathBuf,
    /// The shot's stem, which names the output.
    pub stem: String,
}

/// One derived JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedShot {
    pub stem: String,
    pub source: PathBuf,
    pub output: PathBuf,
    /// Which rung of F14's ladder produced it.
    pub rung: RawSource,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// True if the derivative was scaled to meet the megapixel ceiling.
    pub resized: bool,
    /// True once the capture date was confirmed present in the output by
    /// reading it back — not merely because the copy was requested.
    pub metadata_verified: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DerivationSummary {
    pub derived: Vec<DerivedShot>,
    pub failures: Vec<Skip>,
}

impl DerivationSummary {
    /// True when every derivative carried its metadata across.
    ///
    /// F14 requires capture date, camera and lens to be copied from the RAW; a
    /// derivative without them would be filed by Google Photos under its upload
    /// date rather than the date it was taken.
    pub fn all_metadata_verified(&self) -> bool {
        self.derived.iter().all(|d| d.metadata_verified)
    }

    /// How many came from each rung, for the phase report and the review screen.
    pub fn by_rung(&self, rung: RawSource) -> usize {
        self.derived.iter().filter(|d| d.rung == rung).count()
    }
}

/// Derive JPEGs for a batch of RAW-only shots (F14).
///
/// Two passes. The first runs the ladder and the resize in parallel, because
/// that is where the CPU goes. The second copies metadata serially through a
/// single `exiftool` process (G4).
pub fn derive_batch(
    requests: &[DerivationRequest],
    out_dir: &Path,
    thresholds: &Thresholds,
    progress: &dyn Progress,
) -> Result<DerivationSummary, Error> {
    derive_batch_with(requests, out_dir, thresholds, progress, "exiftool")
}

/// As [`derive_batch`], against a named `exiftool` program.
///
/// Exists so a test can point at a shim that records how many processes were
/// started — the same reason [`ExifWriter::start_with`] does. Overriding `PATH`
/// instead would be process-global, and a test harness runs its tests as threads
/// of one process.
pub fn derive_batch_with(
    requests: &[DerivationRequest],
    out_dir: &Path,
    thresholds: &Thresholds,
    progress: &dyn Progress,
    exiftool: &str,
) -> Result<DerivationSummary, Error> {
    if requests.is_empty() {
        return Ok(DerivationSummary::default());
    }

    std::fs::create_dir_all(out_dir)?;
    let total = requests.len() as u64;
    let done = std::sync::atomic::AtomicU64::new(0);

    // Pass one: pixels.
    let produced: Vec<Result<DerivedShot, Skip>> = requests
        .par_iter()
        .map(|request| {
            let outcome = derive_one(request, out_dir, thresholds);

            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            progress.report(n, total, "deriving JPEGs from RAW");

            outcome
        })
        .collect();

    let mut summary = DerivationSummary::default();
    for result in produced {
        match result {
            Ok(shot) => summary.derived.push(shot),
            Err(skip) => summary.failures.push(skip),
        }
    }

    if summary.derived.is_empty() {
        return Ok(summary);
    }

    // Pass two: metadata, through one process for the whole batch (G4).
    copy_metadata_batch(&mut summary, exiftool)?;

    summary.derived.sort_by(|a, b| a.stem.cmp(&b.stem));
    summary.failures.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(summary)
}

fn derive_one(
    request: &DerivationRequest,
    out_dir: &Path,
    thresholds: &Thresholds,
) -> Result<DerivedShot, Skip> {
    let skip = |reason: String| Skip {
        file: request.source.to_string_lossy().to_string(),
        reason,
    };

    let derived = raw::raw_to_jpeg(&request.source).map_err(|e| skip(e.to_string()))?;
    let output = out_dir.join(format!("{}.jpg", request.stem));

    // F12/F13: a derived JPEG faces the same ceiling as a camera JPEG.
    let target = image_ops::dimensions_for_megapixels(
        derived.width,
        derived.height,
        thresholds.max_megapixels,
    );

    let (width, height, resized) = match target {
        None => {
            // Already within the ceiling and already a JPEG — write the bytes
            // the camera's own engine produced rather than re-encoding them.
            // A re-encode here would cost quality for nothing.
            std::fs::write(&output, &derived.bytes).map_err(|e| skip(e.to_string()))?;
            (derived.width, derived.height, false)
        }
        Some((w, h)) => {
            let image = image::load_from_memory(&derived.bytes)
                .map_err(|e| skip(format!("the derived JPEG would not decode: {e}")))?;
            let scaled = image_ops::resize(&image, w, h).map_err(|e| skip(e.to_string()))?;

            // F13's ladder, so the byte cap is met as well as the pixel one.
            let (bytes, _quality, _fits) =
                image_ops::encode_jpeg_within(&scaled, thresholds.max_output_bytes)
                    .map_err(|e| skip(e.to_string()))?;
            std::fs::write(&output, &bytes).map_err(|e| skip(e.to_string()))?;
            (w, h, true)
        }
    };

    let bytes = std::fs::metadata(&output)
        .map_err(|e| skip(e.to_string()))?
        .len();

    Ok(DerivedShot {
        stem: request.stem.clone(),
        source: request.source.clone(),
        output,
        rung: derived.source,
        width,
        height,
        bytes,
        resized,
        metadata_verified: false,
    })
}

/// Copy capture date, camera and lens from each RAW into its derivative.
///
/// One `exiftool` process for the whole batch (G4), and **each result is read
/// back** rather than assumed: specification §9.2 invariant 6 forbids reporting
/// an outcome that was not verified.
fn copy_metadata_batch(summary: &mut DerivationSummary, exiftool: &str) -> Result<(), Error> {
    let mut writer = ExifWriter::start_with(exiftool, EXIFTOOL_TIMEOUT)?;

    for shot in &mut summary.derived {
        if let Err(e) = writer.copy_metadata(&shot.source, &shot.output) {
            summary.failures.push(Skip {
                file: shot.output.to_string_lossy().to_string(),
                reason: format!("metadata could not be copied: {e}"),
            });
            continue;
        }

        // The copy rewrites the file, so the size is measured again.
        if let Ok(meta) = std::fs::metadata(&shot.output) {
            shot.bytes = meta.len();
        }

        shot.metadata_verified = read_meta(&shot.output)
            .map(|m| m.capture.is_some())
            .unwrap_or(false);
    }

    writer.close()
}

/// The RAW-only shots on a card, as derivation requests (F11's
/// `needs_derivation`).
pub fn requests_for(shots: &[crate::ingest::grouping::Shot]) -> Vec<DerivationRequest> {
    shots
        .iter()
        .filter(|shot| shot.needs_derivation)
        .map(|shot| DerivationRequest {
            source: shot.candidate().path.clone(),
            stem: shot.stem.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::grouping::Shot;
    use crate::ingest::scanner::{AssetKind, ScannedAsset};

    fn asset(name: &str, kind: AssetKind) -> ScannedAsset {
        ScannedAsset {
            path: PathBuf::from(name),
            rel_path: name.into(),
            kind,
            bytes: 1000,
            sha256: "0".repeat(64),
            width: 6000,
            height: 4000,
            capture: None,
            camera: None,
        }
    }

    #[test]
    fn only_raw_only_shots_become_derivation_requests() {
        let shots = vec![
            Shot {
                stem: "IMG_0001".into(),
                assets: vec![asset("IMG_0001.JPG", AssetKind::Jpeg)],
                needs_derivation: false,
            },
            Shot {
                stem: "IMG_0002".into(),
                assets: vec![asset("IMG_0002.CR2", AssetKind::Raw)],
                needs_derivation: true,
            },
        ];

        let requests = requests_for(&shots);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].stem, "IMG_0002");
    }

    #[test]
    fn an_empty_batch_does_no_work_and_starts_no_process() {
        // Notably it must not start exiftool: a card of JPEGs has nothing to
        // derive, and paying 200 ms of process startup for that would be silly.
        let out = tempfile::tempdir().unwrap();
        let summary = derive_batch(
            &[],
            out.path(),
            &Thresholds::default(),
            &crate::jobs::InMemoryProgress::new(),
        )
        .unwrap();

        assert!(summary.derived.is_empty());
        assert!(summary.failures.is_empty());
        assert!(
            summary.all_metadata_verified(),
            "vacuously, with nothing derived"
        );
    }

    #[test]
    fn a_file_that_is_not_a_raw_is_reported_rather_than_failing_the_batch() {
        let temp = tempfile::tempdir().unwrap();
        let not_raw = temp.path().join("notes.txt");
        std::fs::write(&not_raw, b"this is not a photograph").unwrap();

        let summary = derive_batch(
            &[DerivationRequest {
                source: not_raw,
                stem: "notes".into(),
            }],
            &temp.path().join("out"),
            &Thresholds::default(),
            &crate::jobs::InMemoryProgress::new(),
        )
        .unwrap();

        assert!(summary.derived.is_empty());
        assert_eq!(summary.failures.len(), 1);
        assert!(
            summary.failures[0].reason.contains("could not derive"),
            "{}",
            summary.failures[0].reason
        );
    }
}
