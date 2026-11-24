//! Film simulation engine using spektrafilm (DV-4).
//!
//! Simulates 18 film stocks from Kodak and Fujifilm via physical modelling
//! of emulsion sensitivity, dye couplers, development, enlarger/paper printing,
//! or direct slide film scanning.

use phototools_core::error::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::media::looks::builtin::BuiltinLookInfo;

/// Grain setting for film simulation (DV-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Grain {
    /// Grain simulation disabled (`film_render.grain.active = false`).
    Off,
    /// Medium format 60 mm grain (`camera.film_format_mm = 60.0`).
    #[default]
    Fine,
    /// 35 mm format grain (`camera.film_format_mm = 35.0`).
    Standard,
}

/// Requested colour space and transfer function for film output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilmOutput {
    /// Display sRGB with sRGB CCTF applied (for JPEG).
    Srgb,
    /// Scene-referred linear ProPhoto RGB (for 16-bit TIFF).
    ProPhotoLinear,
}

/// Compute backend for film simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilmBackend {
    /// Use wgpu GPU acceleration if an adapter exists, else CPU.
    Auto,
    /// Force CPU reference backend (used in deterministic tests).
    Cpu,
}

/// Film stock specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilmStockInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub maker: &'static str,
    pub is_slide: bool,
    /// Balanced for tungsten light, so blue in daylight unless printed through a filter (DV-6).
    pub tungsten: bool,
}

/// All 18 supported film stocks in defined order (DV-4).
pub static FILM_STOCKS: &[FilmStockInfo] = &[
    // Kodak (13)
    FilmStockInfo {
        id: "kodak_portra_160",
        name: "Kodak Portra 160",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_portra_400",
        name: "Kodak Portra 400",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_portra_800",
        name: "Kodak Portra 800",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_ektar_100",
        name: "Kodak Ektar 100",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_gold_200",
        name: "Kodak Gold 200",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_ultramax_400",
        name: "Kodak Ultramax 400",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_ektachrome_100",
        name: "Kodak Ektachrome 100",
        maker: "Kodak",
        is_slide: true,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_kodachrome_64",
        name: "Kodak Kodachrome 64",
        maker: "Kodak",
        is_slide: true,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_vision3_50d",
        name: "Kodak Vision3 50D",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_vision3_200t",
        name: "Kodak Vision3 200T",
        maker: "Kodak",
        is_slide: false,
        tungsten: true,
    },
    FilmStockInfo {
        id: "kodak_vision3_250d",
        name: "Kodak Vision3 250D",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "kodak_vision3_500t",
        name: "Kodak Vision3 500T",
        maker: "Kodak",
        is_slide: false,
        tungsten: true,
    },
    FilmStockInfo {
        id: "kodak_verita_200d",
        name: "Kodak Verita 200D",
        maker: "Kodak",
        is_slide: false,
        tungsten: false,
    },
    // Fujifilm (5)
    FilmStockInfo {
        id: "fujifilm_c200",
        name: "Fujifilm C200",
        maker: "Fujifilm",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "fujifilm_pro_400h",
        name: "Fujifilm Pro 400H",
        maker: "Fujifilm",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "fujifilm_xtra_400",
        name: "Fujifilm X-Tra 400",
        maker: "Fujifilm",
        is_slide: false,
        tungsten: false,
    },
    FilmStockInfo {
        id: "fujifilm_provia_100f",
        name: "Fujifilm Provia 100F",
        maker: "Fujifilm",
        is_slide: true,
        tungsten: false,
    },
    FilmStockInfo {
        id: "fujifilm_velvia_100",
        name: "Fujifilm Velvia 100",
        maker: "Fujifilm",
        is_slide: true,
        tungsten: false,
    },
];

/// The file name `ensure_look_lut` and `film_lut` give a film simulation in the LUT library.
pub fn film_lut_file_name(name: &str, balance: LutBalance) -> String {
    let tag = match balance {
        LutBalance::NeutralGrey => "neutral (spektrafilm)",
        LutBalance::AsFilm => "(spektrafilm)",
    };
    format!("{name} {tag}.cube")
}

