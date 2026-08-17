use image::{ImageBuffer, RgbImage};
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

pub struct Fixtures {
    pub temp: TempDir,
}

#[allow(dead_code)]
impl Fixtures {
    pub fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }

    pub fn jpeg_with_exif(
        &self,
        name: &str,
        w: u32,
        h: u32,
        capture: &str,
        camera: &str,
    ) -> PathBuf {
        let path = self.temp.path().join(name);
        let img: RgbImage = ImageBuffer::new(w, h);
        img.save(&path).unwrap();

        Command::new("exiftool")
            .args([
                "-overwrite_original",
                &format!("-DateTimeOriginal={}", capture),
                &format!("-Model={}", camera),
                path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

        path
    }

    pub fn jpeg_without_exif(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let path = self.temp.path().join(name);
        let img: RgbImage = ImageBuffer::new(w, h);
        img.save(&path).unwrap();
        path
    }

    pub fn card_tree(&self, shots: u32) -> PathBuf {
        let root = self.temp.path().join("card");
        let dcim = root.join("DCIM").join("100APPLE");
        std::fs::create_dir_all(&dcim).unwrap();

        for i in 0..shots {
            let stem = format!("IMG_{:04}", i);

            // Raw
            let raw_path = dcim.join(format!("{}.CR2", stem));
            std::fs::write(&raw_path, "RAW_DATA").unwrap();

            // JPEG
            let jpg_path = dcim.join(format!("{}.JPG", stem));
            std::fs::write(&jpg_path, "JPEG_DATA").unwrap();
        }

        // Add a junk file
        let junk_path = root.join(".DS_Store");
        std::fs::write(&junk_path, "JUNK").unwrap();

        // Add a duplicate
        let dup_path = dcim.join("IMG_DUP.JPG");
        std::fs::write(&dup_path, "JPEG_DATA").unwrap();

        root
    }
}
