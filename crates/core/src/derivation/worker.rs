use crate::error::Error;
use crate::ingest::scanner::hash_file;
use crate::ingest::{CandidateAsset, CandidateShot};
use crate::ledger::Ledger;
use crate::media::image_ops::{decode, resize};
use crate::media::meta::ExifWriter;
use image::ImageFormat;
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DeriveJob {
    pub shot_id: String,
    pub shot: CandidateShot,
    pub primary_asset: CandidateAsset,
    pub staging_dir: PathBuf,
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

    #[allow(clippy::type_complexity)]
    pub fn process(
        &self,
        jobs: Vec<DeriveJob>,
    ) -> Result<Vec<(String, PathBuf, String, u64, u32, u32)>, Error> {
        let results: Vec<Result<(String, PathBuf, String, u64, u32, u32), Error>> = self
            .pool
            .install(|| jobs.into_par_iter().map(Self::process_single).collect());

        results.into_iter().collect()
    }

    #[allow(clippy::type_complexity)]
    pub fn process_batch(&self, jobs: Vec<DeriveJob>, ledger: &Ledger) -> Result<(), Error> {
        let results: Vec<Result<(String, PathBuf, String, u64, u32, u32), Error>> = self
            .pool
            .install(|| jobs.into_par_iter().map(Self::process_single).collect());

        for res in results {
            match res {
                Ok((shot_id, staged_path, sha256, bytes, width, height)) => {
                    let path_str = staged_path.to_string_lossy().to_string();
                    ledger
                        .add_derived(&shot_id, &path_str, &sha256, bytes, width, height)
                        .unwrap_or_else(|e| {
                            eprintln!(
                                "Failed to persist derived asset for shot {}: {}",
                                shot_id, e
                            );
                        });
                }
                Err(e) => {
                    eprintln!("Job failed: {}", e);
                }
            }
        }

        Ok(())
    }

    fn process_single(job: DeriveJob) -> Result<(String, PathBuf, String, u64, u32, u32), Error> {
        fs::create_dir_all(&job.staging_dir)?;

        let out_name = format!("{}_proxy.jpg", job.shot.id);
        let out_path = job.staging_dir.join(&out_name);

        let img = decode(&job.primary_asset.path)?;

        // Resize to 2000px
        let (w, h) = (img.width(), img.height());
        let ratio = 2000.0 / (w.max(h) as f64);
        let new_w = ((w as f64 * ratio).round() as u32).min(w);
        let new_h = ((h as f64 * ratio).round() as u32).min(h);

        let resized = resize(&img, new_w, new_h)?;

        resized
            .save_with_format(&out_path, ImageFormat::Jpeg)
            .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;

        let mut exif_writer = ExifWriter::start()?;
        exif_writer.copy_metadata(&job.primary_asset.path, &out_path)?;

        let bytes = fs::metadata(&out_path)?.len();
        let sha256 = hash_file(&out_path)?;

        Ok((job.shot_id, out_path, sha256, bytes, new_w, new_h))
    }
}