/// Edit's balance rule: tungsten stocks use NeutralGrey, all others AsFilm.
pub fn edit_balance(tungsten: bool) -> LutBalance {
    if tungsten {
        LutBalance::NeutralGrey
    } else {
        LutBalance::AsFilm
    }
}

/// Lists all supported film stocks as BuiltinLookInfo items.
pub fn film_look_list() -> Vec<BuiltinLookInfo> {
    FILM_STOCKS
        .iter()
        .map(|s| BuiltinLookInfo {
            id: format!("film:{}", s.id),
            name: s.name.to_string(),
            maker: s.maker.to_string(),
            tungsten: s.tungsten,
            library_file_name: edit_film_lut_file_name(s.name, edit_balance(s.tungsten)),
        })
        .collect()
}

/// Embedded film profiles, LUTs, and filter tables.
pub static EMBEDDED_FILES: &[(&str, &[u8])] = &[
    ("CREDITS.md", include_bytes!("../../film-data/CREDITS.md")),
    (
        "SPEKTRAFILM_LICENSE.txt",
        include_bytes!("../../film-data/SPEKTRAFILM_LICENSE.txt"),
    ),
    (
        "filters/neutral_print_filters.json",
        include_bytes!("../../film-data/filters/neutral_print_filters.json"),
    ),
    (
        "luts/spectral_upsampling/irradiance_xy_tc.npy",
        include_bytes!("../../film-data/luts/spectral_upsampling/irradiance_xy_tc.npy"),
    ),
    (
        "profiles/fujifilm_c200.json",
        include_bytes!("../../film-data/profiles/fujifilm_c200.json"),
    ),
    (
        "profiles/fujifilm_crystal_archive_typeii.json",
        include_bytes!("../../film-data/profiles/fujifilm_crystal_archive_typeii.json"),
    ),
    (
        "profiles/fujifilm_pro_400h.json",
        include_bytes!("../../film-data/profiles/fujifilm_pro_400h.json"),
    ),
    (
        "profiles/fujifilm_provia_100f.json",
        include_bytes!("../../film-data/profiles/fujifilm_provia_100f.json"),
    ),
    (
        "profiles/fujifilm_velvia_100.json",
        include_bytes!("../../film-data/profiles/fujifilm_velvia_100.json"),
    ),
    (
        "profiles/fujifilm_xtra_400.json",
        include_bytes!("../../film-data/profiles/fujifilm_xtra_400.json"),
    ),
    (
        "profiles/kodak_2383.json",
        include_bytes!("../../film-data/profiles/kodak_2383.json"),
    ),
    (
        "profiles/kodak_ektachrome_100.json",
        include_bytes!("../../film-data/profiles/kodak_ektachrome_100.json"),
    ),
    (
        "profiles/kodak_ektar_100.json",
        include_bytes!("../../film-data/profiles/kodak_ektar_100.json"),
    ),
    (
        "profiles/kodak_gold_200.json",
        include_bytes!("../../film-data/profiles/kodak_gold_200.json"),
    ),
    (
        "profiles/kodak_kodachrome_64.json",
        include_bytes!("../../film-data/profiles/kodak_kodachrome_64.json"),
    ),
    (
        "profiles/kodak_portra_160.json",
        include_bytes!("../../film-data/profiles/kodak_portra_160.json"),
    ),
    (
        "profiles/kodak_portra_400.json",
        include_bytes!("../../film-data/profiles/kodak_portra_400.json"),
    ),
    (
        "profiles/kodak_portra_800.json",
        include_bytes!("../../film-data/profiles/kodak_portra_800.json"),
    ),
    (
        "profiles/kodak_portra_endura.json",
        include_bytes!("../../film-data/profiles/kodak_portra_endura.json"),
    ),
    (
        "profiles/kodak_ultramax_400.json",
        include_bytes!("../../film-data/profiles/kodak_ultramax_400.json"),
    ),
    (
        "profiles/kodak_verita_200d.json",
        include_bytes!("../../film-data/profiles/kodak_verita_200d.json"),
    ),
    (
        "profiles/kodak_vision3_200t.json",
        include_bytes!("../../film-data/profiles/kodak_vision3_200t.json"),
    ),
    (
        "profiles/kodak_vision3_250d.json",
        include_bytes!("../../film-data/profiles/kodak_vision3_250d.json"),
    ),
    (
        "profiles/kodak_vision3_500t.json",
        include_bytes!("../../film-data/profiles/kodak_vision3_500t.json"),
    ),
    (
        "profiles/kodak_vision3_50d.json",
        include_bytes!("../../film-data/profiles/kodak_vision3_50d.json"),
    ),
];

