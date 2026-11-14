//! Lens correction for Develop, from the lensfun database (DV-9).
//!
//! [`lens_info`] reads what a RAW file says about its camera and lens, [`profile_for`] finds the
//! lensfun profile and evaluates it at the shot's focal length and f-number, and [`correct`] applies
//! it to the linear image in sensor orientation, before noise reduction and the look.
//!
//! A profile is used only when it is the right one. A wrong lens bends straight lines that were
//! straight, so anything short of a confident match corrects nothing and says why.

pub mod db;
pub mod model;

use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;

pub use model::{Correction, Distortion, Tca};

/// The diagonal of a full-frame sensor, in millimetres: what a crop factor is relative to.
const FULL_FRAME_DIAGONAL: f32 = 43.2666;

/// What a RAW file says about the camera and lens that made it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LensInfo {
    pub make: String,
    pub model: String,
    /// The lens's name, for an interchangeable lens; `None` for most fixed-lens cameras.
    pub lens: Option<String>,
    pub focal_mm: Option<f32>,
    pub f_number: Option<f32>,
}

/// Reads the camera, lens, focal length and f-number from a RAW file's metadata.
pub fn lens_info(raw: &[u8]) -> Option<LensInfo> {
    let source = RawSource::new_from_shared_vec(std::sync::Arc::new(raw.to_vec()));
    let decoder = rawler::get_decoder(&source).ok()?;
    let md = decoder
        .raw_metadata(&source, &RawDecodeParams::default())
        .ok()?;
    let rational =
        |r: &rawler::formats::tiff::Rational| (r.d != 0).then(|| r.n as f32 / r.d as f32);
    let lens = md
        .lens
        .as_ref()
        .map(|l| {
            format!("{} {}", l.lens_make, l.lens_model)
                .trim()
                .to_string()
        })
        .or_else(|| md.exif.lens_model.clone())
        .filter(|l| !l.trim().is_empty());
    Some(LensInfo {
        make: md.make.clone(),
        model: md.model.clone(),
        lens,
        focal_mm: md.exif.focal_length.as_ref().and_then(rational),
        f_number: md.exif.fnumber.as_ref().and_then(rational),
    })
}

/// A matched profile, evaluated for one shot.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// The lens's name in lensfun, for the report.
    pub lens_name: String,
    pub correction: Correction,
}

/// For comparing names: lower case, letters, digits and dots only.
fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '.')
        .flat_map(char::to_lowercase)
        .collect()
}

fn maker_matches(names: &[String], make: &str) -> bool {
    let make = normalise(make);
    !make.is_empty()
        && names.iter().any(|n| {
            let n = normalise(n);
            !n.is_empty() && (n.contains(&make) || make.contains(&n))
        })
}

/// The lens on this camera: its name when the file gives one, or the only lens on the camera's own
/// mount (a fixed-lens camera).
fn find_lens<'a>(
    db: &'a db::Database,
    camera: &db::Camera,
    info: &LensInfo,
) -> Result<&'a db::Lens, String> {
    // Lenses that fit: the camera's mount, and the mounts it lists as compatible.
    let mut mounts = vec![camera.mount.clone()];
    if let Some(m) = db.mounts.iter().find(|m| m.name == camera.mount) {
        mounts.extend(m.compat.iter().cloned());
    }
    let fits = |l: &&db::Lens| l.rectilinear && l.mounts.iter().any(|m| mounts.contains(m));

    match &info.lens {
        Some(name) => {
            let wanted = normalise(name);
            let matches: Vec<&db::Lens> = db
                .lenses
                .iter()
                .filter(fits)
                .filter(|l| {
                    l.models.iter().any(|m| {
                        let model = normalise(m);
                        // lensfun often writes the maker into the model; the file often does not.
                        let bare = l
                            .makers
                            .iter()
                            .map(|mk| normalise(mk))
                            .find(|mk| model.starts_with(mk.as_str()))
                            .map(|mk| model[mk.len()..].to_string())
                            .unwrap_or_else(|| model.clone());
                        model == wanted
                            || bare == wanted
                            || wanted.ends_with(&bare) && !bare.is_empty()
                    })
                })
                .collect();
            matches
                .into_iter()
                .find(|l| l.is_calibrated())
                .ok_or_else(|| format!("no lens profile for \"{name}\""))
        }
        None => {
            let own: Vec<&db::Lens> = db
                .lenses
                .iter()
                .filter(|l| l.rectilinear && l.mounts.contains(&camera.mount))
                .collect();
            match own.as_slice() {
                [only] if only.is_calibrated() => Ok(only),
                [_] => Err("its lens has no calibration in the lens database".to_string()),
                [] => Err("the file does not name its lens".to_string()),
                _ => {
                    Err("the file does not name its lens, and several fit this camera".to_string())
                }
            }
        }
    }
}

