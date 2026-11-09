use image::{GrayImage, Rgb, RgbImage};
use phototools_core::jobs::Progress;
use rollchrome_core::media::edit::geometry::Geometry;
use rollchrome_core::media::edit::{
    AdjustmentRecipe, AutoTarget, LocalAdjustments, LocalDetail, Mask, MaskComponent, MaskKind,
    MaskOp, ObjectClick, PreviewSession, PreviewStage, StoredRaster,
};
use rollchrome_core::media::segment::{
    choose_object_mask, download_object_model, finish_object_mask, object_model_path,
    object_prompt, prepare_object_input, to_upright, upright_click, with_test_object_specs,
    ObjectModelSpec, OBJECT_DECODER, OBJECT_ENCODER,
};
use rollchrome_core::tools::edit::{export_edited_image, load_recipe, save_recipe};
use rollchrome_core::tools::presets::{load_preset, save_preset};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn upright_click_for_all_eight_orientations_agrees_with_to_upright_and_the_geometry() {
    let (w, h) = (30u32, 20u32);
    let (sx, sy) = (5u32, 2u32);
    let mut stored = GrayImage::new(w, h);
    stored.put_pixel(sx, sy, image::Luma([255]));

    let u_stored = (sx as f32 + 0.5) / w as f32;
    let v_stored = (sy as f32 + 0.5) / h as f32;

    for o in 1..=8 {
        let up = to_upright(&stored, o);
        let (mx, my) = up
            .enumerate_pixels()
            .find(|(_, _, p)| p[0] == 255)
            .map(|(x, y, _)| (x, y))
            .unwrap();

        let (u_up, v_up) = upright_click(u_stored, v_stored, o);
        let up_x = u_up * up.width() as f32;
        let up_y = v_up * up.height() as f32;
        assert!(
            (up_x - (mx as f32 + 0.5)).abs() < 1e-4 && (up_y - (my as f32 + 0.5)).abs() < 1e-4,
            "orientation {o}: upright_click gave ({up_x}, {up_y}), expected ({}, {})",
            mx as f32 + 0.5,
            my as f32 + 0.5
        );

        let plan = Geometry::default().plan(w, h, o).unwrap();
        let (mapped_sx, mapped_sy) = plan.map(up_x, up_y);
        assert_eq!(
            (mapped_sx.floor() as u32, mapped_sy.floor() as u32),
            (sx, sy),
            "orientation {o}"
        );
    }
}

#[test]
fn the_encoder_input_is_hwc_0_to_255_with_long_edge_1024_on_landscape_and_portrait() {
    let landscape = RgbImage::from_pixel(600, 300, Rgb([17, 129, 241]));
    let out_l = prepare_object_input(&landscape);
    assert_eq!((out_l.width, out_l.height), (1024, 512));
    assert_eq!(out_l.data.len(), 1024 * 512 * 3);
    assert_eq!(&out_l.data[0..3], &[17.0, 129.0, 241.0]);
    assert_eq!(&out_l.data[3..6], &[17.0, 129.0, 241.0]);

    let mut portrait = RgbImage::from_pixel(300, 600, Rgb([10, 20, 30]));
    // Put a uniform block in the bottom-right corner so a known pixel value survives resize.
    for y in 400..600 {
        for x in 200..300 {
            portrait.put_pixel(x, y, Rgb([210, 65, 99]));
        }
    }
    let out_p = prepare_object_input(&portrait);
    assert_eq!((out_p.width, out_p.height), (512, 1024));
    assert_eq!(out_p.data.len(), 512 * 1024 * 3);
    assert_eq!(&out_p.data[0..3], &[10.0, 20.0, 30.0]);
    let idx = ((900 * 512 + 450) * 3) as usize;
    assert_eq!(&out_p.data[idx..idx + 3], &[210.0, 65.0, 99.0]);
    assert!(out_p.data.iter().all(|&v| (0.0..=255.0).contains(&v)));
}

#[test]
fn the_prompt_puts_x_before_y_in_resized_pixels_and_appends_the_sentinel() {
    let clicks = [
        ObjectClick {
            u: 0.25,
            v: 0.5,
            add: true,
        },
        ObjectClick {
            u: 0.75,
            v: 0.125,
            add: false,
        },
    ];
    let (coords, labels) = object_prompt(&clicks, 1, 1024, 680).unwrap();
    assert_eq!(
        coords,
        vec![
            0.25 * 1024.0,
            0.5 * 680.0,
            0.75 * 1024.0,
            0.125 * 680.0,
            0.0,
            0.0
        ]
    );
    assert_eq!(labels, vec![1.0, 0.0, -1.0]);
}