/// First 12 hex characters of SHA-256 over all embedded files' names and contents.
pub fn embedded_files_hash() -> String {
    let mut hasher = Sha256::new();
    for (name, bytes) in EMBEDDED_FILES {
        hasher.update(name.as_bytes());
        hasher.update(bytes);
    }
    let hex = phototools_core::ingest::scanner::hex(&hasher.finalize());
    hex[..12].to_string()
}

/// Materialises embedded film data into `<base_dir>/film-data-<hash>/`.
///
/// On first use, writes files into a temporary sibling directory and atomically renames it.
/// If the directory already exists and all files are intact, reuses them without touching mtimes.
/// Missing or truncated files are repaired.
pub fn ensure_film_data(base_dir: &Path) -> Result<PathBuf, Error> {
    let hash = embedded_files_hash();
    let target_dir = base_dir.join(format!("film-data-{hash}"));

    if !target_dir.exists() {
        std::fs::create_dir_all(base_dir).map_err(Error::Io)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_path = base_dir.join(format!(
            ".film-data-{hash}.tmp-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_path).map_err(Error::Io)?;

        for (rel, bytes) in EMBEDDED_FILES {
            let out_file = temp_path.join(rel);
            if let Some(parent) = out_file.parent() {
                std::fs::create_dir_all(parent).map_err(Error::Io)?;
            }
            std::fs::write(&out_file, bytes).map_err(Error::Io)?;
        }

        if let Err(e) = std::fs::rename(&temp_path, &target_dir) {
            let _ = std::fs::remove_dir_all(&temp_path);
            if !target_dir.exists() {
                return Err(Error::Io(e));
            }
        }
    }

    // Verify all files exist and match expected size; repair missing or damaged files
    for (rel, bytes) in EMBEDDED_FILES {
        let file_path = target_dir.join(rel);
        let valid = match std::fs::metadata(&file_path) {
            Ok(meta) => meta.is_file() && meta.len() == bytes.len() as u64,
            Err(_) => false,
        };
        if !valid {
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent).map_err(Error::Io)?;
            }
            std::fs::write(&file_path, bytes).map_err(Error::Io)?;
        }
    }

    Ok(target_dir)
}

#[cfg(feature = "film")]
static AUTO_BACKEND: std::sync::OnceLock<
    std::sync::Mutex<Box<dyn spektrafilm_gpu::ComputeBackend>>,
> = std::sync::OnceLock::new();
#[cfg(feature = "film")]
static CPU_BACKEND: std::sync::OnceLock<
    std::sync::Mutex<Box<dyn spektrafilm_gpu::ComputeBackend>>,
> = std::sync::OnceLock::new();

// The locks recover from poisoning: a render that panicked is already reported as an error by
// `catch_unwind`, and a poisoned lock would otherwise refuse every later film render until the
// program restarted (G10). Neither the backend nor the cache is left half-updated by a panic.
#[cfg(feature = "film")]
fn backend_lock(
    backend: FilmBackend,
) -> &'static std::sync::Mutex<Box<dyn spektrafilm_gpu::ComputeBackend>> {
    match backend {
        FilmBackend::Auto => {
            AUTO_BACKEND.get_or_init(|| std::sync::Mutex::new(spektrafilm_gpu::select_backend()))
        }
        FilmBackend::Cpu => CPU_BACKEND.get_or_init(|| {
            std::sync::Mutex::new(Box::new(spektrafilm_gpu::cpu_backend::CpuBackend))
        }),
    }
}

#[cfg(feature = "film")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PipelineCacheKey {
    stock: String,
    grain: Grain,
    output: FilmOutput,
}

