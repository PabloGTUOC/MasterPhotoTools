# Rollchrome — manual verification

The checks for Rollchrome's Edit and Batch Grade screens that no test can settle. They moved here
from [`../manual-verification.md`](../manual-verification.md) when the editor left PhotoTools
(RC-3), and **their ids are unchanged**, so anything that names MV-20.4 still finds it.

How to work through them — tick the box, replace **Result:** with what actually happened, and a
failed check is a result — is as described at the top of that file. **60 checks**, none done yet
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

## MV-27 — Virtual copies (RP-6)

[`../briefs/RP-6.md`](../briefs/RP-6.md). Real photographs in the real application.

- [ ] **MV-27.1 — Colour and black & white versions exported together and surviving a restart.**
      **Run:** in Edit, open a photograph, adjust colours, and save a virtual copy named "B&W". On "B&W", convert to black & white (press `V`) and tune tonal sliders. Switch between "Main" and "B&W" to verify that edits on each remain intact. Click "Export all versions" to export both to a destination folder. Quit Rollchrome completely, reopen it, and reopen the same photograph.
      **Pass:** both versions appear with their respective names and recipes intact; switching between them updates the sliders and image without data loss; both exported images are in the destination folder with correct filenames (`_edit.jpg` and `_edit_b-w.jpg`).
      **Result:**

## MV-28 — Detail: sharpening, noise reduction, hot pixels (RP-7)

[`../briefs/RP-7.md`](../briefs/RP-7.md). Real photographs in the real application at 100% zoom.

- [ ] **MV-28.1 — Sharpening and noise reduction on a high-ISO photograph.**
      **Run:** in Edit, open a real high-ISO photograph (e.g. ISO 6400+). Press `Z` (or Cmd-1) to view at 100%. Open the Detail panel. Increase Sharpening Amount to 80, Radius to 1.2, and adjust Masking with Alt-held/visual feedback. Increase Luminance Noise Reduction to 40 and Colour Noise Reduction to 50. Toggle Remove Hot Pixels. Export the edited image.
      **Pass:** at 100% zoom, chroma noise (coloured specks) disappears without desaturating subject colours; luminance noise grain is subdued while sharp subject edges (eyelashes, text, high-contrast borders) remain crisp and defined; single-pixel hot sensor dots vanish; the exported file matches the 100% on-screen appearance.
      **Result:**

## MV-29 — Presence: clarity, texture, dehaze (RP-8)

[`../briefs/RP-8.md`](../briefs/RP-8.md). Real photographs in the real application at 100% and fit zoom.

- [ ] **MV-29.1 — Clarity on a portrait and a landscape, and Dehaze on a real hazy photograph.**
      **Run:** in Edit, open a real portrait photograph; increase Texture to +40 and Clarity to +30, then test negative Clarity (-30) and Texture (-20). Next, open a landscape photograph with foliage, rock relief, and soft clouds; increase Clarity to +50 and Texture to +40. Finally, open a landscape with atmospheric haze or mist; increase Dehaze to +60, and test negative Dehaze (-40) on a sunny scene to introduce haze. Export all results.
      **Pass:** on the portrait, positive clarity and texture accentuate iris and hair detail without haloing high-contrast edges; negative clarity softens skin tones while leaving deep blacks and specular whites untouched. On the landscape, foliage and textures gain local mid-tone contrast and edge punch without haloing against the sky. On the hazy photograph, positive dehaze cuts through the atmospheric wash to restore contrast, saturation, and depth without colour cast in neutral areas; negative dehaze adds mist/glow gracefully without clipping highlights; exports match the on-screen preview.
      **Result:**

## MV-30 — Export options (RP-9)

[`../briefs/RP-9.md`](../briefs/RP-9.md). Real exports opened in external applications (Preview, Photoshop, or browser).

