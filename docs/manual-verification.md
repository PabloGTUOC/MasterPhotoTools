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

The scan, pairing, fingerprinting and staging are all tested headlessly against simulated cards
(build plan §6.3), so what needs a human is the part that only exists on macOS with hardware
attached.

- **Insert a real card and confirm the notification appears**, reading
  `EOS_DIGITAL — 412 new shots. Review?` with the card's actual label and a plausible count. The
  text and the count are tested; that macOS raises the notification at all is not, and
  `tauri-plugin-notification` needs the user to have granted notification permission to the app.
- **Confirm the debounce is long enough for a real card reader.** It is set to 1.5 s, chosen from
  the shape of the problem rather than from measurement: a mount is several events, and looking too
  early finds an empty directory and concludes there is no `DCIM`. If a real reader is slower than
  that, detection will silently miss cards — so watch for a card that mounts but raises nothing, and
  raise `DEBOUNCE` in `crates/desktop/src/detection.rs` if it happens.
- **Confirm `/Volumes` is watchable.** `notify` uses FSEvents on macOS; the watcher is exercised
  against a stand-in directory in tests, and `/Volumes` specifically is not.
- **Confirm a non-card volume raises nothing.** Mount a backup drive or a USB stick with no `DCIM`
  and confirm no notification appears.
- **Confirm a reinserted card with nothing new on it raises nothing**, and that after shooting more
  frames it announces only the new ones. This is tested against simulated cards, but the volume
  label it keys on comes from the real mount point.
- **Scan a real card and confirm it is byte-identical afterwards** (G5). The test proves this
  against a read-only fixture; a real card is the case that matters. Compare with
  `find /Volumes/CARD -type f -exec shasum {} +` before and after.
- **Record how long a real 400-shot card takes to scan.** The measured 44 ms is on 64x48 fixtures
  and is not the real figure — see the phase report's Measurements section, which explains why a
  real card is expected to take several seconds and may exceed the §9.1 budget on a slow reader.

## Phase 9

The rules, the boundaries and the EXIF round trip are all tested against real files, so what is left
is judgement rather than correctness.

- **Look at a resized frame next to its original.** The tests prove the capture date survives and the
  dimensions land under the ceiling; they say nothing about whether a 24 MP frame reduced to 10 MP
  still looks right. Check a few with fine detail — foliage, fabric, text.
- **Confirm the 30-day batch spread suits how you actually shoot.** F12 gives 30 days for the
  camera-clock check, and `BATCH_SPREAD_DAYS` reuses it to decide when a frame is "far from the batch
  median", which the specification does not give a figure for. If you routinely leave a card in the
  camera for six weeks, frames from the start of it will be flagged as warnings.
- **Confirm the clock-offset suggestion is right before applying it in anger.** The shift comes from
  the median, so a card holding two shoots with different clock errors will get one correction that
  suits neither. The dry run shows the resulting date per frame; read it before applying.
- **Judge whether the quality ladder's bottom rung is acceptable.** A frame that cannot meet the byte
  cap at quality 75 is written anyway and reported in `still_too_large`. Whether 75 is low enough to
  publish, or whether such a frame should be resized further instead, is a taste question.

## Phase 10

Rung 1 — the embedded preview — is fully tested, and it is the rung that will handle nearly every
real file. What cannot be tested here is everything about how the results *look*, and the two rungs
below it.

- **Judge colour quality on real RAW files from each camera body.** This is the item the build plan
  names. The tests prove a JPEG comes out with the right dimensions and the right date; whether the
  colour and tone are right is a judgement no assertion can make. Compare a derived JPEG against the
  camera's own JPEG for the same frame if you have one.
- **Confirm the embedded preview is full-resolution on your bodies.** The extractor takes the largest
  embedded JPEG, which on most cameras is the full-size render — but some bodies write only a
  screen-sized preview, and a 1616×1080 derivative from a 45 MP frame would pass every test here
  while being useless. Check the dimensions of a derived file against the RAW's own.
- **Confirm rung 2 runs on macOS.** `sips` is invoked only on macOS and cannot be exercised on Linux
  at all. Force it by deriving from a RAW with no embedded preview, and confirm the reported rung is
  `macos_imageio` rather than `rawler`.
- **Judge whether rung 3's output is acceptable when it is reached.** `rawler`'s development pipeline
  is a generic demosaic with no camera-specific tuning, so its colour will not match either the
  camera's own render or Apple's. It is the only rung available on the Linux server, so the question
  is whether server-side derivation is worth having at all or should be refused in favour of doing it
  on the Mac.
- **Confirm CR3 files are declined rather than mangled.** F14 puts CR3 out of scope; a CR3 on a card
  will be reported as underivable. Confirm that reads as a clear message rather than a crash.

