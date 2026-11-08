# Rollchrome — manual verification

The checks for Rollchrome's Edit and Batch Grade screens that no test can settle. They moved here
from [`../manual-verification.md`](../manual-verification.md) when the editor left PhotoTools
(RC-3), and **their ids are unchanged**, so anything that names MV-20.4 still finds it.

How to work through them — tick the box, replace **Result:** with what actually happened, and a
failed check is a result — is as described at the top of that file. **31 checks**, none done yet
(2026-11-06). They all need a Mac with real photographs, and MV-20.7 also a card and a mounted
share.

## MV-20 — the Edit tab and Bulk LUT

[`edit-plan.md`](edit-plan.md), built. Needs a Mac to run Rollchrome (`crates/rollchrome-desktop`, `cargo tauri dev`). All checks
run locally and need no server, but MV-20.7 needs a card and a mounted share.

- [ ] **MV-20.1 — Exposure matching against camera JPEG.**
      Linear exposure compensation must match optical exposure shifts without flattening highlights.
      **Run:** shoot a RAW frame at 0 EV and +1 EV; develop the 0 EV RAW to JPEG/TIFF in Develop (RP-2b withdrew RAW from Edit); adjust the 0 EV JPEG/TIFF to +1 EV in the Editor; compare the result against the camera's native +1 EV JPEG.
      **Pass:** mid-tone luminance and highlight rolloff match the optical +1 EV frame within visual tolerance (highlight recovery beyond the camera JPEG is not expected in v1).
      **Result:**

- [ ] **MV-20.2 — Film simulation LUT comparison.**
      Display-encoded 3D LUT sampling must reproduce film simulations identically to reference software.
      **Run:** apply a known film `.cube` LUT at 100% intensity in the Editor; compare the exported output side-by-side with the same LUT applied in reference grading software.
      **Pass:** color rendition, shadow tint, and contrast curve are visually indistinguishable from reference output.
      **Result:**

- [ ] **MV-20.3 — Neutral surround calibration.**
      The canvas surround must provide a true radiometric mid-grey ground that does not skew human exposure perception.
      **Run:** view the Editor canvas in dark and light modes beside an X-Rite 18% neutral grey card in daylight.
      **Pass:** the surround matches the card's lightness; mid-tones do not appear artificially washed out or crushed.
      **Result:**

- [ ] **MV-20.4 — Metadata and GPS preservation on export.**
      Exported derivatives must carry the original capture date, camera model, lens metadata, and GPS position, respecting naming and collision rules.
      **Run:** run `exiftool -s` on a photograph before and after exporting from the Editor; export a second time.
      **Pass:** `DateTimeOriginal`, camera tags, lens tags, and GPS coordinates match; output filename appends `_edit`; second export increments to `_edit_1` without overwriting; exporting to `Publishing` is refused.
      **Result:**

- [ ] **MV-20.5 — Bulk LUT folder run.**
      Grading an entire folder must operate reliably, non-destructively, and with full metadata retention.
      **Run:** in the Batch Grade tab with **3D LUT** chosen (the tab was called Bulk LUT), run over a folder of 50 images; cancel halfway through, then re-run to completion.
      **Pass:** cancellation stops cleanly without corrupted files; completed run writes all 50 files with `_lut` suffix; no originals are overwritten.
      **Result:**

- [ ] **MV-20.6 — Bulk LUT refusal on Publishing folder.**
      Bulk LUT must refuse to output directly into the `Publishing` folder (MV-16.7).
      **Run:** attempt to set the Bulk LUT output destination to the active `Publishing` directory.
      **Pass:** the tool refuses to start and displays an actionable error explaining that `Publishing` is reserved for reviewed outputs.
      **Result:**

- [ ] **MV-20.7 — Card media read-only safety.**
      Opening files directly from an SD card volume must never create sidecar files on the removable card (G5), while mounted network shares remain writable.
      **Run:** open a photograph directly from an SD card volume whose root contains a `DCIM` directory in the Editor; adjust sliders. Then open a photograph from a mounted SMB share without `DCIM`.
      **Pass:** on the card volume, the read-only banner is displayed and no `.photoedit` file is written; exporting requires selecting a destination on a local disk. On the mounted SMB share, sidecar saving works normally.
      **Result:**

- [ ] **MV-20.8 — Interactive slider drag timing on a 36 MP file in the running app.**
      Real Tauri IPC cannot be driven headless, so end-to-end responsiveness with real IPC and a full-resolution 36 MP frame must be verified interactively.
      **Run:** open a 36 MP JPEG or TIFF in the desktop Editor (developed from RAW via Develop if shooting RAW, per RP-2b; release build from `cargo tauri build`, not debug from `cargo tauri dev`); rapidly scrub an adjustment slider (Exposure or Highlights) back and forth across its full range for 5 seconds, then release the mouse.
      **Pass:** the preview updates fluidly during dragging without perceptible stutter or event backlog (sustaining ≥ 20 fps interactive response), and settles cleanly to the sharp 1440p frame within ~120 ms of mouse release.
      **Result:**

