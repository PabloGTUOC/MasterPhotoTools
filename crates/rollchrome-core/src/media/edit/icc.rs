//! Colour profile handling and sRGB conversion for Edit and Batch.
//!
//! Develop writes 16-bit TIFFs in ProPhoto RGB with an embedded ICC profile.
//! Edit and Batch operate internally in sRGB. To ensure correct colours without
//! re-quantising sRGB inputs, incoming files are converted to sRGB upon decoding
//! in `decode_image` based on their embedded ICC profile.

use crate::media::edit::pipeline::ImageBuffer;
use phototools_core::error::Error;

/// EXIF corrections applied to every exported derivative of Edit and Batch.
///
/// Because all Edit and Batch outputs are converted to and encoded in sRGB,
/// copying metadata directly from a wide-gamut source (such as a 16-bit ProPhoto TIFF
/// or an Adobe RGB file) would cause the output to carry misleading colour metadata:
/// - `-ExifIFD:ColorSpace#=1` sets the standard sRGB colour space tag.
/// - `-InteropIFD:InteropIndex=` removes any stale interoperability index.
/// - `-ICC_Profile=` strips any copied wide-gamut ICC profile so viewers interpret the file as standard sRGB.
pub const SRGB_METADATA_CORRECTIONS: &[&str] = &[
    "-ExifIFD:ColorSpace#=1",
    "-InteropIFD:InteropIndex=",
    "-ICC_Profile=",
];

/// Returns the EXIF corrections as a `Vec<String>` for use with `carry_metadata_correcting`.
pub fn srgb_metadata_corrections() -> Vec<String> {
    SRGB_METADATA_CORRECTIONS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Returns `buffer` in sRGB, given the profile embedded in the file it came from.
///
/// Follows these rules in order:
/// 1. No profile: unchanged.
/// 2. A profile `moxcms` cannot parse: `Error::Refused`, reporting that the colour profile
///    could not be read and colours would be wrong.
/// 3. A profile whose data colour space is not RGB: unchanged (e.g. greyscale).
/// 4. A profile that behaves as sRGB: unchanged, decided by running a 17x17x17 grid of 8-bit
///    colours through the transform; if every output is within 1 level of input, it is sRGB.
/// 5. Anything else: convert to sRGB using `moxcms` relative colorimetric, preserving bit depth.
pub fn to_srgb(buffer: ImageBuffer, icc: Option<&[u8]>) -> Result<ImageBuffer, Error> {
    // 1. No profile: unchanged
    let Some(icc) = icc else {
        return Ok(buffer);
    };
    if icc.is_empty() {
        return Ok(buffer);
    }

    // 2. A profile moxcms cannot parse: Error::Refused
    let profile = moxcms::ColorProfile::new_from_slice(icc).map_err(|_| {
        Error::Refused(
            "The file's colour profile could not be read; colours would be wrong.".to_string(),
        )
    })?;

    // 3. A profile whose data colour space is not RGB: unchanged
    if profile.color_space != moxcms::DataColorSpace::Rgb {
        return Ok(buffer);
    }

    // 4. A profile that behaves as sRGB: unchanged
    let srgb = moxcms::ColorProfile::new_srgb();
    let options = moxcms::TransformOptions {
        rendering_intent: moxcms::RenderingIntent::RelativeColorimetric,
        prefer_fixed_point: false,
        ..Default::default()
    };

    let transform_8bit = profile
        .create_transform_8bit(moxcms::Layout::Rgb, &srgb, moxcms::Layout::Rgb, options)
        .map_err(|_| {
            Error::Refused(
                "The file's colour profile could not be read; colours would be wrong.".to_string(),
            )
        })?;

    // Run a 17x17x17 grid of 8-bit colours through the transform
    let steps: [u8; 17] = std::array::from_fn(|i| ((i as u32 * 255 + 8) / 16) as u8);
    let mut src_grid = Vec::with_capacity(17 * 17 * 17 * 3);
    for &r in &steps {
        for &g in &steps {
            for &b in &steps {
                src_grid.extend_from_slice(&[r, g, b]);
            }
        }
    }
    let mut dst_grid = vec![0u8; src_grid.len()];
    transform_8bit
        .transform(&src_grid, &mut dst_grid)
        .map_err(|_| {
            Error::Refused(
                "The file's colour profile could not be read; colours would be wrong.".to_string(),
            )
        })?;

    let (src_chunks, _) = src_grid.as_chunks::<3>();
    let (dst_chunks, _) = dst_grid.as_chunks::<3>();
    let behaves_as_srgb = src_chunks.iter().zip(dst_chunks.iter()).all(|(s, d)| {
        (s[0] as i16 - d[0] as i16).abs() <= 1
            && (s[1] as i16 - d[1] as i16).abs() <= 1
            && (s[2] as i16 - d[2] as i16).abs() <= 1
    });

    if behaves_as_srgb {
        return Ok(buffer);
    }

    // 5. Anything else: convert to sRGB with moxcms, relative colorimetric. Keep the bit depth.
    match buffer {
        ImageBuffer::Rgb8 {
            width,
            height,
            data,
        } => {
            let mut converted = vec![0u8; data.len()];
            transform_8bit
                .transform(&data, &mut converted)
                .map_err(|e| Error::Internal(format!("colour transform failed: {e:?}")))?;
            Ok(ImageBuffer::Rgb8 {
                width,
                height,
                data: converted,
            })
        }
        ImageBuffer::Rgb16 {
            width,
            height,
            data,
        } => {
            let transform_16bit = profile
                .create_transform_16bit(moxcms::Layout::Rgb, &srgb, moxcms::Layout::Rgb, options)
                .map_err(|_| {
                    Error::Refused(
                        "The file's colour profile could not be read; colours would be wrong."
                            .to_string(),
                    )
                })?;
            let mut converted = vec![0u16; data.len()];
            transform_16bit
                .transform(&data, &mut converted)
                .map_err(|e| Error::Internal(format!("colour transform failed: {e:?}")))?;
            Ok(ImageBuffer::Rgb16 {
                width,
                height,
                data: converted,
            })
        }
    }
}
