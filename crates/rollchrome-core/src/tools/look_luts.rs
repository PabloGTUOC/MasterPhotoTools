//! Every look as a library LUT, made on demand (UI-3).
//!
//! Converts built-in camera looks (fitted in ProPhoto space) and physical film simulations
//! into sRGB-domain `.cube` LUTs stored directly in the managed LUT library.
//! Once generated, they behave as standard library LUTs across Edit, Batch and export.

use crate::media::edit::lut::Lut;
use crate::media::edit::pipeline::LutRef;
use phototools_core::error::Error;
use std::path::Path;

/// Atomically writes a `.cube` text into `dest_path` inside `library_dir`.
fn atomic_write_lut(library_dir: &Path, dest_path: &Path, bytes: &[u8]) -> Result<(), Error> {
    use std::io::Write;
    std::fs::create_dir_all(library_dir)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("lut");
    let tmp_path = library_dir.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));

    let mut f = std::fs::File::create(&tmp_path)?;
    f.write_all(bytes)?;
    f.flush()?;
    f.sync_all()?;
    drop(f);

    std::fs::rename(&tmp_path, dest_path)?;
    Ok(())
}

/// Transforms an sRGB code value through a ProPhoto look's LUT using the exact
/// transform pipeline from `render.rs`.
#[inline]
pub(crate) fn transform_srgb_through_prophoto_look(
    srgb_in: [f32; 3],
    look: &crate::media::develop::render::Look,
    m_srgb_to_pro: [[f32; 3]; 3],
    m_pro_to_srgb: [[f32; 3]; 3],
) -> [f32; 3] {
    // 1. Decode sRGB code value to linear sRGB
    let s_lin = [
        crate::media::develop::render::srgb_to_linear(srgb_in[0]),
        crate::media::develop::render::srgb_to_linear(srgb_in[1]),
        crate::media::develop::render::srgb_to_linear(srgb_in[2]),
    ];

    // 2. Convert linear sRGB to linear ProPhoto
    let lin_pro = crate::media::develop::colour::mul_3x3_vec(m_srgb_to_pro, s_lin);

    // 3. Encode to the space and encoding the LUT expects (ROMM ProPhoto)
    let in_pro = [
        crate::media::develop::tone::prophoto_encode(lin_pro[0].max(0.0)),
        crate::media::develop::tone::prophoto_encode(lin_pro[1].max(0.0)),
        crate::media::develop::tone::prophoto_encode(lin_pro[2].max(0.0)),
    ];

    // 4. Sample the LUT
    let sampled = look.lut.sample(in_pro);
    let pro_enc = [
        sampled[0].clamp(0.0, 1.0),
        sampled[1].clamp(0.0, 1.0),
        sampled[2].clamp(0.0, 1.0),
    ];

    // 5. Decode ProPhoto gamma back to linear ProPhoto
    let r_lin = crate::media::develop::tone::prophoto_decode(pro_enc[0]);
    let g_lin = crate::media::develop::tone::prophoto_decode(pro_enc[1]);
    let b_lin = crate::media::develop::tone::prophoto_decode(pro_enc[2]);

    // 6. Convert linear ProPhoto back to linear sRGB
    let srgb_lin = crate::media::develop::colour::mul_3x3_vec(m_pro_to_srgb, [r_lin, g_lin, b_lin]);

    // 7. Encode linear sRGB to sRGB code value
    [
        crate::media::develop::render::linear_to_srgb(srgb_lin[0]),
        crate::media::develop::render::linear_to_srgb(srgb_lin[1]),
        crate::media::develop::render::linear_to_srgb(srgb_lin[2]),
    ]
}

/// Bakes a 65³ sRGB-domain `.cube` representation of a ProPhoto built-in look.
///
/// 65 points, not 33: the look's own LUT is gridded in ProPhoto-encoded values, so resampling it onto an
/// sRGB grid loses detail between the points. At 33 the baked cube missed Develop's renderer by up to 6.6
/// code values between grid points; at 65 by 0.7, within Develop's own 8-bit rounding.
pub(crate) fn bake_builtin_look_cube(
    look: &crate::media::develop::render::Look,
    title: &str,
) -> String {
    use rayon::prelude::*;
    let n = 65usize;
    let m_srgb_to_pro = crate::media::develop::colour::srgb_to_prophoto_matrix();
    let m_pro_to_srgb = crate::media::develop::colour::prophoto_to_srgb_matrix();

    let level = |k: usize| k as f32 / (n - 1) as f32;
    // .cube order: red varies fastest, then green, then blue
    let rows: Vec<String> = (0..n * n * n)
        .into_par_iter()
        .map(|i| {
            let r = level(i % n);
            let g = level((i / n) % n);
            let b = level(i / (n * n));

            let out =
                transform_srgb_through_prophoto_look([r, g, b], look, m_srgb_to_pro, m_pro_to_srgb);
            let c = |v: f32| v.clamp(0.0, 1.0);
            format!("{:.6} {:.6} {:.6}", c(out[0]), c(out[1]), c(out[2]))
        })
        .collect();

    let mut cube = format!(
        "# Created by Rollchrome; sRGB-baked camera look\n\
         TITLE \"{title}\"\n\
         LUT_3D_SIZE {n}\n\
         DOMAIN_MIN 0.0 0.0 0.0\n\
         DOMAIN_MAX 1.0 1.0 1.0\n"
    );
    for row in rows {
        cube.push_str(&row);
        cube.push('\n');
    }
    cube
}

