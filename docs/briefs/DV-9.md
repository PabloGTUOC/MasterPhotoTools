# DV-9 · Lens correction from the lensfun database

> Plan: [`rescope-plan.md`](../rescope-plan.md) step DV-9. Branch `dv-9`. Implemented by Claude while agy is
> out of credits, at the owner's instruction (2026-11-13: "carry on with all dev points"). The review states
> that the reviewer was not independent.

## Design note

**What each of the owner's cameras gives** (measured 2026-11-14 with `rawler` on the owner's files and a
chart RAF):

| Camera | What `rawler` reads | What lensfun has |
|---|---|---|
| Olympus XZ-1 (fixed 6–24 mm) | make `Olympus`, model `XZ-1`, focal 6–24, f-number. No lens name. | Camera `XZ-1` on mount `olympusxz1`, and one lens on that mount, *"XZ-1 & compatibles (Standard)"*, with **distortion** (ptlens) at 25 focal lengths. |
| Fujifilm X100VI (fixed 23 mm) | (not measured: no file; the X-Pro2 RAF gave make, model, focal, f-number **and** the lens name) | Camera `X100VI` on mount `fujix100v2`, and *"X100V & compatibles"* with **distortion, TCA and vignetting**. |
| Nikon D60 + kit lens | (not measured: no NEF) | Camera `D60`; *"AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G VR"* and its siblings. `rawler` resolves Nikon lens ids itself, so the lens name should be there. **Unverified.** |

**The source of the corrections: lensfun's database**
- CC BY-SA 3.0, the same kind of terms as the spektrafilm film data already shipped.
- **Shipped in the application,** `crates/rollchrome-core/data/lensfun/`: 5.1 MB of XML, 1,580 lenses,
  taken at lensfun commit `52cf1a7` (2026-11-04).
- **Not downloaded,** because the application must work offline.
- **Parsed once,** on first use, with `quick-xml`. It is already in the lock file as a transitive
  dependency; the direct dependency is recorded here (G8).

**The models** are lensfun's own, as its manual defines them. greycard (GPL-3.0) was read as a reference for
the coordinate conventions, and no code was copied:

| Correction | Model | Radius 1 is | Notes |
|---|---|---|---|
| **Distortion** | `ptlens`: r_d = r (a r³ + b r² + c r + 1 − a − b − c) | Half the shorter side | Also `poly3` and `poly5` |
| **Lateral CA** | `poly3`: r_c = r (v + c r + b r²) for red and blue | Half the shorter side | Also `linear` |
| **Vignetting** | `pa`: gain 1 + k1 r² + k2 r⁴ + k3 r⁶ | Half the diagonal | Held at r = 1. A sixth-order fit bends back up past the corner it was measured to, a point greycard's notes make. |

**Converting to the calibration's sensor:**
- **Distortion and CA:** scaled by the ratio of the two sensors' shorter sides, from crop factor and
  aspect ratio.
- **Vignetting:** scaled by the ratio of the two diagonals.
- **For the owner's three cameras both ratios are 1.**

**Interpolation**
- **Distortion and CA:** linear in focal length between the two nearest calibrations, held at the ends.
- **Vignetting:** bilinear in focal length and f-number, at the largest calibrated distance (focus at
  infinity).

**Where it runs:** on the linear image straight after demosaicing (`develop_linear`), before noise
reduction and before the look.
- It works in **sensor orientation**, which is what the models assume.
- It is **one resample**: each output pixel finds where its light landed through distortion, red and blue
  each through CA, divides by the vignetting gain there, and samples bilinearly.
- **Scale:** the largest magnification that leaves no empty edge, found by bisection over the border.

**Matching**
1. **The camera:** lensfun camera whose maker contains, or is contained in, the file's make, and whose
   model equals the file's model. Compared without case, spaces or hyphens, and including the `lang="en"`
   alias.
2. **The lens name, if the file has one:** the lens on the camera's mount (or a compatible mount) whose
   normalised model equals it.
3. **No lens name:** the lens on the camera's own mount, if exactly one exists. That is how a fixed lens is
   found.
4. **Otherwise:** **no correction**, with a reason. **A wrong profile is worse than none.**

## Brief

### Build
- **Module `media::lens`:** the database (parse, camera and lens records, calibrations), the models with
  interpolation, matching, and `correct_lens(linear, &LensInfo) -> Result<LensOutcome, Error>`.
  - `LensInfo` comes from `lens_info(raw)`: make, model, lens name, focal, f-number, from `rawler`'s metadata.
  - `LensOutcome` says what was applied (distortion, CA and vignetting, each yes or no; the profile's name;
    the scale), or why nothing was.
- **`DevelopItem` and `DevelopAction`:** gain `#[serde(default)] lens_correction: bool`, skipped when false,
  so old plans, hashes and bytes are unchanged.
- **Running it:** `develop_one` applies the correction when the flag is set. A file with no profile is
  developed uncorrected, and its reason is reported in the summary's new `lens_skipped`.
- **Preview:** `DevelopPreview::new_with_lens`. The desktop preview cache key becomes (path, lens).
- **Desktop:** `preview_develop` gains `lens_correction`.
- **CLI and MCP:** `--lens` and `lens_correction`.
- **Screen:**
  - a **Lens correction** checkbox for the selection, on by default for new photographs, as noise
    reduction's 25 is;
  - the preview follows it;
  - changing it locks Run.

