//! Masks made by a model, kept as pixels (ED-23).
//!
//! An automatic mask is computed once from the photograph and **stored**, not recomputed:
//! inference is not guaranteed to repeat bit for bit across machines or runtime versions,
//! and an export must match what was reviewed (edit plan, Round 3, decision 5). It is kept
//! in the sidecar as an 8-bit greyscale PNG, base64-encoded, at most 1024 px on the long
//! edge, in the **stored** frame like every mask, and sampled bilinearly at any size.

use crate::error::Error;
use serde::{Deserialize, Serialize};

/// The long edge an automatic mask is stored at: the models see 1024 px or less, so more
/// would add bytes to the sidecar and no detail.
pub const STORED_MASK_EDGE: u32 = 1024;

/// A mask's pixels as the sidecar holds them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredRaster {
    pub width: u32,
    pub height: u32,
    /// 8-bit greyscale PNG, base64 (standard alphabet, padded).
    pub png: String,
    /// The model that made it, so a mask can be traced to what produced it.
    #[serde(default)]
    pub model_sha256: String,
}

impl StoredRaster {
    /// Encodes `data` (row-major, one byte per pixel) as a stored raster.
    pub fn encode(width: u32, height: u32, data: &[u8], model_sha256: &str) -> Result<Self, Error> {
        if data.len() != (width as usize) * (height as usize) {
            return Err(Error::Refused(format!(
                "a {width}x{height} mask needs {} bytes, not {}",
                width as usize * height as usize,
                data.len()
            )));
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(data, width, height, image::ExtendedColorType::L8)
            .map_err(|e| Error::Internal(format!("could not encode the mask: {e}")))?;
        Ok(Self {
            width,
            height,
            png: base64_encode(&png),
            model_sha256: model_sha256.to_string(),
        })
    }

    /// Decodes the pixels, refusing a raster whose bytes do not match its stated size: a
    /// sidecar edited by hand or truncated must say so rather than mask the wrong place.
    pub fn decode(&self) -> Result<CoverageRaster, Error> {
        let bytes = base64_decode(&self.png)
            .ok_or_else(|| Error::Refused("the stored mask is not valid base64".into()))?;
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|e| Error::Refused(format!("the stored mask is not a readable PNG: {e}")))?
            .to_luma8();
        if img.width() != self.width || img.height() != self.height {
            return Err(Error::Refused(format!(
                "the stored mask is {}x{}, but says {}x{}",
                img.width(),
                img.height(),
                self.width,
                self.height
            )));
        }
        Ok(CoverageRaster {
            w: self.width,
            h: self.height,
            data: img.into_raw(),
        })
    }
}

use image::ImageEncoder;

/// Decoded coverage, 0–255, over the stored frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRaster {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl CoverageRaster {
    /// Coverage in [0, 1] at stored-normalised `(u, v)`, sampled bilinearly.
    #[inline(always)]
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let (w, h) = (self.w as usize, self.h as usize);
        if w == 0 || h == 0 {
            return 0.0;
        }
        let x = (u * self.w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (v * self.h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |xx: usize, yy: usize| self.data[yy * w + xx] as f32;
        let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
        let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
        (top * (1.0 - fy) + bottom * fy) / 255.0
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. Twenty lines here rather than a dependency for them (G8).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard padded base64; `None` for anything else.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| ALPHABET.iter().position(|&a| a == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for (i, chunk) in text.chunks(4).enumerate() {
        let last = i == text.len() / 4 - 1;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (j, &c) in chunk.iter().enumerate() {
            let v = if j >= 4 - pad { 0 } else { value(c)? };
            n = n << 6 | v;
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_length_and_matches_the_standard() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
        for n in 0..300 {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(
                base64_decode(&base64_encode(&bytes)).unwrap(),
                bytes,
                "length {n}"
            );
        }
        assert!(base64_decode("TQ=").is_none());
        assert!(base64_decode("T!==").is_none());
        assert!(
            base64_decode("TQ==TWFu").is_none(),
            "padding only at the end"
        );
    }

    #[test]
    fn a_stored_mask_round_trips_through_its_png() {
        let (w, h) = (40u32, 30u32);
        let data: Vec<u8> = (0..w * h).map(|i| (i % 256) as u8).collect();
        let stored = StoredRaster::encode(w, h, &data, "abc").unwrap();
        let back = stored.decode().unwrap();
        assert_eq!((back.w, back.h), (w, h));
        assert_eq!(back.data, data);
    }

    #[test]
    fn a_stored_mask_that_lies_about_its_size_is_refused() {
        let mut stored = StoredRaster::encode(8, 8, &[200; 64], "").unwrap();
        stored.width = 9;
        assert!(stored
            .decode()
            .unwrap_err()
            .to_string()
            .contains("says 9x8"));
        stored.png = "not base64!".into();
        assert!(stored.decode().is_err());
    }

    #[test]
    fn sampling_is_bilinear_and_clamped_at_the_edges() {
        let r = CoverageRaster {
            w: 2,
            h: 1,
            data: vec![0, 255],
        };
        assert_eq!(r.sample(0.0, 0.5), 0.0);
        assert_eq!(r.sample(1.0, 0.5), 1.0);
        assert!((r.sample(0.5, 0.5) - 0.5).abs() < 1e-6);
    }
}
