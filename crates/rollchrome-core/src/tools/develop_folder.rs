//! Filesystem adapter for Develop folder operations (DV-2b).
//!
//! Implements `DevelopStore` over standard filesystem directories. This is the
//! single place in this step where `Path` and `PathBuf` types are used (R2).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use phototools_core::error::Error;
use phototools_core::media::ExifWriter;

use crate::media::develop::{Look, OutputFormat};
use crate::tools::develop::DevelopStore;
use crate::tools::lut_library::find_lut_by_name_or_sha256;

/// Storage adapter for developing a physical folder on disk.
pub struct FolderStore {
    folder: PathBuf,
    lut_dir: PathBuf,
    film_data_dir: Option<PathBuf>,
    exiftool_program: Option<String>,
    writer: Option<ExifWriter>,
    /// A chosen output folder (DV-7); `None` is `<folder>/Developed`.
    custom_output: Option<PathBuf>,
}

impl FolderStore {
    /// Creates a store targeting `folder` with LUTs resolved from `lut_dir`.
    pub fn new(folder: PathBuf, lut_dir: PathBuf) -> Self {
        Self::with_film_data(folder, lut_dir, None)
    }

    /// Creates a store targeting `folder` with LUTs and film simulation data.
    pub fn with_film_data(
        folder: PathBuf,
        lut_dir: PathBuf,
        film_data_dir: Option<PathBuf>,
    ) -> Self {
        Self::with_exiftool_and_film_data(folder, lut_dir, film_data_dir, None)
    }

    /// Creates a store with a specific exiftool binary (used for single-process G4 tests).
    pub fn with_exiftool(folder: PathBuf, lut_dir: PathBuf, program: Option<String>) -> Self {
        Self::with_exiftool_and_film_data(folder, lut_dir, None, program)
    }

    /// Creates a store with exiftool binary and film data directory.
    pub fn with_exiftool_and_film_data(
        folder: PathBuf,
        lut_dir: PathBuf,
        film_data_dir: Option<PathBuf>,
        program: Option<String>,
    ) -> Self {
        Self {
            folder,
            lut_dir,
            film_data_dir,
            exiftool_program: program,
            writer: None,
            custom_output: None,
        }
    }

    /// Writes into `dir` instead of `<folder>/Developed` (DV-7). The caller has resolved `dir`
    /// against the roots (G6) and refused a card (G5).
    pub fn with_output_dir(mut self, dir: PathBuf) -> Self {
        self.custom_output = Some(dir);
        self
    }

    /// The destination folder for rendered outputs (`<folder>/Developed` unless one was chosen).
    pub fn output_dir(&self) -> PathBuf {
        match &self.custom_output {
            Some(dir) => dir.clone(),
            None => self.folder.join("Developed"),
        }
    }

    /// Read existing output file names in the `Developed/` directory in lower case.
    pub fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        let dev_dir = self.output_dir();
        if !dev_dir.exists() {
            return Ok(BTreeSet::new());
        }

        let entries = std::fs::read_dir(&dev_dir).map_err(Error::Io)?;

        let mut existing = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(Error::Io)?;
            if let Ok(ft) = entry.file_type() {
                if ft.is_file() {
                    existing.insert(entry.file_name().to_string_lossy().to_lowercase());
                }
            }
        }
        Ok(existing)
    }

    fn ensure_writer(&mut self) -> Result<&mut ExifWriter, Error> {
        if self.writer.is_none() {
            let writer = match &self.exiftool_program {
                Some(prog) => ExifWriter::start_with(prog, Duration::from_secs(60))?,
                None => ExifWriter::start()?,
            };
            self.writer = Some(writer);
        }
        match self.writer.as_mut() {
            Some(w) => Ok(w),
            None => Err(Error::Internal(
                "failed to access exiftool writer".to_string(),
            )),
        }
    }
}