#[cfg(feature = "film")]
struct PipelineCache {
    entries: std::collections::VecDeque<(PipelineCacheKey, spektrafilm_core::pipeline::Pipeline)>,
}

#[cfg(feature = "film")]
static PIPELINE_CACHE: std::sync::Mutex<PipelineCache> = std::sync::Mutex::new(PipelineCache {
    entries: std::collections::VecDeque::new(),
});

#[cfg(feature = "film")]
fn get_or_build_pipeline(
    stock_info: &FilmStockInfo,
    grain: Grain,
    output: FilmOutput,
    data_dir: &Path,
) -> Result<spektrafilm_core::pipeline::Pipeline, Error> {
    let key = PipelineCacheKey {
        stock: stock_info.id.to_string(),
        grain,
        output,
    };

    {
        let mut cache = PIPELINE_CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((k, p)) = cache
            .entries
            .iter()
            .position(|(k, _)| k == &key)
            .and_then(|pos| cache.entries.remove(pos))
        {
            let clone = p.clone();
            cache.entries.push_back((k, p));
            return Ok(clone);
        }
    }

    // Ensure film data files are present in the directory
    let data_path = ensure_film_data(data_dir)?;

    let film = spektrafilm_core::profile::load_profile_by_name(&data_path, stock_info.id)
        .map_err(|e| Error::Internal(format!("loading film profile {}: {e}", stock_info.id)))?;

    let print_stock = if stock_info.is_slide {
        stock_info.id.to_string()
    } else if let Some(ref target) = film.info.target_print {
        target.clone()
    } else {
        return Err(Error::Internal(format!(
            "film {} has no target_print and is not a slide",
            stock_info.id
        )));
    };

    let print = spektrafilm_core::profile::load_profile_by_name(&data_path, &print_stock)
        .map_err(|e| Error::Internal(format!("loading print profile {print_stock}: {e}")))?;

    let mut params = spektrafilm_core::params::RuntimeParams::default();
    params.io.scan_film = stock_info.is_slide;
    params.io.input_color_space = "ProPhoto RGB".to_string();
    params.io.input_cctf_decoding = false;

    match output {
        FilmOutput::Srgb => {
            params.io.output_color_space = "sRGB".to_string();
            params.io.output_cctf_encoding = true;
        }
        FilmOutput::ProPhotoLinear => {
            params.io.output_color_space = "ProPhoto RGB".to_string();
            params.io.output_cctf_encoding = false;
        }
    }

    match grain {
        Grain::Off => {
            params.film_render.grain.active = false;
        }
        Grain::Fine => {
            params.film_render.grain.active = true;
            params.camera.film_format_mm = 60.0;
        }
        Grain::Standard => {
            params.film_render.grain.active = true;
            params.camera.film_format_mm = 35.0;
        }
    }

    let pipeline =
        spektrafilm_core::pipeline::Pipeline::new_with_spectral(film, print, params, &data_path)
            .map_err(|e| {
                Error::Internal(format!(
                    "building spektrafilm pipeline for {}: {e}",
                    stock_info.id
                ))
            })?;

    {
        let mut cache = PIPELINE_CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.entries.len() >= 4 {
            cache.entries.pop_front();
        }
        cache.entries.push_back((key, pipeline.clone()));
    }

    Ok(pipeline)
}