## MV-21 — Edit round 2

Judgement checks for the round 2 additions (ED-9 to ED-18). Like MV-20 they need the Mac and real
photographs, and no server.

- [ ] **MV-21.1 — Monotone tone curves and S-curve contrast.**
      Tone curve adjustments must create smooth photographic contrast curves without banding, overshoot, or channel cross-talk.
      **Run:** in the Editor, create an S-curve on the Luma channel; adjust individual Red and Blue curves to warm the highlights and cool the shadows; compare against a reference grading tool.
      **Pass:** curve transitions are completely smooth; no overshoot or gradient reversals occur near extreme highlights or deep blacks; chromaticity ratios are preserved.
      **Result:**

- [ ] **MV-21.2 — 8-band HSL selective hue isolation.**
      HSL adjustments must isolate designated color bands smoothly without contouring or fringing artifacts along color boundaries.
      **Run:** select a portrait with green foliage and blue sky; shift Green hue towards yellow, drop Blue saturation by 50%, and increase Red/Orange luminance by +20%.
      **Pass:** foliage turns autumn yellow, sky desaturates cleanly, and skin tones gain exposure without haloing, banding, or color stepping.
      **Result:**

- [ ] **MV-21.3 — 3-way colour grading mood and tonal balance.**
      Shadows, Midtones, and Highlights wheels must apply photographic split-toning while preserving natural skin tones.
      **Run:** drag Shadows wheel to teal/cyan, Highlights wheel to warm amber/gold, and adjust the Tonal Balance slider.
      **Pass:** shadows are cleanly tinted without muddying mid-tones; highlight rolloff stays natural; adjusting Balance shifts the transition boundary predictably.
      **Result:**

- [ ] **MV-21.4 — Geometry: Straighten crop-to-fit and orientation baking.**
      Straightening an off-level horizon must automatically crop unrendered corners without black wedges, baking orientation to 1 on export.
      **Run:** open an image captured in portrait orientation (EXIF Orientation 6); apply +7.5° straighten and a 4:5 crop preset; export. Inspect the exported file with `exiftool`.
      **Pass:** the horizon is level; the frame fills the 4:5 aspect ratio without unrendered borders; `exiftool` reports `Orientation: Horizontal (normal) (1)` and pixels display upright in all viewers.
      **Result:**

- [ ] **MV-21.5 — Scale-independent vignette and grain on preview vs export.**
      Vignette falloff and film grain structure must appear identical on the 1280px interactive preview and the full-resolution export.
      **Run:** apply a strong vignette and medium grain to a 36 MP photograph; view the interactive preview at 100% zoom; export the image; downscale the export to 1280px and compare side-by-side.
      **Pass:** vignette falloff profile and grain density/appearance are visually identical between proxy preview and full export.
      **Result:**

- [ ] **MV-21.6 — Deterministic grain byte-for-byte reproducibility.**
      Procedural grain generation must be strictly deterministic and seeded from the source photograph.
      **Run:** export a photograph with grain twice in succession to separate files; run `cmp` or `shasum` on the pixel buffers.
      **Pass:** the two output files are bit-for-bit identical.
      **Result:**

- [ ] **MV-21.7 — Optional Narkowicz ACES tone mapper highlight compression.**
      Enabling the filmic tone mapper must compress blown specular highlights into display white smoothly without harsh clipping.
      **Run:** open an over-exposed photograph with blown sky; toggle the Tone Mapper option on and off.
      **Pass:** with the tone mapper on, harsh white blowout transitions into a soft, photographic rolloff, and the panel says in plain words that the filmic tone curve is on and mid-tones shift.
      **Result:**

- [ ] **MV-21.8 — Preset saving, library management, and clipboard copy/paste.**
      Saving custom adjustments as a preset and copying settings between frames must work seamlessly.
      **Run:** tune a photograph with curves, HSL, and grading; save as a preset named "Summer Portra". Open a second photograph from the same shoot, crop it, apply the preset, then press Undo in the confirmation. Press `Cmd+C` on the first photograph, open a third, press `Cmd+V`. Rename the preset, then delete it.
      **Pass:** the preset and the paste change the look but not the crop; Undo restores the previous settings; `Cmd+C` and `Cmd+V` in the path field still copy and paste text; the preset appears in the picker, renames, and is gone from disk once deleted.
      **Result:**

