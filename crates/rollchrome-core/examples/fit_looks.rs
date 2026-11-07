//! Fitting tool for built-in looks from colour chart photographs (DV-3).
//!
//! Usage:
//! ```sh
//! cargo run --release -p rollchrome-core --example fit_looks -- <path-to-styles_source>
//! ```

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use rollchrome_core::media::develop::{develop, DevelopOptions};
use rollchrome_core::media::looks::colour as looks_colour;
use rollchrome_core::media::looks::fit::{
    bake_cube_33, compute_exposure_gain, evaluate_leave_one_out, evaluate_shipped_path,
    fit_look_model, measure_developed_patches, measure_jpeg_patches, FitConfig,
};

struct LookReportRow {
    maker: String,
    display_name: String,
    id: String,
    g: f32,
    det_m: f32,
    mean_de: f32,
    max_unclipped_de: f32,
    loo_mean_de: f32,
    loo_max_de: f32,
    clipped_str: String,
    passed: bool,
    notes: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: fit_looks <path-to-styles_source>");
        std::process::exit(1);
    }
    let sources_dir = PathBuf::from(&args[1]);
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let looks_dir = manifest_dir.join("looks");
    let fit_json_path = looks_dir.join("fit.json");

    println!("Reading configuration from {}...", fit_json_path.display());
    let fit_json_str = fs::read_to_string(&fit_json_path)?;
    let fit_config: FitConfig = serde_json::from_str(&fit_json_str)?;

    let mut report_rows = Vec::new();

    for cam_cfg in &fit_config.cameras {
        let raw_path = sources_dir.join(&cam_cfg.raw);
        println!("\nDeveloping RAW chart {}...", raw_path.display());
        let t_dev = Instant::now();
        let raw_bytes = fs::read(&raw_path)?;
        let developed = develop(&raw_bytes, &DevelopOptions { exposure_ev: 0.0 })?;
        println!(
            "Developed {} in {:.2}s ({}x{})",
            cam_cfg.raw,
            t_dev.elapsed().as_secs_f32(),
            developed.image.width(),
            developed.image.height()
        );

        println!("Measuring developed patches on {}...", cam_cfg.raw);
        let ours_patches = measure_developed_patches(&developed, cam_cfg)?;

        for look_cfg in &cam_cfg.looks {
            println!(
                "\n--- Fitting look: {} ({}) ---",
                look_cfg.display_name, look_cfg.id
            );
            let t_look = Instant::now();
            let jpeg_path = sources_dir.join(&look_cfg.jpeg);
            let jpeg_bytes = fs::read(&jpeg_path)?;

            // Verify ICC profile
            looks_colour::verify_jpeg_srgb(&jpeg_bytes)?;

            // Decode JPEG
            let jpeg_img = image::load_from_memory(&jpeg_bytes)?.to_rgb8();
            let (target_patches_srgb, clipped) =
                match measure_jpeg_patches(&jpeg_img, &look_cfg.jpeg, cam_cfg) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Look {} measure REFUSED: {}", look_cfg.display_name, e);
                        report_rows.push(LookReportRow {
                            maker: cam_cfg.maker.clone(),
                            display_name: look_cfg.display_name.clone(),
                            id: look_cfg.id.clone(),
                            g: 0.0,
                            det_m: 0.0,
                            mean_de: 0.0,
                            max_unclipped_de: 0.0,
                            loo_mean_de: 0.0,
                            loo_max_de: 0.0,
                            clipped_str: "none".to_string(),
                            passed: false,
                            notes: format!("Refused: {e}"),
                        });
                        continue;
                    }
                };

            let target_labs: Vec<[f32; 3]> = target_patches_srgb
                .iter()
                .map(|&p| looks_colour::srgb_to_lab_d50(p))
                .collect();

            let clipped_indices: Vec<usize> = clipped
                .iter()
                .enumerate()
                .filter_map(|(i, &c)| if c { Some(i) } else { None })
                .collect();
            let clipped_str = if clipped_indices.is_empty() {
                "none".to_string()
            } else {
                format!("{:?}", clipped_indices)
            };

            // Gain g
            let g = compute_exposure_gain(&ours_patches, &target_patches_srgb);

            // Fit full model
            let model = fit_look_model(&ours_patches, &target_labs, &clipped, g, None)?;
            let det_m = model.det_m();

            // Leave-one-out cross validation
            let loo = evaluate_leave_one_out(&ours_patches, &target_labs, &clipped, g)?;

            // Bake .cube
            let cube_str = bake_cube_33(&model, &look_cfg.display_name);

            // Shipped path evaluation
            let shipped_eval = evaluate_shipped_path(
                &developed,
                cube_str.as_bytes(),
                cam_cfg,
                &target_labs,
                &clipped,
                loo.mean_delta_e,
            )?;

            let cube_dest = looks_dir.join(format!("{}.cube", look_cfg.id));
            if shipped_eval.passed {
                fs::write(&cube_dest, cube_str.as_bytes())?;
                println!("Passed! Wrote .cube to {}", cube_dest.display());
            } else {
                if cube_dest.exists() {
                    let _ = fs::remove_file(&cube_dest);
                }
                println!("FAILED: {:?}", shipped_eval.failure_reasons);
            }

            let notes = if shipped_eval.passed {
                "OK".to_string()
            } else {
                shipped_eval.failure_reasons.join("; ")
            };

            println!(
                "Look {} fitted in {:.2}s: g={:.3}, det M={:.3}, mean ΔE={:.2}, max unclipped ΔE={:.2}, LOO mean={:.2}, LOO max={:.2}, status={}",
                look_cfg.display_name,
                t_look.elapsed().as_secs_f32(),
                g,
                det_m,
                shipped_eval.mean_delta_e,
                shipped_eval.max_unclipped_delta_e,
                loo.mean_delta_e,
                loo.max_delta_e,
                if shipped_eval.passed { "PASS" } else { "FAIL" }
            );

            report_rows.push(LookReportRow {
                maker: cam_cfg.maker.clone(),
                display_name: look_cfg.display_name.clone(),
                id: look_cfg.id.clone(),
                g,
                det_m,
                mean_de: shipped_eval.mean_delta_e,
                max_unclipped_de: shipped_eval.max_unclipped_delta_e,
                loo_mean_de: loo.mean_delta_e,
                loo_max_de: loo.max_delta_e,
                clipped_str,
                passed: shipped_eval.passed,
                notes,
            });
        }
    }

    // Write looks/fit-report.md
    let report_path = looks_dir.join("fit-report.md");
    let mut md = String::new();
    md.push_str("# Built-in Looks Fit Report (DV-3)\n\n");
    md.push_str(&format!(
        "Source: `{}` commit `{}`\n\n",
        fit_config.source_repository, fit_config.source_commit
    ));
    md.push_str("Thresholds: Mean ΔE2000 ≤ 2.5, Max unclipped ΔE2000 ≤ 7.0, Leave-one-out mean ΔE2000 ≤ 5.0.\n\n");
    md.push_str("| Look | Maker | ID | g | det M | Mean ΔE | Max unclipped ΔE | LOO Mean ΔE | LOO Max ΔE | Clipped patches | Status | Notes |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|\n");

    for row in &report_rows {
        md.push_str(&format!(
            "| {} | {} | `{}` | {:.3} | {:.3} | {:.2} | {:.2} | {:.2} | {:.2} | {} | **{}** | {} |\n",
            row.display_name,
            row.maker,
            row.id,
            row.g,
            row.det_m,
            row.mean_de,
            row.max_unclipped_de,
            row.loo_mean_de,
            row.loo_max_de,
            row.clipped_str,
            if row.passed { "PASS" } else { "FAIL" },
            row.notes
        ));
    }

    fs::write(&report_path, md.as_bytes())?;
    println!("\nWrote fit report to {}", report_path.display());

    Ok(())
}