- [ ] **MV-30.1 — Opening each format from a real export in another application.**
      **Run:** in Edit, open a real photograph and make several adjustments. In the Export Render section, export the photograph across all supported formats: JPEG at quality 90, TIFF at 16-bit, PNG at 8-bit, WebP (lossless), and AVIF at quality 70. Test resizing with Long Edge (e.g. 1920 px) and Megapixels (e.g. 12 MP), output sharpening (Screen and Print), and a custom name template `{date}_{name}_{version}_{edit}` with No Location metadata policy. Open each exported file in macOS Preview, a web browser (Safari/Chrome), or another image viewer.
      **Pass:** every file opens cleanly without decode errors or corrupted artifacts; JPEG matches the requested quality and dimensions; 16-bit TIFF displays full dynamic range; PNG and WebP show sharp pixel-exact rendering; AVIF opens with photographic fidelity at compact file size; dimensions strictly follow the resize constraint without enlargement; sharpened files exhibit clean edge acuity tailored for display/print; GPS location metadata is cleanly stripped when No Location is chosen while camera and capture timestamp remain intact; custom filename template substitution and monotonic numbering work accurately.
      **Result:**

## MV-31 — Owner's cameras: XZ-1, D60, and X100VI (RP-12)

[`../briefs/RP-12.md`](../briefs/RP-12.md). Develop real RAW captures and compare with camera JPEG.

- [ ] **MV-31.1 — Develop real XZ-1, D60, and X100VI RAW captures.**
      **Run:** in Develop, point the folder input at a directory containing real RAW captures from Olympus XZ-1 (.ORF), Nikon D60 (.NEF), and Fujifilm X100VI (.RAF, 40 MP). Verify that .ORF files are listed alongside .NEF and .RAF. Select an image, assign a look, adjust the exposure slider (noting fast draft rendering during dragging and full resolution on release), and develop each file. Compare the resulting developed image with the in-camera JPEG. Note: Olympus XZ-1 is verified with the real file; Nikon D60 and Fujifilm X100VI camera support is written and wired in rawler, and manual visual comparison awaits real captures.
      **Pass:** .ORF, .NEF, and .RAF are discovered and listed; thumbnails load upright without distortion; dragging exposure shows instant draft previews; releasing exposure settles to full preview; development produces clean JPEG/TIFF without artifacts; colors and tone curve match expectations; Olympus XZ-1 matches camera JPEG rendering.
      **Result:**

## MV-32 — Point colour (RP-10)

[`../briefs/RP-10.md`](../briefs/RP-10.md). Real photographs in the real application.

- [ ] **MV-32.1 — Single colour changed on a real photograph.**
      **Run:** in Edit, open a real photograph containing a distinct, saturated subject against contrasting surroundings (e.g. a red car, a yellow flower, a blue jacket, or a model in a vibrant dress). Open the Colour / HSL section and click "Pick a colour". Sample the subject by clicking directly on its surface. Observe that a new swatch appears, selected, with Hue, Saturation, Luminance, and Range sliders at their defaults. Adjust the Hue slider to shift the color (e.g. from red to purple or blue), increase Saturation (+40), and adjust Luminance (+20) and Range. Toggle Black & White on and off, and verify the point colour is bypassed in B&W and restored when returning to colour. Export the edited image.
      **Pass:** only the picked subject's colour changes smoothly; adjacent colors, skin tones, neutrals, and skies remain untouched; the transition at the edge of the range falloff is smooth without blotchy haloing or chromatic noise in near-neutrals; the exported photograph exactly matches the preview.
      **Result:**

## MV-33 — Masks: ranges and combining (RP-11)

[`../briefs/RP-11.md`](../briefs/RP-11.md). Real photographs in the real application.

- [ ] **MV-33.1 — Add.**
      **Run:** in Edit, create a base Linear gradient mask covering the top half. Add a Radial gradient component with `Add` operation covering a subject in the bottom half. Boost exposure on the mask (+1.5 EV).
      **Pass:** both regions receive the boost (the union of the two shapes); dragging the radial component handles adjusts its coverage without altering the base linear gradient.
      **Result:**