- [ ] **MV-21.9 — Bulk preset folder run with content hash lock discipline.**
      Applying a full preset across a batch of photographs must enforce dry-run verification and content hash locking.
      **Run:** save a preset that uses a LUT, a curve and grain. In Batch Grade choose **Preset**, dry run over a folder of 30 photographs, and check the sample previews. Change the preset in Edit (Save as preset, Replace) and press Run without a new dry run. Then dry run again and run.
      **Pass:** changing the preset or any setting in Batch Grade locks Run and says why; the run after the preset was replaced is refused with a message asking for another dry run, and writes nothing; the final run writes `<name>_edit.<ext>` for each photograph, uncropped, looking like the same photograph exported from Edit with that preset, with capture date, camera and GPS preserved (`exiftool -s`).
      **Result:**

- [ ] **MV-21.10 — Interactive slider responsiveness with all Round 2 controls active.**
      The application must maintain fluid interactive scrubbing with all Round 2 additions enabled on a 36 MP frame.
      **Run:** open a 36 MP photograph; enable curves, 8-band HSL, 3-way grading, vignette, grain, and glow; rapidly scrub the Exposure slider for 5 seconds; release.
      **Pass:** scrubbing remains responsive and fluid (sustaining $\ge 20$ fps interactive updates without backlog), settling to retina sharpness within ~120 ms of release.
      **Result:**

- [ ] **MV-21.11 — Histogram and clipping warnings on a real photograph (ED-15).**
      **Run:** open a photograph with a bright sky. Raise Exposure until the sky blows; turn on Highlight clipping, then Shadow clipping; scroll the adjustment panel; export.
      **Pass:** the histogram moves with the slider and stays in view while the panel scrolls; the red overlay covers exactly the sky that blew and the percentages under the histogram agree with what is marked; the overlay never appears in the export.
      **Result:**

## MV-22 — Masks (edit round 3)

Judgement and timing checks for masks (ED-19 to ED-23), on the Mac the application is used on: the
Mac Studio, M1 Max, 32 GB. No server.

- [ ] **MV-22.1 — A graduated sky.**
      **Run:** on a landscape, add a Linear mask, drag it down from the top to the horizon; give it −0.7 EV and −20 temperature; turn on Show mask overlay; crop and straighten the photograph; export.
      **Pass:** the darkening fades smoothly with no visible edge; the overlay shows exactly where it acts; after the crop and straighten the gradient still sits on the same sky; the export matches the preview.
      **Result:**

- [ ] **MV-22.2 — A radial on a portrait.**
      **Run:** add a Radial mask over a face, Invert it, lower exposure and saturation outside; drag the width handle to turn the ellipse; nudge the centre with the arrow keys.
      **Pass:** the face is untouched and the surround darkens with a soft edge; the handles are easy to grab (40 px) and move the shape where expected.
      **Result:**

- [ ] **MV-22.3 — A brushed dodge and burn.**
      **Run:** add a Brush mask; paint a light stroke across a shadowed area with +0.5 EV; erase part of it with ⌥; change the size with [ and ]; paint a long stroke quickly.
      **Pass:** painting keeps up with the pointer (no lag on a long stroke); erasing removes painting cleanly; painting over an erased area brings it back; the export matches the preview.
      **Result:**

- [ ] **MV-22.4 — Masks stay with their photograph.**
      **Run:** with masks on a photograph, save a preset, copy settings, and run Batch Grade with that preset on another folder; paste the settings onto another photograph that has its own mask.
      **Pass:** no preset, paste or batch output carries the first photograph's masks; the second photograph keeps its own mask.
      **Result:**

- [ ] **MV-22.5 — Subject and sky on your photographs (ED-23).**
      **Run:** press + Subject on a portrait and on a photograph of a pet or object; press + Sky on three landscapes, one with low cloud at the horizon and one with buildings. The first press of each asks to download its model: accept. Refine one mask with the brush.
      **Pass:** each model is downloaded once, with its size stated, and not again; the masks follow the subject's outline and the sky's edge closely enough that a brush touch-up is the most needed; the mask is saved with the photograph and the export matches the preview. Note any photograph where the mask is wrong enough to be useless.
      **Result:**

- [ ] **MV-22.6 — Automatic mask timing and memory on the M1 Max.**
      **Run:** `cargo run --release -p rollchrome-core --features segment --example auto_mask -- <models dir> <out dir> <three of your photographs>`, with Activity Monitor open on the Memory tab.
      **Pass:** record the subject and sky times printed (measured on an M4 Pro: about 3.7 s and 0.7 s) and the peak memory (about 10 GB for the subject). Acceptable if the subject takes under 8 s and the Mac does not swap.
      **Result:**

## MV-23 — the neutral development (DV-1)