impl DevelopStore for FolderStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        let dev_dir = self.output_dir();
        if !dev_dir.exists() {
            return Ok(BTreeSet::new());
        }

        let entries = std::fs::read_dir(&dev_dir).map_err(Error::Io)?;

        let mut existing = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(Error::Io)?;
            if let Ok(ft) = entry.file_type() {
                if ft.is_file() {
                    existing.insert(entry.file_name().to_string_lossy().to_lowercase());
                }
            }
        }
        Ok(existing)
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        let path = self.folder.join(name);
        std::fs::read(&path).map_err(Error::Io)
    }

    fn look(&self, look: &str) -> Result<Look, Error> {
        match find_lut_by_name_or_sha256(&self.lut_dir, look)? {
            Some((_path, lut)) => Ok(Look::srgb(lut)),
            None => Err(Error::Refused(format!(
                "the look '{look}' is not in the LUT library"
            ))),
        }
    }

    fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error> {
        let dev_dir = self.output_dir();
        std::fs::create_dir_all(&dev_dir).map_err(Error::Io)?;

        let path = dev_dir.join(output);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::Refused(match &self.custom_output {
                    None => format!("Developed/{output} already exists"),
                    Some(dir) => format!("{output} already exists in {}", dir.display()),
                }));
            }
            Err(e) => return Err(Error::Io(e)),
        };

        if let Err(e) = file.write_all(bytes) {
            let _ = std::fs::remove_file(&path);
            return Err(Error::Io(e));
        }

        if let Err(e) = file.sync_all() {
            let _ = std::fs::remove_file(&path);
            return Err(Error::Io(e));
        }

        Ok(())
    }

    fn carry_metadata(
        &mut self,
        source: &str,
        output: &str,
        width: u32,
        height: u32,
        format: OutputFormat,
    ) -> Result<(), Error> {
        let src_path = self.folder.join(source);
        let dst_path = self.output_dir().join(output);

        let writer = self.ensure_writer()?;

        // 1. Copy metadata to derivative.
        // upright is false: pixels are stored as captured (DV-1 reports orientation and does not apply it),
        // so the original Orientation tag remains valid and accurate.
        writer.copy_metadata_to_derivative(&src_path, &dst_path, width, height, false)?;

        // 2. Format-specific and DNG tag corrections in a single set_tags call:
        let mut corrections = Vec::new();
        match format {
            OutputFormat::Jpeg { .. } => {
                // JPEG is rendered in sRGB; tag with ExifIFD:ColorSpace=1 (sRGB)
                corrections.push("-ExifIFD:ColorSpace#=1".to_string());
            }
            OutputFormat::Tiff => {
                // TIFF is 16-bit ProPhoto RGB with an embedded ICC profile.
                // ExifIFD:ColorSpace=65535 (Uncalibrated) signals that viewers must consult the
                // embedded ICC profile instead of assuming sRGB.
                corrections.push("-ExifIFD:ColorSpace#=65535".to_string());
                // Delete InteropIFD:InteropIndex (e.g. "R98") which would incorrectly assert sRGB.
                corrections.push("-InteropIFD:InteropIndex=".to_string());
            }
        }

        // Delete DNG identification tags so TIFF/JPEG derivatives are not mistaken for sensor RAW DNGs.
        corrections.push("-IFD0:DNGVersion=".to_string());
        corrections.push("-IFD0:DNGBackwardVersion=".to_string());

        writer.set_tags(&dst_path, &corrections)?;

        Ok(())
    }

    fn film_data_dir(&self) -> Option<&Path> {
        self.film_data_dir.as_deref()
    }

    fn finish(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.close();
        }
    }
}

/// G5: Develop writes beside its RAW files, so a folder on a card is refused before anything runs.
pub fn refuse_card_folder<F>(folder: &Path, resolve_root: F) -> Result<(), Error>
where
    F: Fn(&Path) -> Option<PathBuf>,
{
    if crate::tools::edit::is_card_volume_with_resolver(folder, resolve_root) {
        return Err(Error::Refused(
            "Cannot develop on a card volume: card media is read-only (G5)".to_string(),
        ));
    }
    Ok(())
}

/// Returns the list of RAW file names in `folder`, non-recursive, sorted by name (RP-1).
pub fn raw_names(folder: &Path) -> Result<Vec<String>, Error> {
    if !folder.is_dir() {
        return Err(Error::Refused(format!(
            "{} is not a directory",
            folder.display()
        )));
    }
    let entries = std::fs::read_dir(folder).map_err(Error::Io)?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(Error::Io)?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if phototools_core::media::raw::RAW_EXTENSIONS
                    .contains(&ext.to_lowercase().as_str())
                {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Generates an upright thumbnail JPEG for a RAW file, fit to `max_edge` (RP-1).
pub fn thumbnail(path: &Path, max_edge: u32) -> Result<Vec<u8>, Error> {
    let derived = phototools_core::media::raw::raw_to_jpeg(path)?;
    let dyn_img = image::load_from_memory(&derived.bytes)
        .map_err(|e| Error::Internal(format!("failed to decode raw preview JPEG: {e}")))?;
    let orientation = phototools_core::media::meta::read_meta(path)
        .map(|m| m.orientation)
        .unwrap_or(phototools_core::media::meta::Orientation::Normal);
    let upright = phototools_core::media::image_ops::apply_orientation(dyn_img, orientation);
    let scaled = phototools_core::media::image_ops::downscale_to_max_edge(&upright, max_edge)?;
    let opts = phototools_core::media::jpeg::JpegOptions::deliverable(85);
    phototools_core::media::jpeg::encode(&scaled, &opts)
}