- [ ] **MV-33.2 — Subtract.**
      **Run:** in Edit, create a base Linear gradient mask covering an entire landscape. Add a Radial gradient or Luminance range component with `Subtract` operation over a foreground rock or subject. Apply an exposure adjustment (-1.0 EV).
      **Pass:** the subtracted region is cleanly punched out and remains unadjusted, with smooth falloff at the boundary.
      **Result:**

- [ ] **MV-33.3 — Intersect.**
      **Run:** in Edit, create a base Linear gradient mask over the sky/top region. Add a Luminance range component with `Intersect` operation targeting bright tones (Low 0.60, High 1.0, Feather 0.2). Adjust temperature/tint or exposure.
      **Pass:** only pixels that are both in the top linear gradient and in the bright luminance range receive the adjustment; darker areas in the top region and bright areas outside the top region are excluded.
      **Result:**

- [ ] **MV-33.4 — Luminance range.**
      **Run:** in Edit, click `+ Luminance range` to add a standalone luminance range mask. Adjust Low, High, and Feather sliders to target mid-tones (e.g. Low 0.25, High 0.75, Feather 0.2). Apply exposure (+1.0 EV) and contrast adjustments.
      **Pass:** shadows below Low and highlights above High remain untouched; mid-tones transition smoothly according to Feather; changing global exposure does not shift the mask's boundary.
      **Result:**

- [ ] **MV-33.5 — Colour range.**
      **Run:** in Edit, click `+ Colour range`, entering eyedropper picking mode. Click on a distinct coloured object (e.g. foliage, clothing, or blue sky). Inspect the picked Hue, Chroma, and Lightness values. Adjust the Range slider and boost saturation (+40) or shift exposure on the mask. Re-pick another colour using `Re-pick colour`.
      **Pass:** only the chosen colour range is affected with smooth OkLCh falloff; nearby neutrals and different hues remain unadjusted; re-picking updates the target colour accurately.
      **Result:**

- [ ] **MV-33.6 — Background.**
      **Run:** in Edit on a portrait or subject photo, click `+ Background`. If the subject model is not cached, verify download intent and auto-generation create an inverted subject mask named "Background". Adjust exposure and saturation on the mask.
      **Pass:** the subject remains untouched while the entire surround/background is adjusted cleanly without haloing around edges.
      **Result:**

## MV-34 — Masks: local detail (RP-11b)

[`../briefs/RP-11b.md`](../briefs/RP-11b.md). Real photographs in the real application.

- [ ] **MV-34.1 — Local clarity inside a mask.**
      **Run:** in Edit, open a real photograph and add a Linear gradient or Brush mask over a midtone texture region (e.g. foliage, architecture, or fabric). In the mask panel, adjust the local Clarity slider to +50 while keeping global Clarity at 0. Scrub exposure to verify cached frames render without delay.
      **Pass:** midtone local contrast increases smoothly only within the mask coverage; unmasked regions are completely unchanged; scrubbing exposure is smooth.
      **Result:**

- [ ] **MV-34.2 — Local texture inside a mask.**
      **Run:** in Edit, create a mask over fine details (e.g. hair, pores, wood grain). Increase local Texture to +60. Then test with Texture at -60 on skin.
      **Pass:** high-frequency fine surface details are accentuated or smoothed without affecting coarse tonal edges or introducing halos outside the mask.
      **Result:**

- [ ] **MV-34.3 — Local dehaze inside a mask.**
      **Run:** in Edit, create a mask over a hazy distant mountain or background. Increase local Dehaze to +40. Then create a mask over shadows with Dehaze +100 and apply a +1 EV exposure boost.
      **Pass:** atmospheric haze is penetrated only in the masked region; shadow range coverage is preserved and does not shrink or move under dehaze.
      **Result:**