[`../briefs/DV-1.md`](../briefs/DV-1.md). Needs real RAW files, one Fujifilm (X-Trans) and one Bayer,
each with the camera's own JPEG of the same frame. No application: the measuring tool writes
sRGB PNGs.

- [ ] **MV-23.1 — Colour and no camera look.**
      **Run:** `cargo run --release -p rollchrome-core --example develop -- <out dir> <a RAF> <a Bayer RAW>`; open each PNG beside the camera's JPEG.
      **Pass:** similar colour and white balance; no film simulation (a Velvia frame is not Velvia); the clipped areas are white, never magenta. Record any frame that is clearly off.
      **Result:**

- [ ] **MV-23.2 — No maze on X-Trans.**
      **Run:** the RAF's PNG from MV-23.1, at 200% on fine detail: foliage, fabric, brickwork.
      **Pass:** no maze or worm pattern, and no coloured fringes beyond the camera JPEG's. Lens distortion and vignetting are expected to differ, since DV-1 applies no lens correction.
      **Result:**

- [ ] **MV-23.3 — Time per photograph on the Mac Studio.**
      **Run:** the same command on a 24 MP and on a 40 MP-plus RAW.
      **Pass:** record the printed times. They set Develop's budget; there is no pass threshold yet.
      **Result:**

## MV-24 — the Develop screen (RP-1, RP-2)

[`../briefs/RP-1.md`](../briefs/RP-1.md). Needs a folder of real RAW files on the Mac, in the real application:
the automated check drives the screen against a stub.

- [ ] **MV-24.1 — A look each, then develop.**
      **Run:** Develop tab, choose the folder. Give one photograph one look and a selection another. Dry run, Develop.
      **Pass:** thumbnails appear upright; each photograph's preview shows its look; `Developed/` holds one file per photograph, each with the look it was given.
      **Result:**

- [ ] **MV-24.2 — The preview keeps up.**
      **Run:** drag the exposure slider end to end, then click quickly through several photographs.
      **Pass:** the Mac stays responsive and the preview settles on the last choice within one development. Record the seconds for a first look at a photograph and for changing look on it.
      **Result:**

- [ ] **MV-24.3 — Develop to Edit keeps its colour (needs RP-2).**
      **Run:** develop one photograph to both JPEG and TIFF. Open the TIFF in Edit, beside the JPEG in Preview.
      **Pass:** the same colours, not muted. Export it from Edit without changes to a slider and with one change: both look right in Preview.
      **Result:**

- [ ] **MV-24.4 — Zoom and compare (RP-3).**
      **Run:** in Edit, a landscape and a portrait photograph, one of them cropped: Cmd-1, pinch, drag to pan, then `Y` and Shift-Y.
      **Pass:** at 100% the picture is sharp and the right shape, it cannot be dragged out of view, and the before lines up with the after.
      **Result:**

## MV-25 — Colour tools (RP-4)

[`../briefs/RP-4.md`](../briefs/RP-4.md). Real photographs in the real application, one of them portrait.

- [ ] **MV-25.1 — Eyedropper.**
      **Run:** press `W`, click something that should be neutral, on a landscape and on a portrait photograph.
      **Pass:** that spot turns neutral, and Undo puts it back.
      **Result:**

- [ ] **MV-25.2 — Auto.**
      **Run:** Auto on a dark, a bright and a well-exposed photograph.
      **Pass:** better or unchanged, never worse: no blown highlights brought on by Auto. Record any photograph it spoils.
      **Result:**

- [ ] **MV-25.3 — Black & white.**
      **Run:** press `V`, move the Blue and Green sliders, add a colour grade. Export.
      **Pass:** no colour without the grade; the skies and foliage respond; the export matches the screen.
      **Result:**

## MV-26 — Undo, history and snapshots (RP-5)

[`../briefs/RP-5.md`](../briefs/RP-5.md). Real photographs in the real application.

- [ ] **MV-26.1 — Undo across a real editing session.**
      **Run:** in Edit, open a photograph and make several adjustments: sliders, black & white, crop, and a mask. Use Cmd-Z and Cmd-Shift-Z to step backward and forward through changes, and jump between states using the History list.
      **Pass:** every change steps backward and forward smoothly without orphaned state or skipped steps; clicking in the History list restores the exact chosen state; deselects mask if undone.
      **Result:**

- [ ] **MV-26.2 — Snapshot surviving an application restart.**
      **Run:** in Edit, create an edit, save a snapshot named "Grade A", make further adjustments and save "Grade B". Quit Rollchrome completely, reopen it, and load the same photograph.
      **Pass:** both "Grade A" and "Grade B" appear in the Snapshots list; restoring each reproduces its exact recipe; deleting removes it and persists across reload.
      **Result:**
