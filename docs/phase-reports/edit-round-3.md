# Edit, Round 3 — local adjustments (masks)

The steps are in [`edit-plan.md`](../edit-plan.md), Round 3, each with an **As built** note that
records where it departed from the plan and why. This report holds what the ground rules ask a
report to hold: the new dependency and its reason (G8), and what could not be verified here.

The edit plan's header points to `phase-reports/edit.md` for Rounds 1 and 2; that file was never
written. Those rounds' departures are recorded in the plan's own **As built** notes.

## The new dependency (G8)

| Crate | Version | Licence | Where | Why |
|---|---|---|---|---|
| `ort` | `=2.0.0-rc.13` | MIT or Apache-2.0 | `core`, optional, feature `segment`; enabled only by `desktop` | Runs the automatic subject and sky models (ED-23) through ONNX Runtime |

- **Chosen by the owner** (2026-10-03) over `tract`, after ED-22 measured both options' cost: `tract`
  needs Rust 1.91, `ort` 1.88. The MSRV rose from 1.80 to 1.88 in its own commit.
- **Pinned exactly** because 2.0 is a release candidate: a minor bump of a candidate may change its
  API, and a model run is not something to discover broken at a photographer's desk.
- **Optional.** `core` builds and passes its tests without it (G2); the NAS server never links it.
  `cargo test --workspace` builds `core` with it, because the desktop crate enables the feature.
- **Linked statically.** With `ort`'s default features ONNX Runtime is downloaded at build time and
  linked into the binary, which then depends only on system frameworks: nothing is added to the
  application bundle. The build therefore needs the network the first time it is built on a machine.
- **CPU only.** CoreML hung with default settings, could not compile BiRefNet, and ran it some
  thirty times slower on the GPU (ED-22 findings). A side effect worth knowing: the CoreML attempts
  left macOS's `ANECompilerService` at 100% of a core for hours afterwards on the measuring machine.

No other crate was added: the downloads use `reqwest`, the hashes `sha2`, the stored masks `image`'s
PNG codec, and base64 is twenty lines in `media::edit::raster` rather than a dependency for them.

## The models

Not dependencies of the build: downloaded at run time, with the person's consent, into
`Config::models_dir`, and verified by SHA-256 before every use.

| Mask | Model | Licence | Bytes | SHA-256 |
|---|---|---|---|---|
| Subject | BiRefNet lite, full precision (`onnx-community/BiRefNet_lite-ONNX`, `onnx/model.onnx`) | MIT (ZhengPeng7/BiRefNet) | 224,005,088 | `5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333` |
| Sky | U²-Net sky segmentation (`JianyuanWang/skyseg`, `skyseg.onnx`) | MIT (xiongzhu666/Sky-Segmentation-and-Post-processing) | 175,997,079 | `ab9c34c64c3d821220a2886a4a06da4642ffa14d5b30e8d5339056a089aa1d39` |

## What is unverified

- **Timing and memory on the Mac Studio (M1 Max, 32 GB)**, where the application is used. Measured
  only on an M4 Pro: subject 3.6–3.9 s a photograph including verification and loading, sky about
  0.7 s; the subject model peaks near 10 GB for one inference. MV-22.6 measures it there.
- **Quality on the owner's photographs.** Judged here on four Wikimedia Commons photographs only.
  MV-22.5.
- **A real model run inside the application.** The desktop's `make_auto_mask` is exercised in tests
  only up to its refusal when the model is missing; the model path itself is exercised through
  `core`'s `auto_mask` example, the same function the command calls. MV-22.5 runs it in the
  application.