/// Ensures a camera look (`builtin:<id>`) or film simulation (`film:<stock>`) exists
/// in `library_dir` as an sRGB-domain `.cube` LUT file, writing it on demand if missing.
///
/// Returns the `LutRef` pointing to the library LUT.
pub fn ensure_look_lut(
    look_id: &str,
    library_dir: &Path,
    film_data_dir: &Path,
) -> Result<LutRef, Error> {
    std::fs::create_dir_all(library_dir)?;

    if let Some(builtin_id) = look_id.strip_prefix("builtin:") {
        let entry = crate::media::looks::builtin::builtin_looks()
            .into_iter()
            .find(|l| l.id == builtin_id)
            .ok_or_else(|| {
                Error::Refused(format!("the built-in look '{builtin_id}' does not exist"))
            })?;
        let file_name =
            crate::media::looks::builtin::builtin_lut_file_name(entry.maker, entry.display_name);
        let dest_path = library_dir.join(&file_name);
        if dest_path.exists() {
            let lut = Lut::from_file(&dest_path)?;
            return Ok(LutRef {
                name: file_name,
                sha256: lut.sha256,
            });
        }
        let look = crate::media::looks::builtin::builtin_look(builtin_id)?;
        let title = file_name.strip_suffix(".cube").unwrap_or(&file_name);
        let cube_text = bake_builtin_look_cube(&look, title);
        atomic_write_lut(library_dir, &dest_path, cube_text.as_bytes())?;
        let lut = Lut::from_file(&dest_path)?;
        Ok(LutRef {
            name: file_name,
            sha256: lut.sha256,
        })
    } else if let Some(stock) = look_id.strip_prefix("film:") {
        #[cfg(feature = "film")]
        {
            let lut_data = crate::media::film::edit_film_lut(stock, film_data_dir)?;
            let file_name = lut_data.file_name;
            let dest_path = library_dir.join(&file_name);
            if dest_path.exists() {
                let lut = Lut::from_file(&dest_path)?;
                return Ok(LutRef {
                    name: file_name,
                    sha256: lut.sha256,
                });
            }
            atomic_write_lut(library_dir, &dest_path, lut_data.cube.as_bytes())?;
            let lut = Lut::from_file(&dest_path)?;
            Ok(LutRef {
                name: file_name,
                sha256: lut.sha256,
            })
        }
        #[cfg(not(feature = "film"))]
        {
            let _ = (stock, film_data_dir);
            Err(Error::Refused(
                "film simulation is not built into this program".to_string(),
            ))
        }
    } else {
        Err(Error::Refused(format!("unknown look id: '{look_id}'")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_builtin_look_becomes_an_srgb_library_lut() {
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("luts");
        let film_dir = temp.path().join("film");

        let lut_ref1 = ensure_look_lut("builtin:fujifilm-classic-chrome", &lib_dir, &film_dir)
            .expect("ensure look LUT");
        let dest_file = lib_dir.join(&lut_ref1.name);
        assert!(dest_file.exists());
        assert_eq!(lut_ref1.name, "Builtin - Fujifilm Classic Chrome.cube");

        let parsed = Lut::from_file(&dest_file).expect("parse baked LUT");
        assert_eq!(parsed.sha256, lut_ref1.sha256);
        assert_eq!(parsed.size, 65);

        let mtime_before = std::fs::metadata(&dest_file).unwrap().modified().unwrap();

        // Small sleep to ensure filesystem timestamp granularity would advance if touched
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Second call must return the exact same LutRef without rewriting
        let lut_ref2 = ensure_look_lut("builtin:fujifilm-classic-chrome", &lib_dir, &film_dir)
            .expect("second ensure look LUT");
        assert_eq!(lut_ref1, lut_ref2);

        let mtime_after = std::fs::metadata(&dest_file).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "existing LUT file must not be rewritten"
        );
    }

    /// The grid test above compares the bake with the function that made it, at the cube's own grid points,
    /// so it can only catch a file that was written or read wrongly. This one compares against Develop's
    /// renderer itself, `render_srgb8_image` with the look, at 64 colours that fall between grid points,
    /// where the baked cube has to interpolate.
    #[test]
    fn a_baked_builtin_look_matches_develops_renderer_between_grid_points() {
        use crate::media::develop::{render::render_srgb8_image, Demosaic, Developed};
        let temp = tempfile::tempdir().expect("tempdir");
        let lib_dir = temp.path().join("luts");
        std::fs::create_dir_all(&lib_dir).expect("lib dir");
        let look_id = "fujifilm-classic-chrome";
        let lut_ref = ensure_look_lut(
            &format!("builtin:{look_id}"),
            &lib_dir,
            &temp.path().join("film"),
        )
        .expect("ensure look LUT");
        let baked = Lut::from_file(&lib_dir.join(&lut_ref.name)).expect("load baked lut");
        let look = crate::media::looks::builtin::builtin_look(look_id).expect("builtin look");

        // Off the 65-point grid (spacing 1/64): 0.1 is 6.4 steps, 0.37 is 23.68, and so on.
        let levels = [0.1f32, 0.37, 0.61, 0.88];
        let m = crate::media::develop::colour::srgb_to_prophoto_matrix();
        let mut colours = Vec::new();
        let mut pixels = Vec::new();
        for &r in &levels {
            for &g in &levels {
                for &b in &levels {
                    let lin = [r, g, b].map(crate::media::develop::render::srgb_to_linear);
                    let pro = crate::media::develop::colour::mul_3x3_vec(m, lin);
                    for v in pro {
                        let enc = crate::media::develop::tone::prophoto_encode(v.max(0.0));
                        pixels.push((enc * 65535.0).round().clamp(0.0, 65535.0) as u16);
                    }
                    colours.push([r, g, b]);
                }
            }
        }
        let developed = Developed {
            image: image::ImageBuffer::from_raw(colours.len() as u32, 1, pixels).expect("buffer"),
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };
        let rendered = render_srgb8_image(&developed, Some(&look));

        let mut worst = 0.0f32;
        for (i, srgb) in colours.iter().enumerate() {
            let develop = rendered.get_pixel(i as u32, 0).0.map(|v| v as f32 / 255.0);
            let edit = baked.sample(*srgb);
            for c in 0..3 {
                worst = worst.max((edit[c] - develop[c]).abs());
            }
        }
        // The brief's 1/255, Develop's own 8-bit rounding included (measured 0.68 at 65 points).
        assert!(
            worst <= 1.0 / 255.0,
            "the baked cube differs from Develop's renderer by {} code values",
            worst * 255.0
        );
    }

    #[test]
    fn a_baked_builtin_look_matches_develop_on_the_grid() {
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("luts");
        let film_dir = temp.path().join("film");

        let look_id = "fujifilm-classic-chrome";
        let lut_ref = ensure_look_lut(&format!("builtin:{look_id}"), &lib_dir, &film_dir)
            .expect("ensure look LUT");
        let baked_lut = Lut::from_file(&lib_dir.join(&lut_ref.name)).expect("load baked lut");
        let prophoto_look =
            crate::media::looks::builtin::builtin_look(look_id).expect("load builtin look");

        let m_srgb_to_pro = crate::media::develop::colour::srgb_to_prophoto_matrix();
        let m_pro_to_srgb = crate::media::develop::colour::prophoto_to_srgb_matrix();

        // Test 64 sRGB colours on the 65³ grid (4x4x4 grid at indices [0, 16, 32, 64])
        let grid_indices = [0usize, 16, 32, 64];
        for &r_i in &grid_indices {
            for &g_i in &grid_indices {
                for &b_i in &grid_indices {
                    let srgb_in = [r_i as f32 / 64.0, g_i as f32 / 64.0, b_i as f32 / 64.0];

                    // Direct transform as render.rs applies it
                    let develop_expected = transform_srgb_through_prophoto_look(
                        srgb_in,
                        &prophoto_look,
                        m_srgb_to_pro,
                        m_pro_to_srgb,
                    );

                    // Output of the baked sRGB LUT
                    let baked_out = baked_lut.sample(srgb_in);

                    for c in 0..3 {
                        let diff = (baked_out[c] - develop_expected[c]).abs();
                        assert!(
                            diff <= 1.0 / 255.0,
                            "colour {:?} channel {c} mismatch: baked {}, develop {}, diff {}",
                            srgb_in,
                            baked_out[c],
                            develop_expected[c],
                            diff
                        );
                    }
                }
            }
        }

        // Grey 0.5 stays within the look's own result; it is not forced to grey.
        let grey_in = [0.5, 0.5, 0.5];
        let look_grey = transform_srgb_through_prophoto_look(
            grey_in,
            &prophoto_look,
            m_srgb_to_pro,
            m_pro_to_srgb,
        );
        let baked_grey = baked_lut.sample(grey_in);
        for c in 0..3 {
            let diff = (baked_grey[c] - look_grey[c]).abs();
            assert!(
                diff <= 1.0 / 255.0 + 1e-4,
                "grey 0.5 must match look's own result: baked {}, expected {}, diff {}",
                baked_grey[c],
                look_grey[c],
                diff
            );
        }
        // Classic chrome has a non-neutral color shift on 0.5 grey (not forced to [0.5, 0.5, 0.5])
        assert!(
            (baked_grey[0] - baked_grey[2]).abs() > 0.001 || (baked_grey[0] - 0.5).abs() > 0.001,
            "look's own result should retain its characteristic look rather than being forced to grey"
        );
    }

    #[test]
    #[cfg(feature = "film")]
    fn a_film_look_becomes_a_library_lut() {
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("luts");
        let film_dir = temp.path().join("film");
        let _ = crate::media::film::ensure_film_data(&film_dir).expect("ensure film data");

        let lut_ref =
            ensure_look_lut("film:kodak_portra_400", &lib_dir, &film_dir).expect("ensure film LUT");
        let dest_file = lib_dir.join(&lut_ref.name);
        assert!(dest_file.exists());
        assert_eq!(lut_ref.name, "Kodak Portra 400 (spektrafilm v2).cube");

        let parsed = Lut::from_file(&dest_file).expect("parse film LUT");
        assert_eq!(parsed.sha256, lut_ref.sha256);
        assert_eq!(parsed.size, 33);

        // Tungsten stock gets neutralgrey balance
        let tungsten_ref = ensure_look_lut("film:kodak_vision3_500t", &lib_dir, &film_dir)
            .expect("ensure tungsten film LUT");
        assert_eq!(
            tungsten_ref.name,
            "Kodak Vision3 500T neutral (spektrafilm v2).cube"
        );
        assert!(lib_dir.join(&tungsten_ref.name).exists());
    }

    #[test]
    fn an_unknown_look_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("luts");
        let film_dir = temp.path().join("film");

        match ensure_look_lut("builtin:nonexistent-look", &lib_dir, &film_dir) {
            Err(Error::Refused(msg)) => assert!(msg.contains("does not exist")),
            other => panic!("expected Refused for unknown builtin, got {other:?}"),
        }

        match ensure_look_lut("film:nonexistent-film", &lib_dir, &film_dir) {
            Err(Error::Refused(msg)) => {
                assert!(
                    msg.contains("unknown film stock")
                        || msg.contains("not built into this program")
                )
            }
            other => panic!("expected Refused for unknown film, got {other:?}"),
        }

        match ensure_look_lut("custom.cube", &lib_dir, &film_dir) {
            Err(Error::Refused(msg)) => assert!(msg.contains("unknown look id")),
            other => panic!("expected Refused for custom look id, got {other:?}"),
        }
    }

    #[test]
    fn every_camera_look_is_written_under_its_listed_name() {
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("luts");
        let film_dir = temp.path().join("film");

        for entry in crate::media::looks::builtin::builtin_look_list() {
            let lut_ref =
                ensure_look_lut(&entry.id, &lib_dir, &film_dir).expect("ensure camera look LUT");
            assert_eq!(lut_ref.name, entry.library_file_name);
            assert!(lib_dir.join(&lut_ref.name).exists());
        }
    }

    #[cfg(feature = "film")]
    #[test]
    fn a_film_simulation_is_written_under_its_listed_name() {
        let temp = tempfile::tempdir().unwrap();
        let lib_dir = temp.path().join("luts");
        let film_dir = temp.path().join("film");

        let film_looks = crate::media::film::film_look_list();
        for id in ["film:kodak_portra_400", "film:kodak_vision3_500t"] {
            let entry = film_looks
                .iter()
                .find(|l| l.id == id)
                .unwrap_or_else(|| panic!("expected to find {id} in film_look_list"));
            let lut_ref = ensure_look_lut(id, &lib_dir, &film_dir).expect("ensure film LUT");
            assert_eq!(lut_ref.name, entry.library_file_name);
            assert!(lib_dir.join(&lut_ref.name).exists());
        }
    }
}