#[test]
fn object_prompt_refuses_when_given_no_clicks() {
    let err = object_prompt(&[], 1, 1024, 683).unwrap_err();
    assert!(
        err.to_string().contains("click on an object to select it"),
        "got: {err}"
    );
}

#[test]
fn object_prompt_refuses_more_than_thirty_two_clicks() {
    let clicks = vec![
        ObjectClick {
            u: 0.5,
            v: 0.5,
            add: true,
        };
        33
    ];
    let err = object_prompt(&clicks, 1, 1024, 683).unwrap_err();
    assert!(err.to_string().contains("at most 32 clicks"), "got: {err}");
}

#[test]
fn object_prompt_refuses_when_there_is_no_add_click() {
    let clicks = [ObjectClick {
        u: 0.5,
        v: 0.5,
        add: false,
    }];
    let err = object_prompt(&clicks, 1, 1024, 683).unwrap_err();
    assert!(
        err.to_string()
            .contains("click on the object to add before taking away"),
        "got: {err}"
    );
}

#[test]
fn object_prompt_refuses_a_click_that_is_not_finite_or_outside_the_unit_square() {
    for bad in [
        ObjectClick {
            u: -0.01,
            v: 0.5,
            add: true,
        },
        ObjectClick {
            u: 0.5,
            v: 1.01,
            add: true,
        },
        ObjectClick {
            u: f32::NAN,
            v: 0.5,
            add: true,
        },
        ObjectClick {
            u: 0.5,
            v: f32::INFINITY,
            add: true,
        },
    ] {
        let err = object_prompt(&[bad], 1, 1024, 683).unwrap_err();
        assert!(
            err.to_string().contains("outside the photograph"),
            "got: {err}"
        );
    }
}

#[test]
fn choose_object_mask_never_picks_index_zero_for_one_click_and_always_picks_zero_for_more() {
    assert_eq!(choose_object_mask(&[0.99, 0.40, 0.85, 0.70], 1), 2);
    assert_eq!(choose_object_mask(&[0.99, 0.91, 0.50, 0.60], 1), 1);
    assert_eq!(choose_object_mask(&[0.99, 0.40, 0.50, 0.88], 1), 3);

    assert_eq!(choose_object_mask(&[0.10, 0.95, 0.92, 0.90], 2), 0);
    assert_eq!(choose_object_mask(&[0.05, 0.99, 0.98, 0.97], 5), 0);
}

#[test]
fn finish_object_mask_preserves_the_soft_edge_orients_to_stored_and_refuses_an_empty_mask() {
    // A logit of 0 maps to 128 (soft midpoint), and a positive logit satisfies >= 128.
    let logits = vec![0.0f32, 4.0, -4.0, 0.0];
    let raster = finish_object_mask(&logits, 2, 2, 1).unwrap();
    assert_eq!(raster.model_sha256, OBJECT_DECODER.sha256);
    let decoded = raster.decode().unwrap();
    assert!(
        decoded.data[0] == 127 || decoded.data[0] == 128,
        "logit 0 must be 127 or 128, got {}",
        decoded.data[0]
    );
    assert!(decoded.data[1] > 240 && decoded.data[1] < 255);
    assert!(decoded.data[2] > 0 && decoded.data[2] < 15);

    // Orientation 6: upright frame is 20 × 30 (stored 30 × 20 rotated 90° CW).
    // Upright top corresponds to stored left, matching the geometry engine.
    let (up_w, up_h) = (20u32, 30u32);
    let mut up_logits = vec![-6.0f32; (up_w * up_h) as usize];
    for y in 0..10 {
        for x in 0..up_w {
            up_logits[(y * up_w + x) as usize] = 6.0;
        }
    }
    let oriented = finish_object_mask(&up_logits, up_w, up_h, 6).unwrap();
    assert_eq!((oriented.width, oriented.height), (30, 20));
    let cov = oriented.decode().unwrap();
    let plan = Geometry::default().plan(30, 20, 6).unwrap();
    let (sx_top, sy_top) = plan.map(10.0, 3.0);
    let (sx_bot, sy_bot) = plan.map(10.0, 25.0);
    assert!(cov.sample(sx_top / 30.0, sy_top / 20.0) > 0.98);
    assert!(cov.sample(sx_bot / 30.0, sy_bot / 20.0) < 0.02);

    // Nothing reaching 128 is refused in words.
    let empty_logits = vec![-0.5f32; 16];
    let err = finish_object_mask(&empty_logits, 4, 4, 1).unwrap_err();
    assert!(
        err.to_string()
            .contains("nothing was found there; click on the object itself"),
        "got: {err}"
    );
}