### Acceptance
- **Database:**
  1. It parses, with more than 1,000 lenses.
  2. The XZ-1 matches by camera.
  3. The X-Pro2 with "XF35mmF1.4 R" matches by name.
  4. An unknown camera does not match, and says why.
- **Models:**
  5. ptlens at the edge of the documented values.
  6. Focal interpolation lies between its neighbours and is held outside them.
  7. The vignetting gain is held past r = 1.
- **Correction on synthetic images:**
  8. A grid barrel-distorted with a known ptlens comes back straight, within 0.5 px of straight at the
     edges.
  9. A flat field darkened with a known `pa` comes back flat, within 1 %.
  10. Red and blue images shifted by a known CA land back on green, within 0.3 px.
  11. No output pixel samples outside the source after the auto scale.
- **Develop:**
  12. `lens_correction: false` gives the bytes it always did.
  13. True on a synthetic DNG with no profile develops, and reports the reason in `lens_skipped`.
  14. A plan without the field deserialises to false.
- **Measured on the owner's XZ-1 files:** whether a profile matched at each focal length, and the time added
  per photograph.
- **`check:develop` 30:** the checkbox exists, defaults on, is carried into the dry-run items, and locks Run
  when changed.
- **All gates pass.**

## Report

Implemented by Claude on 2026-11-14.

**What was done**
- **`media::lens`:**
  - `db` parses the 56 lensfun files: 1,059 cameras, 1,580 lenses and 52 mounts, in 89 ms on first use.
  - `model` holds distortion (ptlens, poly3, poly5), TCA (linear, poly3) and vignetting (pa); the
    interpolation; the fill scale; and the single bilinear resample.
  - `mod` holds `lens_info`, which reads make, model, lens name, focal length and f-number through
    `rawler`'s metadata; the matching; and `profile_for` and `correct`.
- **`develop::correct_lens`:** applied in `develop_one` straight after `develop_linear`, and in
  `DevelopPreview::new_with_lens`. The desktop preview cache is keyed on (path, lens).
- **`lens_correction`:**
  - on `DevelopItem`, `DevelopAction` (skipped in serialisation when off) and `DevelopRequest`;
  - CLI `--lens`, MCP `lens_correction`;
  - `DevelopSummary.lens_skipped` for photographs developed uncorrected, with the reason.
- **Screen:** a Lens correction checkbox for the selection, on by default; the preview follows it.
- **Data:** `data/lensfun/` holds the XML, `COPYING.CC_BY-SA_3.0` and a README recording lensfun commit
  `52cf1a7`. `quick-xml` is recorded in `known-gaps.md` (G8).

**Measured on the owner's six XZ-1 ORFs** (neutral JPEG, one at a time, release build): all six matched
*XZ-1 & compatibles (Standard)*, and none was reported in `lens_skipped`.

| | Per photograph |
|---|---|
| Without correction | 1.34 s |
| With correction | 1.26 s |

The two times are within run-to-run noise: the resample costs less than the measurement varies.

Also matched by name in a probe: the chart X-Pro2 RAF's *XF35mmF1.4 R* (distortion, TCA and vignetting).

**Mechanical test changes:** `lens_correction: false` added to the existing `DevelopItem`,
`DevelopAction` and `DevelopRequest` literals. The 14 calls to `preview_develop_impl` in
`develop_desktop.rs` gained `false`. No assertion changed.

**Found while testing:** the first run of
`distortion_is_interpolated_between_focal_lengths_and_held_outside` compared floats exactly (−0.20000002
against −0.2). The new test was fixed to use a tolerance; the code was right. Clippy asked for one match
guard in the parser.

**Gates** (`gate-dv9.log`)

| Gate | Result |
|---|---|
| fmt / clippy / build | rc 0 |
| `--workspace` | **1162 passed**, 0 failed, 5 ignored (1148 + 14) |
| `phototools-core` | 543 |
| `rollchrome-core` | **483** (469 + 14) |
| `--features film` | **504 passed, 3 ignored** |
| `phototools-desktop` | 26 |
| `rollchrome-desktop` | 28 |
| `rollchrome-cli` | 8 |
| `check:develop` | **30 / 30** |
| Every other check and build | pass |

**Unverified**
- A Nikon D60 NEF and an X100VI RAF: no file of either was available. lensfun has both, but whether
  `rawler`'s lens name for the D60's kit lens matches lensfun's spelling is unknown.
- How the corrected XZ-1 photographs look: the tests prove the geometry on synthetic pictures. This is
  MV-44.

## Review

**Approved**, with the caveat that **the reviewer wrote the code**, so this is not an independent review.

**What was checked**
- **Gates:** run on the branch.
- **Off by default in data:** a plan with lens correction off serialises exactly as before (test 12), so old
  plans, hashes and bytes hold.
- **Refusal:** a file with no profile develops uncorrected and says why (test 13). An unknown lens is
  refused rather than guessed (test 4).
- **The geometry is measured, not assumed:**
  - a known distortion is straightened to under 0.5 px (it started visibly bent, over 2 px);
  - a flat field returns within 1 %;
  - fringes of over 1.5 px return within 0.3 px;
  - no edge pixel samples outside the frame. The test calls the same `source_pixel` that `apply` uses.
- **Licences:** lensfun data CC BY-SA 3.0, shipped with its licence and provenance. greycard was read, not
  copied.
- **G8:** `quick-xml` and `libc` are recorded.

