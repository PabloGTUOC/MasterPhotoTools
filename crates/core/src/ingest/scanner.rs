use crate::error::Error;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateAsset {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

pub struct Scanner;

impl Scanner {
    pub fn scan(root: &Path) -> Result<Vec<CandidateAsset>, Error> {
        let mut assets = Vec::new();

        for entry in WalkDir::new(root).into_iter().filter_entry(|e| !is_junk(e)) {
            let entry = entry.map_err(|e| Error::Internal(e.to_string()))?;
            let path = entry.path();
            if path.is_file() {
                let metadata = entry
                    .metadata()
                    .map_err(|e| Error::Internal(e.to_string()))?;
                let sha256 = hash_file(path)?;
                assets.push(CandidateAsset {
                    path: path.to_path_buf(),
                    sha256,
                    bytes: metadata.len(),
                });
            }
        }
        Ok(assets)
    }
}

fn is_junk(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    name.starts_with('.') || name == "System Volume Information" || name == "$RECYCLE.BIN"
}

pub fn hash_file(path: &Path) -> Result<String, Error> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let res = hasher.finalize();
    Ok(res.iter().map(|b| format!("{:02x}", b)).collect())
}
