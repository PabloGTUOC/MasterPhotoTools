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
- F4 (Half-frame split): Needs a human to judge colour quality and split accuracy on real scans.
- F7 (Print border): Needs a human to judge the border trim on real scans.

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