- [ ] **MV-34.4 — Local sharpness inside a mask.**
      **Run:** in Edit, create a mask over a primary subject's eyes or focal point. Increase local Sharpness to +50 while global sharpening is at default.
      **Pass:** the masked region exhibits crisp high-frequency edge definition matching combined amount with global sharpening; unmasked areas retain standard sharpness.
      **Result:**

- [ ] **MV-34.5 — Export matches preview with local detail.**
      **Run:** on an image with two masks each using Clarity, Texture, Dehaze, and Sharpness, export the photograph to full-resolution 16-bit TIFF and 8-bit JPEG. Open the exported TIFF beside the Rollchrome 100% zoom preview.
      **Pass:** tone, local contrast, edge sharpness, and haze reduction in the exported file match the preview visually within tolerance.
      **Result:**

## MV-35 — Automation: Command Line and MCP Server ([RP-13](../briefs/RP-13.md))

[`automation.md`](automation.md), built. Needs a Mac with a real RAW folder and an MCP client (such as Claude Code or Claude Desktop).

- [ ] **MV-35.1 — Command-line develop matches desktop application.**
      **Run:** run `rollchrome-cli develop <folder> --look builtin:fujifilm-classic-chrome --apply` on a real RAW folder, then develop one of the same RAW files with `builtin:fujifilm-classic-chrome` in the Rollchrome desktop application and compare the two outputs.
      **Pass:** the command line writes into `<folder>/Developed/` without touching originals, and the resulting photograph matches the desktop application's output for the same look and exposure.
      **Result:**

- [ ] **MV-35.2 — MCP server plan-to-run workflow.**
      **Run:** register `rollchrome-cli mcp` with a real MCP client, inspect the available tools, ask the client to plan RAW development on a folder (`plan_develop`), review the plan, and instruct the client to execute it (`run_develop`) using the returned `plan_sha256`.
      **Pass:** the client discovers all seven tools, presents the dry-run plan before writing anything, and completes `run_develop` with the reviewed `plan_sha256`, writing the developed files into `Developed/`.
      **Result:**

- [ ] **MV-35.3 — MCP server cancellation from client.**
      **Run:** start a `run_develop` or `run_batch` tool call on a multi-photograph folder from the MCP client and cancel the in-flight request mid-run.
      **Pass:** the server honours `notifications/cancelled`, stops after the current photograph finishes without leaving partial files, and reports the photographs developed before cancellation.
      **Result:**

## MV-36 — Masks: click to select an object ([RP-11c](../briefs/RP-11c.md))

[`edit-plan.md`](edit-plan.md), built. Needs a Mac running Rollchrome (`crates/rollchrome-desktop`, `cargo tauri dev`) with a real photograph.

- [ ] **MV-36.1 — First click selects an object; further clicks add; Alt-click subtracts.**
      **Run:** in Edit, open a photograph containing distinct subjects and objects. Click **+ Object**. Click on a primary object (e.g. a car, cup, or animal). Observe the mask overlay. Click on an unselected portion of the same object to add it. Hold Option/Alt and click on an unwanted selected area to subtract it. Press Cmd-Z to verify each click undoes cleanly. Click **Done** or press Escape to finalize.
      **Pass:** the first click identifies and outlines the object cleanly; subsequent clicks add or subtract regions; each click is an undo step; the overlay displays the mask; Done finalizes the object mask.
      **Result:**

- [ ] **MV-36.2 — Model download prompt on missing model and intent preservation.**
      **Run:** with `models/` devoid of `mobile_sam_image_encoder.onnx` and `sam_mask_decoder_multi.onnx` (or moved aside), click **+ Object**. Confirm the download dialog prompts to download the ~56 MB MobileSAM model. Approve the download.
      **Pass:** a download progress indicator appears; once complete, the object selection mode activates immediately without needing to click **+ Object** again; clicks now segment objects normally.
      **Result:**

