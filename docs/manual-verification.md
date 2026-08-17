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
- **Encoder output is now to specification but needs a visual check.** F4 and F7 write 4:4:4 and F8
  writes progressive 4:2:0, verified from the files. mozjpeg's quantisation tables differ from the
  previous encoder's, so the same nominal quality number produces a different-looking file. Compare
  a print-size F7 output and an F8 scan conversion against the old output and confirm the change is
  an improvement, not just a difference.
- **F5 captions use a built-in 5×7 bitmap font**, not a real typeface. Check that captions are
  legible at the intended cell sizes and print scale.

## Phase 6

- **The 390 px check proves geometry, not feel.** `npm --prefix frontend/web run check:layout`
  asserts that no route scrolls sideways and that every control is at least 40 px tall, and writes
  screenshots to `frontend/web/layout-proof/`. Whether the tool views are actually pleasant to drive
  one-handed — reachable thumb targets, sensible keyboard behaviour on iOS and Android, readable
  contrast in sunlight — needs a person with a real phone.
- **Sign-in cannot be exercised until a Firebase project exists.** Set the four `VITE_FIREBASE_*`
  variables and confirm: Google sign-in completes, the ID token reaches the server, and an expired
  token triggers a silent refresh and retry rather than a login screen (§5.3).

## Phase 7

Everything below needs a Mac. The Rust command layer, the server connection and the
Tauri front end all build and are tested on Linux; the parts that cannot be are:

- **Confirm the app builds and launches on macOS.** `cargo tauri dev` from `crates/desktop`, or
  `npm --prefix frontend/desktop run build` then `cargo tauri build`.
- **Run an F1 date scan on a local folder from the running app** and confirm the table appears.
  The scan itself is tested headlessly; what is unverified is the `invoke` round trip and rendering.
- **Stop `phototools-server` and confirm the app still starts**, shows "Server offline" with a
  reason, and that the local tools still run. The underlying behaviour is tested; the indicator is
  not.
- **Confirm the refresh token round-trips through the macOS Keychain.** `keyring` has no usable
  backend in a headless Linux container, so `credentials.rs` is only tested for the shape of its
  failure. Sign in, quit, relaunch, and confirm the session survives — and that the token appears in
  Keychain Access under `com.phototools.master`, not in a file.
- **Confirm job progress arrives as Tauri events.** Start a long rename and watch the progress bar
  move, then quit mid-job and relaunch to confirm the job is reported interrupted rather than
  vanishing (F17).

## Phase 8
- Requires a physical SD card for end-to-end ingest validation.

## Phase 10
- macOS ImageIO RAW decode path requires a macOS machine.
- Needs a human to judge colour quality on real RAW files from each camera body.

## Phase 14
- macOS `.dmg` bundle requires installation and launch testing on macOS.
- NAS deployment testing.