## Phase 11

The protocol is fully tested and the ledger is a file on disk that outlives the process. What cannot
be tested here is the **medium**: every test in this phase writes to a local temporary directory, and
the real staging directory is an SMB share mounted from a NAS. That is a different filesystem with
different guarantees, and three of the five items below are about exactly that.

- **Confirm `rename` is atomic enough on your SMB share.** The copy writes `<hash>.jpg.partial` and
  renames it into place, so the server never sees a half-written file under its final name. On a
  local POSIX filesystem that rename is atomic; over SMB it is a server-side operation whose
  atomicity depends on the NAS. If it is not atomic, the failure is benign — verification fails and
  the file is recopied — but it would happen on *every* file, turning one pass into two. Watch the
  `recopied` count on a real card: it should be zero.
- **Confirm the server can read a file the Mac has just finished writing.** SMB clients cache
  writes, so a file can look complete on the Mac before all of it has reached the NAS. The recopy
  path covers this correctly, but again: if it fires on most files rather than none, the handoff is
  doing double the work and the cause is write caching, not corruption.
- **Time a real card, and check the two timeouts against it.** Verification hashes every arrival and
  is allowed 30 minutes (`VERIFY_TIMEOUT`); each individual HTTP call is allowed 60 seconds
  (`HANDOFF_TIMEOUT`). Neither has ever been measured against a real card over a real network. A
  400-frame card is 1–3 GB, and the hashing runs against the NAS's own disks rather than the share.
- **Set `ADMIN_TOKEN` and confirm the desktop can authenticate.** The desktop has a Keychain but no
  Firebase sign-in yet, so today the handoff authenticates with §5.3's break-glass token, set in
  `ServerSettings.auth_token`. Without it every request is a `401`. This is the one thing that stops
  the handoff working end to end today, and it is configuration rather than code.
- **Fill the NAS and watch what happens.** A copy that runs out of space fails the handoff with the
  underlying I/O error rather than reporting a hash mismatch, which is the right answer — but nobody
  has seen the message it produces, and "no space left on device" arriving in the middle of a card
  is a moment when the wording matters.

## Phase 12

Every acceptance criterion is tested against a mock, and **no test reaches Google** — the API is a
trait, and the one test of the real HTTP client points it at a socket on `127.0.0.1`. That proves the
client sends what it intends to send. It cannot prove Google wants it, and it cannot check anything
about how a photograph looks once it arrives.

- **Publish one photograph and confirm its capture date survives and it is filed under the correct
  day.** Specification §6.4, and the build plan names it as the thing to do *before the first bulk
  run*. The staged file is uploaded byte for byte — nothing here rewrites EXIF — so this is really a
  check on what Phases 9 and 10 preserved, arriving at the one place that matters.
- **Confirm the consent screen is published to "In production", not left in Testing.** A project in
  Testing issues refresh tokens that **expire after seven days**, and no client configuration avoids
  it. The reconnect path is built and tested regardless, but a connector that has to be
  re-authorised every week is a weekly interruption nobody wants. Unverified is fine for personal
  use; it shows a warning screen and caps at 100 users.
- **Confirm the exact upload headers against the live API.** `X-Goog-Upload-Protocol: raw`,
  `X-Goog-Upload-Content-Type` and `X-Goog-File-Name` are what the client sends, and the mock
  asserts it sends them. Whether Google requires exactly that set — and whether the file name is
  taken from the header or only from `simpleMediaItem.fileName` in the create call, which is also
  set — cannot be established without an account. The first single-photograph publish answers it.
- **Look at the `oauth` table and confirm the refresh token is unreadable.** `SELECT * FROM oauth`
  should show `v1:<hex>:<hex>` and nothing resembling a token. If
  `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` is unset, connecting refuses outright rather than storing it
  in the clear — so seeing a plausible-looking token there would mean something is badly wrong.
- **Confirm the redirect URI matches the OAuth client exactly.** `GOOGLE_OAUTH_REDIRECT_URI` has to
  be registered on the client character for character, including scheme and port. A mismatch fails
  at Google's end with an error the server never sees.
- **Watch a real `429`.** The thirty-second floor and the exponential growth above it are asserted by
  what the code *asks* to wait, not by wall-clock time — a test that slept for thirty seconds is a
  test that gets deleted. Whether a 500-photograph run provokes rate limiting at all, and whether
  Google's `Retry-After` is generous or absent, is unmeasured.
- **Decide whether photographs should carry a description.** `simpleMediaItem.fileName` is set to the
  stem, so a photograph is findable by the name the camera gave it. The `description` field is left
  empty, because inventing one is a choice about someone's library rather than a technical decision.

## Phase 14
- macOS `.dmg` bundle requires installation and launch testing on macOS.
- NAS deployment testing.
