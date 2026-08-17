# Manual Verification Steps

This document tracks verification steps that require human intervention, physical hardware, or visual judgement.

## Phase 2

- **Four of F1's seven date tags cannot be read from a real file yet.** `nom-exif` 3.6 exposes
  `EXIF:DateTimeOriginal`, `EXIF:CreateDate` and a single collapsed QuickTime `CreateDate`. It does
  not surface `QuickTime:CreationDate`, `Keys:CreationDate`, `XMP:CreateDate` or
  `QuickTime:ModifyDate` separately. The preference order is implemented and unit-tested at all
  seven positions, but positions 3, 5, 6 and 7 cannot currently be populated from a file. A human
  should check a real MOV from an iPhone (which carries `Keys:CreationDate`) and a file with an XMP
  packet against `exiftool` output, and confirm whether the resolved date matches.
- **EXIF fixtures are hand-built.** The generator writes its own TIFF/IFD structures rather than
  using a camera file. `exiftool` and `nom-exif` both read them correctly, but a human should run
  `read_meta` over real files from each camera body and compare against `exiftool -s`, especially
  for maker-note-heavy RAW containers.
- **EXIF preservation across a resize is verified structurally, not visually.** The round-trip test
  asserts the capture date, camera and pixel dimensions. A human should confirm on a real photograph
  that lens, exposure and GPS also survive, and that Google Photos files the result under the
  capture date rather than the upload date (specification §6.4).

## Phase 4

- **F4 (half-frame split) needs a human on real scans.** The synthetic fixture plants a hard-edged
  dark divider at a known column against flat colour panels. A real lab scan has a soft, uneven
  divider, film grain, dust, and a surround that is neither uniformly white nor uniformly black.
  Check on real Pentax 17 scans that: the divider is found rather than a dark part of the image; the
  lab border is fully removed without eating into the frame; and the trimmed halves are not cropping
  away picture content.
- **F7 (print border) needs a human on real scans.** The dark-edge trim thresholds (luma 28, 70% of
  a band, 40 px maximum) are the specification's numbers but have only been exercised against
  synthetic edges. Check that a genuinely dark photograph is not mistaken for a scan border, and
  judge whether the 2%-of-short-side corner radius looks right at print size.
- **Chroma subsampling and progressive encoding cannot currently be honoured.** The `image` crate's
  JPEG encoder is fixed at 4:2:2 with no progressive mode, so F4's and F7's "no chroma subsampling"
  and F8's "4:2:0, progressive, optimised" are not met. Judge whether the difference is visible in
  print and social output; this is the same encoder decision raised by the Phase 2 benchmark.
- **F5 captions use a built-in 5×7 bitmap font**, not a real typeface. Check that captions are
  legible at the intended cell sizes and print scale.

## Phase 7
- Requires macOS machine to build and run the Tauri application.

## Phase 8
- Requires a physical SD card for end-to-end ingest validation.

## Phase 10
- macOS ImageIO RAW decode path requires a macOS machine.
- Needs a human to judge colour quality on real RAW files from each camera body.

## Phase 14
- macOS `.dmg` bundle requires installation and launch testing on macOS.
- NAS deployment testing.