/// Renders a linear ProPhoto RGB image through the film simulation engine (DV-4).
#[cfg(feature = "film")]
#[allow(clippy::too_many_arguments)]
pub fn render_film(
    linear: &[[f32; 3]],
    width: u32,
    height: u32,
    stock: &str,
    grain: Grain,
    exposure_ev: f32,
    output: FilmOutput,
    backend: FilmBackend,
    data_dir: &Path,
) -> Result<Vec<[f32; 3]>, Error> {
    let stock_info = FILM_STOCKS
        .iter()
        .find(|s| s.id == stock)
        .ok_or_else(|| Error::Refused(format!("unknown film stock: '{stock}'")))?;

    if !exposure_ev.is_finite() || !(-4.0..=4.0).contains(&exposure_ev) {
        return Err(Error::Refused(format!(
            "exposure_ev {exposure_ev} is outside supported range -4.0..=4.0 or not finite"
        )));
    }

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut pipeline = get_or_build_pipeline(stock_info, grain, output, data_dir)?;
        pipeline.params.camera.exposure_compensation_ev = exposure_ev;

        let input_f64: Vec<f64> = linear
            .iter()
            .flat_map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let input_buf = spektrafilm_math::image::ImageBuf::from_data(width, height, input_f64);

        let lock = backend_lock(backend);
        let backend_guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result_buf = pipeline.process(input_buf, backend_guard.as_ref());

        let out_pixels: Vec<[f32; 3]> = result_buf
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32])
            .collect();

        Ok(out_pixels)
    }))
    .unwrap_or_else(|panic_payload| {
        let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = panic_payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        Err(Error::Internal(format!(
            "film simulation panicked on stock '{stock}': {msg}"
        )))
    })
}

/// How a film LUT treats colour balance (DV-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LutBalance {
    /// The film's own colour, as a lab would print it straight: a tungsten film is blue in daylight.
    AsFilm,
    /// Printed through the enlarger filter that makes 18% grey neutral, found for the stock.
    NeutralGrey,
}

/// What a film LUT is built to be fed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LutInput {
    /// Any editor's sRGB (DV-6): decoded and handed to the film as it is, with the DIR couplers off.
    Srgb,
    /// Rollchrome's neutral rendering, which is what Edit applies a look to. Fed the plain sRGB
    /// way, the film was exposed to a picture that already had the neutral's tone curve and came out
    /// flatter and less saturated than the neutral itself, nothing like Develop's render of the same
    /// RAW. So the neutral's curve is undone first and the film sees scene light in ProPhoto as in
    /// Develop, with the DIR couplers on as in Develop (a LUT holds their colour, not their edges), at
    /// a fixed exposure where Develop meters each photograph.
    NeutralRendering { exposure_ev: f32 },
}

/// The exposure Edit's film LUTs are built at, in place of Develop's metering: fitted so that Edit's
/// look on a RAW matches Develop's film render of it (`examples/fit_film_lut.rs`). On the owner's five
/// LX100 RW2s of 2026-11-22, +0.75 EV puts Ektar 100, Portra 400 and Velvia 100 within 0.01 EV of
/// Develop on average (each photograph within 0.2 EV: Develop meters each one, a LUT cannot), with
/// 98–99% of Develop's colourfulness, where the sRGB-in LUT had 63–74% and −0.26 EV.
pub const EDIT_FILM_EXPOSURE_EV: f32 = 0.75;

/// The file name `ensure_look_lut` gives a film simulation in Edit's LUT library. Different from
/// [`film_lut_file_name`] (the LUT for other editors) because the two are built for different input,
/// and from the first Edit LUTs, so photographs already edited with those keep them.
pub fn edit_film_lut_file_name(name: &str, balance: LutBalance) -> String {
    let tag = match balance {
        LutBalance::NeutralGrey => "neutral (spektrafilm v2)",
        LutBalance::AsFilm => "(spektrafilm v2)",
    };
    format!("{name} {tag}.cube")
}

/// A film stock as a `.cube` LUT for other editors (DV-6).
#[derive(Debug, Clone, PartialEq)]
pub struct FilmLut {
    /// `<Name> (spektrafilm).cube`, or `<Name> neutral (spektrafilm).cube`.
    pub file_name: String,
    /// The whole `.cube` text.
    pub cube: String,
    /// The enlarger's yellow and magenta filter shifts the LUT was printed through.
    pub y_shift: f32,
    pub m_shift: f32,
}

/// The LUT size: 33 points a side, the size Photomator was tested with (DV-6).
pub const FILM_LUT_SIZE: usize = 33;

/// sRGB 118/255 is 18% grey; the neutral search makes it print grey.
#[cfg(feature = "film")]
const NEUTRAL_PROBE: f64 = 118.0 / 255.0;

