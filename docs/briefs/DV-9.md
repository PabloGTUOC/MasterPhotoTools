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
