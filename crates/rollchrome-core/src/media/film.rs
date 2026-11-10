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
}

/// All 18 supported film stocks in defined order (DV-4).
pub static FILM_STOCKS: &[FilmStockInfo] = &[
    // Kodak (13)
    FilmStockInfo {
        id: "kodak_portra_160",
        name: "Kodak Portra 160",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_portra_400",
        name: "Kodak Portra 400",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_portra_800",
        name: "Kodak Portra 800",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_ektar_100",
        name: "Kodak Ektar 100",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_gold_200",
        name: "Kodak Gold 200",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_ultramax_400",
        name: "Kodak Ultramax 400",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_ektachrome_100",
        name: "Kodak Ektachrome 100",
        maker: "Kodak",
        is_slide: true,
    },
    FilmStockInfo {
        id: "kodak_kodachrome_64",
        name: "Kodak Kodachrome 64",
        maker: "Kodak",
        is_slide: true,
    },
    FilmStockInfo {
        id: "kodak_vision3_50d",
        name: "Kodak Vision3 50D",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_vision3_200t",
        name: "Kodak Vision3 200T",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_vision3_250d",
        name: "Kodak Vision3 250D",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_vision3_500t",
        name: "Kodak Vision3 500T",
        maker: "Kodak",
        is_slide: false,
    },
    FilmStockInfo {
        id: "kodak_verita_200d",
        name: "Kodak Verita 200D",
        maker: "Kodak",
        is_slide: false,
    },
    // Fujifilm (5)
    FilmStockInfo {
        id: "fujifilm_c200",
        name: "Fujifilm C200",
        maker: "Fujifilm",
        is_slide: false,
    },
    FilmStockInfo {
        id: "fujifilm_pro_400h",
        name: "Fujifilm Pro 400H",
        maker: "Fujifilm",
        is_slide: false,
    },
    FilmStockInfo {
        id: "fujifilm_xtra_400",
        name: "Fujifilm X-Tra 400",
        maker: "Fujifilm",
        is_slide: false,
    },
    FilmStockInfo {
        id: "fujifilm_provia_100f",
        name: "Fujifilm Provia 100F",
        maker: "Fujifilm",
        is_slide: true,
    },
    FilmStockInfo {
        id: "fujifilm_velvia_100",
        name: "Fujifilm Velvia 100",
        maker: "Fujifilm",
        is_slide: true,
    },
];

/// Lists all supported film stocks as BuiltinLookInfo items.
pub fn film_look_list() -> Vec<BuiltinLookInfo> {
    FILM_STOCKS
        .iter()
        .map(|s| BuiltinLookInfo {
            id: format!("film:{}", s.id),
            name: s.name.to_string(),
            maker: s.maker.to_string(),
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