- [ ] **MV-36.3 — Component object mask and Refine mode.**
      **Run:** create an existing mask (e.g. a Linear gradient or Subject mask). Click **Add component** -> **+ Object** and click an object to add to the composite mask. Deselect and re-select the Object component (or an existing Object mask) and click **Refine**.
      **Pass:** the object is added as a component with `op: add`; clicking Refine enters object selection mode with all saved clicks restored, allowing further prompt clicks to refine the boundary; saving and reopening the photograph preserves the raster and clicks without requiring the model.
      **Result:**

## MV-37 — Masks: People, Subject and Background, and Sky guard ([RP-11d](../briefs/RP-11d.md))

[`edit-plan.md`](edit-plan.md), built. Needs a Mac running Rollchrome (`crates/rollchrome-desktop`, `cargo tauri dev`) with real photographs.

- [ ] **MV-37.1 — People mask isolates people; Subject and Background use full BiRefNet.**
      **Run:** in Edit, open a photograph containing people and other distinct foreground objects (e.g. a person with a bicycle). Click **+ People**. Note the mask boundary. Then add a **+ Subject** mask or **+ Background** mask.
      **Pass:** People mask cleanly isolates the person and omits non-person foreground objects (like the bicycle); Subject mask cleanly isolates the subject including fine hair details; Background correctly inverts the full subject mask.
      **Result:**

- [ ] **MV-37.2 — Guarded Sky mask rejects photograph without sky.**
      **Run:** open an indoor photograph or landscape with no sky. Click **+ Sky**.
      **Pass:** Sky segmentation is refused immediately with "no sky was found in this photograph"; no mask is created, and the sky segmentation model is not run; opening a photograph with sky segments the sky cleanly as before.
      **Result:**

- [ ] **MV-37.3 — Missing model downloads only required files.**
      **Run:** with `skyseg.onnx` present but `sky_guard.onnx` missing, click **+ Sky**. Confirm the prompt asks for 238 MB (UperNet ConvNeXt tiny) rather than the combined total. Approve download.
      **Pass:** only the missing guard model (238 MB) is downloaded; progress reports the single file's progress; once downloaded, the sky mask completes normally.
      **Result:**

## MV-38 — Film stock simulations in Develop (DV-4)

[`develop-plan.md`](../develop-plan.md), built. Needs a Mac running Rollchrome with real photographs.

- [ ] **MV-38.1 — Develop three RAWs across film stocks and grain settings.**
      **Run:** develop three of the owner's RAWs in Portra 160, Ektachrome 100 and Vision3 250D at each grain setting (Off, Fine, 35 mm).
      **Pass:** preview and exported file match by eye, grain Off has none, and a 40 MP X100VI file exports in under 10 s.
      **Result:**

## MV-39 — Noise reduction in Develop (DV-5)

[`develop-plan.md`](../develop-plan.md), built. Needs a Mac running Rollchrome with real photographs.

- [ ] **MV-39.1 — High-ISO RAWs through a film stock, with and without noise reduction.**
      **Run:** develop two of the owner's ISO 400 XZ-1 files in Portra 160, grain Off, at Noise reduction 0, 25 and 50.
      **Pass:** at 25 the colour speckle is gone and skin and fabric stay detailed at 100%; the preview and the exported file match by eye.
      **Result:**

## MV-40 — Film stock LUTs in another editor (DV-6)

[`develop-plan.md`](../develop-plan.md), briefed. Needs a Mac running Rollchrome and Photomator.

- [ ] **MV-40.1 — Save Portra 400 and Vision3 500T (neutral) as LUTs and apply them in Photomator.**
      **Run:** in Develop, choose each stock for a photograph, press Save as LUT, then load the file from `Developed` in Photomator on the same photograph's JPEG.
      **Pass:** Photomator accepts both files. The colours match Rollchrome's grain-off preview by eye. 500T neutral is not blue on a daylight photograph.
      **Result:**
