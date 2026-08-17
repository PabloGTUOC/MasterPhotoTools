use crate::error::Error;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub hash: String,
    pub volume_label: Option<String>,
}

impl Fingerprint {
    pub fn generate(_path: &Path) -> Result<Self, Error> {
        // Fall back to a deterministic hash for tests and initial implementation
        let mut hasher = Sha256::new();
        hasher.update(b"mock_fingerprint_data");
        let res = hasher.finalize();
        let hash = res.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Self {
            hash,
            volume_label: Some("MOCK_CARD".to_string()),
        })
    }
}