#[cfg(feature = "film")]
fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The pipeline a LUT is sampled through.
///
/// Different from [`get_or_build_pipeline`] on purpose, and never cached there:
/// - the input is **gamma-encoded sRGB**, which is what an editor feeds a LUT, decoded before the
///   engine rather than by it;
/// - grain, halation, DIR couplers and auto exposure are off, because a LUT maps one colour to one
///   colour and can hold none of them;
/// - it is built with `new_with_spectral`. The engine's own `export-lut` uses `Pipeline::new`,
///   which skips the print-exposure calibration, and its LUTs come out about two stops dark.
#[cfg(feature = "film")]
fn lut_pipeline(
    stock: &FilmStockInfo,
    y_shift: f32,
    m_shift: f32,
    data_path: &Path,
    input: LutInput,
) -> Result<(spektrafilm_core::pipeline::Pipeline, String), Error> {
    let film = spektrafilm_core::profile::load_profile_by_name(data_path, stock.id)
        .map_err(|e| Error::Internal(format!("loading film profile {}: {e}", stock.id)))?;
    let print_stock = if stock.is_slide {
        stock.id.to_string()
    } else if let Some(target) = film.info.target_print.clone() {
        target
    } else {
        return Err(Error::Internal(format!(
            "film {} has no target_print and is not a slide",
            stock.id
        )));
    };
    let print = spektrafilm_core::profile::load_profile_by_name(data_path, &print_stock)
        .map_err(|e| Error::Internal(format!("loading print profile {print_stock}: {e}")))?;

    let mut params = spektrafilm_core::params::RuntimeParams::default();
    params.io.scan_film = stock.is_slide;
    params.io.input_color_space = "sRGB".to_string();
    params.io.input_cctf_decoding = false;
    params.io.output_color_space = "sRGB".to_string();
    params.io.output_cctf_encoding = true;
    params.film_render.grain.active = false;
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    params.camera.auto_exposure = false;
    params.enlarger.y_filter_shift = y_shift;
    params.enlarger.m_filter_shift = m_shift;
    if let LutInput::NeutralRendering { .. } = input {
        params.io.input_color_space = "ProPhoto RGB".to_string();
        params.film_render.dir_couplers.active = true;
    }

    let mut pipeline =
        spektrafilm_core::pipeline::Pipeline::new_with_spectral(film, print, params, data_path)
            .map_err(|e| Error::Internal(format!("calibrating {}: {e}", stock.id)))?;
    // After building, as `render_film` does: the build calibrates the print exposure, which
    // cancels an exposure set before it.
    if let LutInput::NeutralRendering { exposure_ev } = input {
        pipeline.params.camera.exposure_compensation_ev = exposure_ev;
    }
    Ok((pipeline, print_stock))
}

/// One sRGB colour through a LUT pipeline, as its own 1×1 picture: the scanner's unsharp mask and
/// the glare look at neighbouring pixels, so a grid rendered as one picture would blur its entries
/// into each other.
#[cfg(feature = "film")]
fn lut_sample(
    pipeline: &spektrafilm_core::pipeline::Pipeline,
    backend: &dyn spektrafilm_gpu::ComputeBackend,
    srgb: [f64; 3],
    input: LutInput,
) -> [f64; 3] {
    let linear = [
        srgb_decode(srgb[0]),
        srgb_decode(srgb[1]),
        srgb_decode(srgb[2]),
    ];
    let data = match input {
        LutInput::Srgb => linear.to_vec(),
        LutInput::NeutralRendering { .. } => {
            // Back through the neutral's last steps: sRGB to linear ProPhoto, then its tone curve.
            let m = crate::media::develop::colour::srgb_to_prophoto_matrix();
            let pp = crate::media::develop::colour::mul_3x3_vec(
                m,
                [linear[0] as f32, linear[1] as f32, linear[2] as f32],
            );
            pp.iter()
                .map(|&v| {
                    f64::from(crate::media::develop::tone::standard_tone_inverse(
                        v.clamp(0.0, 1.0),
                    ))
                })
                .collect()
        }
    };
    let input = spektrafilm_math::image::ImageBuf::from_data(1, 1, data);
    let out = pipeline.process(input, backend);
    [out.data[0], out.data[1], out.data[2]]
}

