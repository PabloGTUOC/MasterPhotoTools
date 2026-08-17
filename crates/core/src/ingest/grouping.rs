use crate::ingest::scanner::CandidateAsset;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateShot {
    pub id: String, // Stem representing the shot
    pub assets: Vec<CandidateAsset>,
}

pub fn group_assets(assets: Vec<CandidateAsset>) -> Vec<CandidateShot> {
    let mut groups: HashMap<String, Vec<CandidateAsset>> = HashMap::new();

    for asset in assets {
        let name = asset
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Extract stem: e.g. "IMG_0001.JPG" -> "IMG_0001", "IMG_0001.JPG.json" -> "IMG_0001"
        let real_stem = name.split('.').next().unwrap_or(&name).to_string();

        groups.entry(real_stem).or_default().push(asset);
    }

    groups
        .into_iter()
        .map(|(stem, assets)| CandidateShot { id: stem, assets })
        .collect()
}
