pub mod fingerprint;
pub mod grouping;
pub mod scanner;

pub use fingerprint::Fingerprint;
pub use grouping::{group_assets, CandidateShot};
pub use scanner::{CandidateAsset, Scanner};

use crate::error::Error;
use crate::ledger::Ledger;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn ingest_card(root: &Path, ledger: &Ledger) -> Result<(), Error> {
    let fingerprint = Fingerprint::generate(root)?;
    let card_id = fingerprint.hash.clone();

    ledger
        .add_card(&card_id, &fingerprint.hash)
        .map_err(|e| Error::Internal(e.to_string()))?;

    let assets = Scanner::scan(root)?;
    let shots = group_assets(assets);

    for shot in shots {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}_{}", card_id, shot.id).as_bytes());
        let res = hasher.finalize();
        let shot_id: String = res.iter().map(|b| format!("{:02x}", b)).collect();

        ledger
            .add_shot(&shot_id, &card_id, &shot.id)
            .map_err(|e| Error::Internal(e.to_string()))?;

        for asset in shot.assets {
            let mut hasher = Sha256::new();
            hasher.update(format!("{}_{}", shot_id, asset.sha256).as_bytes());
            let res = hasher.finalize();
            let asset_id: String = res.iter().map(|b| format!("{:02x}", b)).collect();

            let rel_path = asset
                .path
                .strip_prefix(root)
                .unwrap_or(&asset.path)
                .to_string_lossy()
                .to_string();
            ledger
                .add_asset(&asset_id, &shot_id, &rel_path, asset.bytes, &asset.sha256)
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
    }

    Ok(())
}