fn serve_two(first: Vec<u8>, second: Vec<u8>) -> (String, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for body in [first, second] {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 2 {
                    line.clear();
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        }
    });
    (
        format!("http://{addr}/encoder.onnx"),
        format!("http://{addr}/decoder.onnx"),
    )
}

fn sha(bytes: &[u8]) -> String {
    phototools_core::ingest::scanner::hex(&Sha256::digest(bytes))
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn make_spec(file: &'static str, url: String, sha256: String, bytes: u64) -> ObjectModelSpec {
    ObjectModelSpec {
        name: "test object model",
        file,
        url: leak_str(url),
        sha256: leak_str(sha256),
        bytes,
    }
}

#[derive(Default)]
struct TestProgress {
    events: std::sync::Mutex<Vec<(u64, u64)>>,
    cancelled: AtomicBool,
}

impl Progress for TestProgress {
    fn report(&self, done: u64, total: u64, _phase: &str) {
        self.events.lock().unwrap().push((done, total));
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[test]
fn download_object_model_over_a_local_server_verifies_both_files_handles_failures_and_tracks_total_bytes(
) {
    // 1. Both files valid: both kept, running total reaches the sum of both files.
    {
        let dir = tempfile::tempdir().unwrap();
        let b1: Vec<u8> = (0..90_000u32).map(|i| (i % 251) as u8).collect();
        let b2: Vec<u8> = (0..60_000u32).map(|i| ((i * 7) % 251) as u8).collect();
        let (u1, u2) = serve_two(b1.clone(), b2.clone());
        let s1 = make_spec(OBJECT_ENCODER.file, u1, sha(&b1), b1.len() as u64);
        let s2 = make_spec(OBJECT_DECODER.file, u2, sha(&b2), b2.len() as u64);
        let progress = TestProgress::default();
        let (p1, p2) = with_test_object_specs(s1, s2, || {
            download_object_model(dir.path(), &progress).unwrap()
        });
        assert_eq!(std::fs::read(&p1).unwrap(), b1);
        assert_eq!(std::fs::read(&p2).unwrap(), b2);
        let events = progress.events.lock().unwrap().clone();
        let sum = (b1.len() + b2.len()) as u64;
        assert!(!events.is_empty());
        assert!(events.iter().all(|&(_, total)| total == sum));
        assert_eq!(events.last().unwrap().0, sum);
    }

    // 2. Wrong second file leaves the first on disk and no second file or .part.
    {
        let dir = tempfile::tempdir().unwrap();
        let b1: Vec<u8> = (0..50_000u32).map(|i| (i % 241) as u8).collect();
        let b2 = vec![42u8; 30_000];
        let (u1, u2) = serve_two(b1.clone(), b2.clone());
        let s1 = make_spec(OBJECT_ENCODER.file, u1, sha(&b1), b1.len() as u64);
        let s2 = make_spec(
            OBJECT_DECODER.file,
            u2,
            sha(b"different bytes"),
            b2.len() as u64,
        );
        let progress = TestProgress::default();
        let err = with_test_object_specs(s1, s2, || {
            download_object_model(dir.path(), &progress).unwrap_err()
        });
        assert!(err.to_string().contains("not the one expected"));
        let enc_path = object_model_path(dir.path(), &OBJECT_ENCODER);
        let dec_path = object_model_path(dir.path(), &OBJECT_DECODER);
        assert_eq!(std::fs::read(&enc_path).unwrap(), b1);
        assert!(!dec_path.exists());
        assert!(!dec_path.with_extension("part").exists());
    }

    // 3. Cancel during the second file leaves no .part behind.
    {
        struct CancelOnSecond {
            calls: AtomicUsize,
            first_bytes: u64,
        }
        impl Progress for CancelOnSecond {
            fn report(&self, done: u64, _total: u64, _phase: &str) {
                if done > self.first_bytes {
                    self.calls.fetch_add(1, Ordering::SeqCst);
                }
            }
            fn cancelled(&self) -> bool {
                self.calls.load(Ordering::SeqCst) > 0
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let b1: Vec<u8> = (0..40_000u32).map(|i| (i % 233) as u8).collect();
        let b2 = vec![9u8; 2_500_000];
        let (u1, u2) = serve_two(b1.clone(), b2.clone());
        let s1 = make_spec(OBJECT_ENCODER.file, u1, sha(&b1), b1.len() as u64);
        let s2 = make_spec(OBJECT_DECODER.file, u2, sha(&b2), b2.len() as u64);
        let progress = CancelOnSecond {
            calls: AtomicUsize::new(0),
            first_bytes: b1.len() as u64,
        };
        let err = with_test_object_specs(s1, s2, || {
            download_object_model(dir.path(), &progress).unwrap_err()
        });
        assert!(err.to_string().contains("cancelled"), "got: {err}");
        let enc_path = object_model_path(dir.path(), &OBJECT_ENCODER);
        let dec_path = object_model_path(dir.path(), &OBJECT_DECODER);
        assert_eq!(std::fs::read(&enc_path).unwrap(), b1);
        assert!(!dec_path.exists());
        assert!(!dec_path.with_extension("part").exists());
        assert!(!enc_path.with_extension("part").exists());
    }
}

fn sample_raster(w: u32, h: u32, fill_left_half: u8, fill_right_half: u8) -> StoredRaster {
    let mut bytes = Vec::with_capacity((w * h) as usize);
    for _y in 0..h {
        for x in 0..w {
            bytes.push(if x < w / 2 {
                fill_left_half
            } else {
                fill_right_half
            });
        }
    }
    StoredRaster::encode(w, h, &bytes, OBJECT_DECODER.sha256).unwrap()
}

#[test]
fn object_mask_renders_byte_identically_to_auto_mask_as_base_and_as_component_in_preview_and_export(
) {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.jpg");
    let mut img = RgbImage::new(80, 60);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = Rgb([(x * 3) as u8, (y * 4) as u8, 120]);
    }
    img.save(&src).unwrap();

    let raster = sample_raster(80, 60, 255, 20);
    let clicks = vec![ObjectClick {
        u: 0.25,
        v: 0.5,
        add: true,
    }];
    let adj = LocalAdjustments {
        exposure: 0.8,
        contrast: 15.0,
        saturation: -20.0,
        ..Default::default()
    };
    let det = LocalDetail {
        clarity: 25.0,
        sharpness: 30.0,
        ..Default::default()
    };

    // Base mask comparison (preview + export).
    let recipe_auto = AdjustmentRecipe {
        masks: vec![Mask {
            id: "m1".into(),
            name: "Mask 1".into(),
            kind: MaskKind::Auto {
                target: AutoTarget::Subject,
                mask: raster.clone(),
            },
            invert: false,
            opacity: 0.85,
            enabled: true,
            adjustments: adj,
            detail: det,
            strokes: Vec::new(),
            components: Vec::new(),
        }],
        ..Default::default()
    };
    let recipe_obj = AdjustmentRecipe {
        masks: vec![Mask {
            id: "m1".into(),
            name: "Mask 1".into(),
            kind: MaskKind::Object {
                clicks: clicks.clone(),
                mask: raster.clone(),
            },
            invert: false,
            opacity: 0.85,
            enabled: true,
            adjustments: adj,
            detail: det,
            strokes: Vec::new(),
            components: Vec::new(),
        }],
        ..Default::default()
    };

    let session = PreviewSession::open(&src).unwrap();
    let prev_auto = session
        .render(&recipe_auto, None, PreviewStage::Settle)
        .unwrap();
    let prev_obj = session
        .render(&recipe_obj, None, PreviewStage::Settle)
        .unwrap();
    assert_eq!(prev_auto.bytes, prev_obj.bytes);

    let cov_auto = session
        .mask_coverage(&recipe_auto, "m1", PreviewStage::Settle)
        .unwrap();
    let cov_obj = session
        .mask_coverage(&recipe_obj, "m1", PreviewStage::Settle)
        .unwrap();
    assert_eq!(cov_auto, cov_obj);

    let out_dir_auto = dir.path().join("export_auto");
    let out_dir_obj = dir.path().join("export_obj");
    std::fs::create_dir_all(&out_dir_auto).unwrap();
    std::fs::create_dir_all(&out_dir_obj).unwrap();

    let res_auto = export_edited_image(&src, &recipe_auto, None, &out_dir_auto).unwrap();
    let res_obj = export_edited_image(&src, &recipe_obj, None, &out_dir_obj).unwrap();
    assert_eq!(
        std::fs::read(&res_auto.path).unwrap(),
        std::fs::read(&res_obj.path).unwrap()
    );

    // Component comparison (preview + export).
    let comp_auto = AdjustmentRecipe {
        masks: vec![Mask {
            id: "m2".into(),
            name: "Mask 2".into(),
            kind: MaskKind::Linear {
                start: [0.0, 0.0],
                end: [1.0, 1.0],
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: adj,
            detail: det,
            strokes: Vec::new(),
            components: vec![MaskComponent {
                op: MaskOp::Subtract,
                kind: MaskKind::Auto {
                    target: AutoTarget::Subject,
                    mask: raster.clone(),
                },
                invert: false,
            }],
        }],
        ..Default::default()
    };
    let comp_obj = AdjustmentRecipe {
        masks: vec![Mask {
            id: "m2".into(),
            name: "Mask 2".into(),
            kind: MaskKind::Linear {
                start: [0.0, 0.0],
                end: [1.0, 1.0],
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: adj,
            detail: det,
            strokes: Vec::new(),
            components: vec![MaskComponent {
                op: MaskOp::Subtract,
                kind: MaskKind::Object {
                    clicks,
                    mask: raster,
                },
                invert: false,
            }],
        }],
        ..Default::default()
    };

    let prev_comp_auto = session
        .render(&comp_auto, None, PreviewStage::Settle)
        .unwrap();
    let prev_comp_obj = session
        .render(&comp_obj, None, PreviewStage::Settle)
        .unwrap();
    assert_eq!(prev_comp_auto.bytes, prev_comp_obj.bytes);

    let res_comp_auto = export_edited_image(&src, &comp_auto, None, &out_dir_auto).unwrap();
    let res_comp_obj = export_edited_image(&src, &comp_obj, None, &out_dir_obj).unwrap();
    assert_eq!(
        std::fs::read(&res_comp_auto.path).unwrap(),
        std::fs::read(&res_comp_obj.path).unwrap()
    );
}

#[test]
fn a_sidecar_with_an_object_mask_round_trips_with_its_clicks_and_a_preset_never_carries_one() {
    let dir = tempfile::tempdir().unwrap();
    let photo_with = dir.path().join("with_object.jpg");
    let photo_without = dir.path().join("without_object.jpg");
    RgbImage::new(32, 32).save(&photo_with).unwrap();
    RgbImage::new(32, 32).save(&photo_without).unwrap();

    let raster = sample_raster(16, 16, 240, 10);
    let clicks = vec![
        ObjectClick {
            u: 0.2,
            v: 0.3,
            add: true,
        },
        ObjectClick {
            u: 0.8,
            v: 0.7,
            add: false,
        },
    ];
    let recipe_with = AdjustmentRecipe {
        exposure: 0.4,
        masks: vec![Mask {
            id: "obj-1".into(),
            name: "Object 1".into(),
            kind: MaskKind::Object {
                clicks: clicks.clone(),
                mask: raster.clone(),
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure: 0.5,
                ..Default::default()
            },
            detail: LocalDetail::default(),
            strokes: Vec::new(),
            components: Vec::new(),
        }],
        ..Default::default()
    };

    let sidecar_with = save_recipe(&photo_with, &recipe_with).unwrap();
    let loaded_with = load_recipe(&sidecar_with).unwrap();
    assert_eq!(loaded_with.masks, recipe_with.masks);
    assert_eq!(loaded_with.exposure, recipe_with.exposure);
    match &loaded_with.masks[0].kind {
        MaskKind::Object {
            clicks: loaded_clicks,
            mask: loaded_mask,
        } => {
            assert_eq!(loaded_clicks, &clicks);
            assert_eq!(loaded_mask, &raster);
        }
        other => panic!("expected Object mask, got {other:?}"),
    }

    let recipe_without = AdjustmentRecipe {
        exposure: -0.25,
        contrast: 12.0,
        ..Default::default()
    };
    let sidecar_without = save_recipe(&photo_without, &recipe_without).unwrap();
    let loaded_without = load_recipe(&sidecar_without).unwrap();
    assert_eq!(loaded_without.masks, recipe_without.masks);
    assert_eq!(loaded_without.exposure, recipe_without.exposure);
    assert_eq!(loaded_without.contrast, recipe_without.contrast);
    assert!(loaded_without.masks.is_empty());

    let presets_dir = dir.path().join("presets");
    save_preset(&presets_dir, "Warm Object Test", &recipe_with, false).unwrap();
    let preset_recipe = load_preset(&presets_dir, "Warm Object Test").unwrap();
    assert!(
        preset_recipe.masks.is_empty(),
        "a preset must never carry masks"
    );
}

#[test]
fn changing_an_object_mask_raster_changes_the_next_render() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.jpg");
    RgbImage::from_pixel(64, 64, Rgb([120, 120, 120]))
        .save(&src)
        .unwrap();

    let session = PreviewSession::open(&src).unwrap();
    let raster_left = sample_raster(64, 64, 255, 0);
    let raster_right = sample_raster(64, 64, 0, 255);
    assert_ne!(raster_left.png, raster_right.png);

    let make_recipe = |mask: StoredRaster| AdjustmentRecipe {
        masks: vec![Mask {
            id: "same-mask-id".into(),
            name: "Object 1".into(),
            kind: MaskKind::Object {
                clicks: vec![ObjectClick {
                    u: 0.25,
                    v: 0.5,
                    add: true,
                }],
                mask,
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure: 1.5,
                ..Default::default()
            },
            detail: LocalDetail::default(),
            strokes: Vec::new(),
            components: Vec::new(),
        }],
        ..Default::default()
    };

    let frame_1 = session
        .render(&make_recipe(raster_left), None, PreviewStage::Settle)
        .unwrap();
    let frame_2 = session
        .render(&make_recipe(raster_right), None, PreviewStage::Settle)
        .unwrap();
    assert_ne!(
        frame_1.bytes, frame_2.bytes,
        "replacing the Object mask raster under the same mask id must invalidate the cached raster"
    );
}

#[cfg(feature = "segment")]
#[test]
#[ignore = "requires MobileSAM ONNX models in ROLLCHROME_OBJECT_MODELS"]
fn the_real_model_selects_the_object_that_was_clicked() {
    use rollchrome_core::media::segment::ObjectSelector;
    use std::path::PathBuf;
    use std::time::Instant;

    let models_var = std::env::var("ROLLCHROME_OBJECT_MODELS").unwrap_or_else(|_| {
        panic!(
            "ROLLCHROME_OBJECT_MODELS is not set; point it at a folder containing {} and {}",
            OBJECT_ENCODER.file, OBJECT_DECODER.file
        )
    });
    let models_dir = PathBuf::from(models_var);

    // 3000 × 2000 synthetic picture: dark background, one bright disc on the left,
    // and one bright square on the right apart from it.
    let (w, h) = (3000u32, 2000u32);
    let (disc_cx, disc_cy, disc_r) = (900.0f32, 1000.0f32, 360.0f32);
    let (sq_x0, sq_x1, sq_y0, sq_y1) = (1780u32, 2420u32, 680u32, 1320u32);

    let in_disc = |u: f32, v: f32| -> bool {
        let x = u * w as f32;
        let y = v * h as f32;
        let dx = x - disc_cx;
        let dy = y - disc_cy;
        dx * dx + dy * dy <= disc_r * disc_r
    };
    let in_square = |u: f32, v: f32| -> bool {
        let x = u * w as f32;
        let y = v * h as f32;
        x >= sq_x0 as f32 && x <= sq_x1 as f32 && y >= sq_y0 as f32 && y <= sq_y1 as f32
    };

    let mut img = RgbImage::from_pixel(w, h, Rgb([20, 22, 26]));
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let v = (y as f32 + 0.5) / h as f32;
            if in_disc(u, v) {
                img.put_pixel(x, y, Rgb([235, 220, 195]));
            } else if in_square(u, v) {
                img.put_pixel(x, y, Rgb([210, 235, 240]));
            }
        }
    }

    let t_enc = Instant::now();
    let mut selector = ObjectSelector::new(&img, 1, &models_dir).unwrap();
    let enc_elapsed = t_enc.elapsed();

    let disc_click = ObjectClick {
        u: disc_cx / w as f32,
        v: disc_cy / h as f32,
        add: true,
    };
    let sq_add_click = ObjectClick {
        u: ((sq_x0 + sq_x1) as f32 * 0.5) / w as f32,
        v: ((sq_y0 + sq_y1) as f32 * 0.5) / h as f32,
        add: true,
    };
    let sq_sub_click = ObjectClick {
        add: false,
        ..sq_add_click
    };

    // 1. Click the disc: >= 90% IoU with the disc, and the square not selected.
    let t_click1 = Instant::now();
    let r_disc = selector.select(&[disc_click]).unwrap();
    let click1_elapsed = t_click1.elapsed();
    let cov_disc = r_disc.decode().unwrap();

    let measure = |cov: &rollchrome_core::media::edit::CoverageRaster| -> (f64, f64, f64) {
        let mut inter_disc = 0u64;
        let mut union_disc = 0u64;
        let mut disc_total = 0u64;
        let mut disc_selected = 0u64;
        let mut sq_total = 0u64;
        let mut sq_selected = 0u64;
        for y in 0..cov.h {
            let v = (y as f32 + 0.5) / cov.h as f32;
            for x in 0..cov.w {
                let u = (x as f32 + 0.5) / cov.w as f32;
                let sel = cov.sample(u, v) >= 0.5;
                let d = in_disc(u, v);
                let s = in_square(u, v);
                if d {
                    disc_total += 1;
                    if sel {
                        disc_selected += 1;
                    }
                }
                if d && sel {
                    inter_disc += 1;
                }
                if d || sel {
                    union_disc += 1;
                }
                if s {
                    sq_total += 1;
                    if sel {
                        sq_selected += 1;
                    }
                }
            }
        }
        let disc_iou = inter_disc as f64 / union_disc.max(1) as f64;
        let disc_cov = disc_selected as f64 / disc_total.max(1) as f64;
        let sq_cov = sq_selected as f64 / sq_total.max(1) as f64;
        (disc_iou, disc_cov, sq_cov)
    };

    let (disc_iou_1, _disc_cov_1, sq_cov_1) = measure(&cov_disc);
    assert!(
        disc_iou_1 >= 0.90,
        "clicking the disc must reach >= 90% IoU with the disc, got {:.4}",
        disc_iou_1
    );
    assert!(
        sq_cov_1 < 0.01,
        "clicking the disc must not select the square, got square coverage {:.4}",
        sq_cov_1
    );

    // 2. Click the disc and the square: both selected.
    let t_click2 = Instant::now();
    let r_both = selector.select(&[disc_click, sq_add_click]).unwrap();
    let click2_elapsed = t_click2.elapsed();
    let cov_both = r_both.decode().unwrap();
    let (_disc_iou_2, disc_cov_2, sq_cov_2) = measure(&cov_both);
    assert!(
        disc_cov_2 >= 0.90 && sq_cov_2 >= 0.90,
        "clicking both must select both (disc coverage {:.4}, square coverage {:.4})",
        disc_cov_2,
        sq_cov_2
    );

    // 3. Click the disc then Alt-click the square: disc only.
    let t_click3 = Instant::now();
    let r_sub = selector.select(&[disc_click, sq_sub_click]).unwrap();
    let click3_elapsed = t_click3.elapsed();
    let cov_sub = r_sub.decode().unwrap();
    let (disc_iou_3, _disc_cov_3, sq_cov_3) = measure(&cov_sub);
    assert!(
        disc_iou_3 >= 0.90,
        "disc + Alt-square must select the disc with >= 90% IoU, got {:.4}",
        disc_iou_3
    );
    assert!(
        sq_cov_3 < 0.01,
        "disc + Alt-square must not select the square, got square coverage {:.4}",
        sq_cov_3
    );

    println!(
        "MobileSAM timing on 3000x2000: encoder={:.1} ms, click1={:.2} ms (disc IoU={:.4}), click2={:.2} ms (disc={:.4}, sq={:.4}), click3={:.2} ms (disc IoU={:.4}, sq={:.4})",
        enc_elapsed.as_secs_f64() * 1000.0,
        click1_elapsed.as_secs_f64() * 1000.0,
        disc_iou_1,
        click2_elapsed.as_secs_f64() * 1000.0,
        disc_cov_2,
        sq_cov_2,
        click3_elapsed.as_secs_f64() * 1000.0,
        disc_iou_3,
        sq_cov_3,
    );
}