/// The profile for one shot, evaluated at its focal length and f-number, for a picture of `width`
/// by `height` in sensor orientation.
pub fn profile_for(info: &LensInfo, width: u32, height: u32) -> Result<Profile, String> {
    let db = db::database()?;
    let model = normalise(&info.model);
    let camera = db
        .cameras
        .iter()
        .find(|c| {
            maker_matches(&c.makers, &info.make) && c.models.iter().any(|m| normalise(m) == model)
        })
        .ok_or_else(|| format!("{} {} is not in the lens database", info.make, info.model))?;
    let lens = find_lens(db, camera, info)?;

    let focal = info.focal_mm.filter(|f| *f > 0.0);
    let needs_focal = |n: usize| n > 1;
    let distortion = match focal {
        Some(f) => model::distortion_at(&lens.distortion, f),
        None if !needs_focal(lens.distortion.len()) => lens.distortion.first().map(|d| d.1),
        None => None,
    };
    let tca = match focal {
        Some(f) => model::tca_at(&lens.tca, f),
        None if !needs_focal(lens.tca.len()) => lens.tca.first().map(|t| t.1),
        None => None,
    };
    let vignetting = match (focal, info.f_number) {
        (Some(f), Some(n)) if n > 0.0 => model::vignetting_at(&lens.vignetting, f, n),
        _ => None,
    };
    if distortion.is_none() && tca.is_none() && vignetting.is_none() {
        return Err("the file does not record the focal length the profile needs".to_string());
    }

    // The calibration was made on a sensor of the lens's crop factor and aspect ratio; this picture
    // is from the camera's. Both radii are converted through millimetres on the sensor.
    let crop = if camera.crop > 0.0 { camera.crop } else { 1.0 };
    let cal_crop = if lens.crop > 0.0 { lens.crop } else { crop };
    let aspect = width.max(height) as f32 / width.min(height).max(1) as f32;
    let short_side =
        |crop: f32, aspect: f32| (FULL_FRAME_DIAGONAL / crop) / (1.0 + aspect * aspect).sqrt();
    let radius_scale = short_side(crop, aspect) / short_side(cal_crop, lens.aspect);
    let vignetting_scale = cal_crop / crop;

    Ok(Profile {
        lens_name: lens.models.first().cloned().unwrap_or_default(),
        correction: Correction {
            distortion,
            tca,
            vignetting,
            radius_scale,
            vignetting_scale,
        },
    })
}

/// What a lens correction did to one photograph.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub lens_name: String,
    pub distortion: bool,
    pub tca: bool,
    pub vignetting: bool,
    pub scale: f32,
}

/// Corrects linear pixels in sensor orientation with a profile.
pub fn correct(
    width: u32,
    height: u32,
    pixels: &[[f32; 3]],
    profile: &Profile,
) -> (Vec<[f32; 3]>, Applied) {
    let (out, scale) = model::apply(width, height, pixels, &profile.correction);
    (
        out,
        Applied {
            lens_name: profile.lens_name.clone(),
            distortion: profile.correction.distortion.is_some(),
            tca: profile.correction.tca.is_some(),
            vignetting: profile.correction.vignetting.is_some(),
            scale,
        },
    )
}