/// The enlarger filter shifts that print 18% grey neutral for `stock`, by Newton's method on
/// `(r − g, b − g)` with a Jacobian from central differences.
#[cfg(feature = "film")]
fn neutral_shifts(
    stock: &FilmStockInfo,
    data_path: &Path,
    input: LutInput,
) -> Result<(f32, f32), Error> {
    let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
    let grey = [NEUTRAL_PROBE; 3];
    let cast = |y: f32, m: f32| -> Result<[f64; 2], Error> {
        let (pipeline, _) = lut_pipeline(stock, y, m, data_path, input)?;
        let o = lut_sample(&pipeline, &backend, grey, input);
        Ok([o[0] - o[1], o[2] - o[1]])
    };
    const STEP: f32 = 2.0;
    const MAX_STEP: f64 = 40.0;
    let (mut y, mut m) = (0.0f32, 0.0f32);
    for _ in 0..20 {
        let d = cast(y, m)?;
        if d[0].abs() < 0.002 && d[1].abs() < 0.002 {
            return Ok((y, m));
        }
        let dy_p = cast(y + STEP, m)?;
        let dy_m = cast(y - STEP, m)?;
        let dm_p = cast(y, m + STEP)?;
        let dm_m = cast(y, m - STEP)?;
        let step = f64::from(2.0 * STEP);
        let j = [
            [(dy_p[0] - dy_m[0]) / step, (dm_p[0] - dm_m[0]) / step],
            [(dy_p[1] - dy_m[1]) / step, (dm_p[1] - dm_m[1]) / step],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if !det.is_finite() || det.abs() < 1e-9 {
            break;
        }
        let dy = (j[1][1] * d[0] - j[0][1] * d[1]) / det;
        let dm = (-j[1][0] * d[0] + j[0][0] * d[1]) / det;
        // A full Newton step from no filter overshoots for Edit's input (Vision3 500T jumped to
        // y −163 and out of range), so a step is limited to MAX_STEP on its larger axis.
        let scale = (MAX_STEP / dy.abs().max(dm.abs())).min(1.0);
        y -= (dy * scale) as f32;
        m -= (dm * scale) as f32;
        if !y.is_finite() || !m.is_finite() || y.abs() > 200.0 || m.abs() > 200.0 {
            break;
        }
    }
    Err(Error::Refused(format!(
        "could not find a neutral print filter for {}",
        stock.id
    )))
}

/// A film stock as a 33-point `.cube` LUT with sRGB in and sRGB out (DV-6), for other editors.
///
/// Deterministic: sampled on the CPU backend, so the same stock and balance give the same bytes.
#[cfg(feature = "film")]
pub fn film_lut(stock: &str, balance: LutBalance, data_dir: &Path) -> Result<FilmLut, Error> {
    film_lut_with(stock, balance, data_dir, LutInput::Srgb)
}

/// A film stock as the LUT Edit applies to its neutral rendering, at Edit's balance for the stock.
#[cfg(feature = "film")]
pub fn edit_film_lut(stock: &str, data_dir: &Path) -> Result<FilmLut, Error> {
    let info = FILM_STOCKS
        .iter()
        .find(|s| s.id == stock)
        .ok_or_else(|| Error::Refused(format!("unknown film stock: '{stock}'")))?;
    film_lut_with(
        stock,
        edit_balance(info.tungsten),
        data_dir,
        LutInput::NeutralRendering {
            exposure_ev: EDIT_FILM_EXPOSURE_EV,
        },
    )
}

/// A film stock as a 33-point `.cube` LUT built for `input`.
#[cfg(feature = "film")]
pub fn film_lut_with(
    stock: &str,
    balance: LutBalance,
    data_dir: &Path,
    input: LutInput,
) -> Result<FilmLut, Error> {
    use rayon::prelude::*;

    let info = FILM_STOCKS
        .iter()
        .find(|s| s.id == stock)
        .ok_or_else(|| Error::Refused(format!("unknown film stock: '{stock}'")))?;
    let data_path = ensure_film_data(data_dir)?;

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (y_shift, m_shift) = match balance {
            LutBalance::AsFilm => (0.0, 0.0),
            LutBalance::NeutralGrey => neutral_shifts(info, &data_path, input)?,
        };
        let (pipeline, print_stock) = lut_pipeline(info, y_shift, m_shift, &data_path, input)?;
        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;

        let n = FILM_LUT_SIZE;
        let level = |k: usize| k as f64 / (n - 1) as f64;
        // `.cube` order: red varies fastest, then green, then blue.
        let rows: Vec<String> = (0..n * n * n)
            .into_par_iter()
            .map(|i| {
                let o = lut_sample(
                    &pipeline,
                    &backend,
                    [level(i % n), level((i / n) % n), level(i / (n * n))],
                    input,
                );
                let c = |v: f64| v.clamp(0.0, 1.0);
                format!("{:.6} {:.6} {:.6}", c(o[0]), c(o[1]), c(o[2]))
            })
            .collect();

        let (file_name, input_line, effects_line) = match input {
            LutInput::Srgb => (
                film_lut_file_name(info.name, balance),
                "sRGB (gamma-encoded)".to_string(),
                "No grain, halation, DIR couplers or auto exposure.",
            ),
            LutInput::NeutralRendering { exposure_ev } => (
                edit_film_lut_file_name(info.name, balance),
                format!(
                    "Rollchrome's neutral rendering (sRGB), its tone curve undone; exposure {exposure_ev:+.2} EV"
                ),
                "No grain, halation or auto exposure; the DIR couplers' colour, not their edges.",
            ),
        };
        let title = file_name.strip_suffix(".cube").unwrap_or(&file_name);
        let printed = if info.is_slide {
            "scanned".to_string()
        } else {
            format!("printed on {print_stock}")
        };
        let mut cube = format!(
            "# Created by Rollchrome with spektrafilm (GPL-3.0); film data CC BY-SA 4.0, spektrafilm by Andrea Volpato\n\
             # Input: {input_line}. Output: sRGB. {id} {printed}.\n\
             # {effects_line} Enlarger Y/M shift {y_shift:.1} {m_shift:.1}.\n\
             TITLE \"{title}\"\n\
             LUT_3D_SIZE {n}\n\
             DOMAIN_MIN 0.0 0.0 0.0\n\
             DOMAIN_MAX 1.0 1.0 1.0\n",
            id = info.id,
        );
        for row in rows {
            cube.push_str(&row);
            cube.push('\n');
        }
        Ok(FilmLut {
            file_name,
            cube,
            y_shift,
            m_shift,
        })
    }))
    .unwrap_or_else(|_| {
        Err(Error::Internal(format!(
            "the film simulation failed while making the LUT for '{stock}'"
        )))
    })
}

