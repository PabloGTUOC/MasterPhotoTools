//! Develop preview generation and fast look re-rendering (RP-1).

use fast_image_resize as fr;
use phototools_core::error::Error;

use crate::media::develop::render::{render, Look, OutputFormat, Rendered};
use crate::media::develop::{develop, DevelopOptions, Developed};
use crate::media::segment::to_upright;

pub const PREVIEW_MAX_EDGE: u32 = 1600;

/// Reduced-resolution developed RAW representation for interactive previewing.
///
/// Keeps a 16-bit ProPhoto RGB image reduced to fit the long edge (at most `PREVIEW_MAX_EDGE`),
/// with orientation already applied (upright). Changing the look re-renders from this
/// cached buffer without re-demosaicing or re-developing the RAW.
pub struct DevelopPreview {
    developed: Developed,
}

impl DevelopPreview {
    /// Develops raw file bytes to a reduced upright preview image.
    pub fn new(raw: &[u8], exposure_ev: f32, max_edge: u32) -> Result<Self, Error> {
        let dev = develop(raw, &DevelopOptions { exposure_ev })?;
        Self::from_developed(dev, max_edge)
    }

    /// Creates a reduced upright preview from an already developed image.
    pub fn from_developed(developed: Developed, max_edge: u32) -> Result<Self, Error> {
        // Apply EXIF orientation to ensure pixels are stored upright
        let upright = to_upright(&developed.image, developed.orientation as u32);
        let w = upright.width();
        let h = upright.height();
        let long = w.max(h);

        let final_image = if max_edge > 0 && long > max_edge {
            let ratio = max_edge as f64 / long as f64;
            let new_w = ((w as f64 * ratio).floor() as u32).max(1);
            let new_h = ((h as f64 * ratio).floor() as u32).max(1);

            let raw_u16 = upright.as_raw();
            let byte_slice = unsafe {
                std::slice::from_raw_parts(raw_u16.as_ptr() as *const u8, raw_u16.len() * 2)
            };
            let src = fr::images::ImageRef::new(w, h, byte_slice, fr::PixelType::U16x3)
                .map_err(|e| Error::Internal(format!("resize preview source: {e:?}")))?;
            let mut dst = fr::images::Image::new(new_w, new_h, fr::PixelType::U16x3);
            fr::Resizer::new()
                .resize(&src, &mut dst, None)
                .map_err(|e| Error::Internal(format!("resize preview: {e:?}")))?;

            let dst_bytes = dst.into_vec();
            let mut out16 = Vec::with_capacity(dst_bytes.len() / 2);
            for chunk in dst_bytes.as_chunks::<2>().0.iter() {
                out16.push(u16::from_ne_bytes([chunk[0], chunk[1]]));
            }

            image::ImageBuffer::from_raw(new_w, new_h, out16).ok_or_else(|| {
                Error::Internal("failed to create preview ImageBuffer from resized bytes".into())
            })?
        } else {
            upright
        };

        Ok(Self {
            developed: Developed {
                image: final_image,
                orientation: 1,
                demosaic: developed.demosaic,
            },
        })
    }

    /// Upright, JPEG quality 90, with the look if any. Does not develop the RAW again.
    pub fn render(&self, look: Option<&Look>) -> Result<Rendered, Error> {
        render(&self.developed, look, OutputFormat::Jpeg { quality: 90 })
    }

    pub fn width(&self) -> u32 {
        self.developed.image.width()
    }

    pub fn height(&self) -> u32 {
        self.developed.image.height()
    }
}
