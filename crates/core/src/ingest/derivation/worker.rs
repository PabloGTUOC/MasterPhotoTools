use crate::error::Error;
use crate::ingest::scanner::hash_file;
use crate::ingest::{ScannedAsset, Shot};
use crate::ledger::Ledger;
use crate::media::image_ops;
use crate::media::meta::ExifWriter;
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;

/// Long edge of a staged derivative, in pixels.
const DERIVATIVE_LONG_EDGE: u32 = 2000;

#[derive(Debug, Clone)]
pub struct DeriveJob {
    pub shot_id: String,
    pub shot: Shot,
    pub primary_asset: ScannedAsset,
    pub staging_dir: PathBuf,
}

/// One finished derivative.
#[derive(Debug, Clone)]
pub struct Derived {
    pub shot_id: String,
    pub staged_path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

pub struct WorkerPool {
    pool: rayon::ThreadPool,
}

impl WorkerPool {
    pub fn new(threads: usize) -> Result<Self, Error> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(Self { pool })
    }

    /// Derive staged copies for a batch of shots.
    ///
    /// Two passes, and the split is deliberate (G4). Decode, resize and encode
    /// are CPU-bound and run in parallel across the pool. Metadata is then
    /// carried across serially through **one** `exiftool` process for the whole
    /// batch — starting one per file costs 150–250 ms each regardless of file
    /// size, which on a 400-frame card is over a minute of pure process overhead
    /// (specification §2.6).
    pub fn process(&self, jobs: Vec<DeriveJob>) -> Result<Vec<Derived>, Error> {
        let staged: Vec<(DeriveJob, PathBuf, u32, u32)> = self
            .pool
            .install(|| {
                jobs.into_par_iter()
                    .map(Self::derive_pixels)
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .collect::<Result<Vec<_>, Error>>()?;

        if staged.is_empty() {
            return Ok(Vec::new());
        }

        let mut writer = ExifWriter::start()?;
        let mut out = Vec::with_capacity(staged.len());

        for (job, path, width, height) in staged {
            writer.copy_metadata(&job.primary_asset.path, &path)?;

            // Hash and size are measured after the metadata pass, because that
            // pass rewrites the file.
            let bytes = fs::metadata(&path)?.len();
            let sha256 = hash_file(&path)?;

            out.push(Derived {
                shot_id: job.shot_id,
                staged_path: path,
                sha256,
                bytes,
                width,
                height,
            });
        }

        writer.close()?;
        Ok(out)
    }

    /// Derive and record a batch, persisting each result to the ledger.
    pub fn process_batch(&self, jobs: Vec<DeriveJob>, ledger: &Ledger) -> Result<(), Error> {
        for derived in self.process(jobs)? {
            ledger
                .add_derived(
                    &derived.shot_id,
                    &derived.staged_path.to_string_lossy(),
                    &derived.sha256,
                    derived.bytes,
                    derived.width,
                    derived.height,
                )
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
        Ok(())
    }

    /// The parallel half: everything that touches pixels, and nothing that
    /// touches `exiftool`.
    fn derive_pixels(job: DeriveJob) -> Result<(DeriveJob, PathBuf, u32, u32), Error> {
        fs::create_dir_all(&job.staging_dir)?;

        let out_path = job.staging_dir.join(format!("{}_proxy.jpg", job.shot.stem));
        let img = image_ops::decode_oriented(&job.primary_asset.path)?;
        let resized = image_ops::downscale_to_max_edge(&img, DERIVATIVE_LONG_EDGE)?;
        let (width, height) = (resized.width(), resized.height());

        image_ops::encode_jpeg(&resized, 95, &out_path)?;
        Ok((job, out_path, width, height))
    }
}