/// Fallback stub when the film feature is disabled.
#[cfg(not(feature = "film"))]
pub fn film_lut(_stock: &str, _balance: LutBalance, _data_dir: &Path) -> Result<FilmLut, Error> {
    Err(Error::Refused(
        "film simulation is not built into this program".to_string(),
    ))
}

/// Fallback stub when the film feature is disabled.
#[cfg(not(feature = "film"))]
pub fn edit_film_lut(_stock: &str, _data_dir: &Path) -> Result<FilmLut, Error> {
    Err(Error::Refused(
        "film simulation is not built into this program".to_string(),
    ))
}

/// Fallback stub when the film feature is disabled.
#[cfg(not(feature = "film"))]
pub fn film_lut_with(
    _stock: &str,
    _balance: LutBalance,
    _data_dir: &Path,
    _input: LutInput,
) -> Result<FilmLut, Error> {
    Err(Error::Refused(
        "film simulation is not built into this program".to_string(),
    ))
}

/// Fallback stub when the film feature is disabled.
#[cfg(not(feature = "film"))]
#[allow(clippy::too_many_arguments)]
pub fn render_film(
    _linear: &[[f32; 3]],
    _width: u32,
    _height: u32,
    _stock: &str,
    _grain: Grain,
    _exposure_ev: f32,
    _output: FilmOutput,
    _backend: FilmBackend,
    _data_dir: &Path,
) -> Result<Vec<[f32; 3]>, Error> {
    Err(Error::Refused(
        "film simulation is not built into this program".to_string(),
    ))
}
