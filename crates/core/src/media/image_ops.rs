use crate::error::Error;
use fast_image_resize as fr;
use image::{DynamicImage, ImageReader, RgbaImage};
use std::path::Path;

pub fn decode(path: &Path) -> Result<DynamicImage, Error> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let img = reader
        .decode()
        .map_err(|e| Error::Internal(format!("Decode failed: {}", e)))?;
    Ok(img)
}

pub fn resize(img: &DynamicImage, new_w: u32, new_h: u32) -> Result<DynamicImage, Error> {
    if new_w == 0 || new_h == 0 {
        return Ok(img.clone());
    }

    let width = img.width();
    let height = img.height();

    // Convert to RGBA8 for resizing
    let rgba8 = img.to_rgba8();

    let src_image =
        fr::images::Image::from_vec_u8(width, height, rgba8.into_raw(), fr::PixelType::U8x4)
            .map_err(|e| Error::Internal(format!("fast_image_resize error: {:?}", e)))?;

    let dst_width = new_w;
    let dst_height = new_h;
    let mut dst_image = fr::images::Image::new(dst_width, dst_height, fr::PixelType::U8x4);

    let mut resizer = fr::Resizer::new();
    resizer
        .resize(&src_image, &mut dst_image, None)
        .map_err(|e| Error::Internal(format!("fast_image_resize resize error: {:?}", e)))?;

    let result = RgbaImage::from_raw(new_w, new_h, dst_image.into_vec()).unwrap();
    Ok(DynamicImage::ImageRgba8(result))
}

pub fn encode_jpeg(img: &DynamicImage, quality: u8, path: &Path) -> Result<(), Error> {
    let mut file = std::fs::File::create(path)?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
    encoder
        .encode_image(img)
        .map_err(|e| Error::Internal(format!("Encode failed: {}", e)))?;
    Ok(())
}
