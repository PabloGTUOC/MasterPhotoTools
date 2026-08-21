# Manual verification

Everything that cannot be settled by a test, gathered in one place and numbered so it can be worked
through, referred to and ticked off.

**Read [`testing.md`](testing.md) first** — it covers getting the thing running on a Mac, which is
the precondition for most of what is below.

## How to use this file

Each item has a stable id (`MV-8.3`), so it can be named in a commit, an issue, or a conversation
with an agent: *"do MV-8.3 and MV-8.4"*.

When you have done one:

1. Tick the box.
2. Replace the `**Result:**` line with what actually happened — a number, a yes, or a sentence about
   what went wrong. **A failed check is a result, not a reason to leave the box empty.**
3. If it found a defect, say so and open the work; the item stays ticked, because the check *was*
   performed.

Nothing here is optional-if-it-looks-fine. Each exists because a specific thing could not be
established from a machine with no camera, no Mac, no NAS and no Google account.

## What unblocks what

| You need | Unblocks | Items |
|---|---|---|
| A Mac | 7, 8, 10.3, 13, 14 | 21 |
| A Google OAuth client | 12 | 7 |
| A NAS and an SMB share | 11, 14 | 7 |
| A physical SD card | 8 | 7 |
| A Firebase project | 6.2 | 1 |
| Real photographs, and your eyes | 2, 4, 9, 10 | 15 |

**51 checks in all, and all of them are actionable.** Four are done — MV-2.1, MV-7.1, MV-7.2 and
MV-7.3 — and **three of those four found defects**, which is the argument for doing the rest. The
suggested order, and why, is in [`testing.md`](testing.md#5-suggested-order).

> **These checks are not the only way to find things.** Sixteen defects were found in two sessions
> simply by using the tabs with real photographs — a rename that renamed the folder, tools that
> reported success having done nothing, a metadata reader that returned nothing for files that
> plainly carried dates. Most had one cause: every tool had only ever been given typed file paths,
> and the folder pickers made pointing at a folder the ordinary gesture. **Point things at folders.**

---

## Phase 2 — Media layer

- [x] **MV-2.1 — Read metadata from real files from each camera body.**
      The fixture generator writes its own TIFF/IFD structures rather than using a camera file.
      `exiftool` and `nom-exif` both read them correctly, but maker-note-heavy RAW containers are a
      different proposition.
      **Run:** for a few real files, compare `read_meta` against `exiftool -s <file>`.
      **Pass:** capture date, camera and dimensions agree.
      **Result:** **Performed 2026-08-21 on Nikon D810 TIFFs, and it found a defect.** `read_meta`
      returned nothing at all — no date, no camera, 0×0 — for files `exiftool` reads
      `DateTimeOriginal 2025:10:03 12:51:27` from. `nom-exif`'s streaming reader opens the file and
      then fails part-way through its IFD (`Incomplete(Size(3809))`), and the failure was being
      turned into an empty result indistinguishable from a file with no metadata. Every TIFF on a
      card would have looked undated to F12.
      Fixed: a streaming failure is retried with the file in memory, which parses correctly. The
      same files now read capture `2025-10-03T12:51:27`, camera `NIKON D810`, 7360×4912, agreeing
      with `exiftool`.
      **Still to do for this check:** the other camera bodies, and a genuine RAW container rather
      than a TIFF — the maker-note case this check was written for is not yet covered.

- [ ] **MV-2.2 — Check the four date tags that cannot be read yet.**
      `nom-exif` 3.6 exposes `EXIF:DateTimeOriginal`, `EXIF:CreateDate` and a single collapsed
      QuickTime `CreateDate`. It does **not** separately surface `QuickTime:CreationDate`,
      `Keys:CreationDate`, `XMP:CreateDate` or `QuickTime:ModifyDate`. F1's preference order is
      implemented and unit-tested at all seven positions, but positions 3, 5, 6 and 7 cannot
      currently be populated from a file.
      **Run:** a real iPhone MOV (which carries `Keys:CreationDate`) and a file with an XMP packet,
      against `exiftool` output.
      **Pass:** decide whether the resolved date is still right in practice. If it is not, this
      becomes a real defect rather than a known limitation.
      **Result:**

- [ ] **MV-2.3 — Confirm lens, exposure and GPS survive a resize.**
      The round-trip test asserts capture date, camera and pixel dimensions. It says nothing about
      the rest.
      **Run:** `exiftool -s` on a real photograph before and after an F13 resize.
      **Pass:** lens, exposure and GPS are present and unchanged. (Whether Google Photos then files
      it under the capture date is **MV-12.1**.)
      **Result:**

## Phase 4 — Archive tools II

- [ ] **MV-4.1 — F4 (half-frame split) on real scans.**
      The synthetic fixture plants a hard-edged dark divider at a known column against flat colour
      panels. A real lab scan has a soft, uneven divider, film grain, dust, and a surround that is
      neither uniformly white nor uniformly black.
      **Run:** the split over real Pentax 17 scans.
      **Pass:** the divider is found rather than a dark part of the image; the lab border is fully
      removed without eating into the frame; the trimmed halves are not cropping away picture.
      **Result:**

- [ ] **MV-4.2 — F7 (print border) on real scans.**
      The dark-edge trim thresholds — luma 28, 70% of a band, 40 px maximum — are the
      specification's numbers but have only met synthetic edges.
      **Pass:** a genuinely dark photograph is not mistaken for a scan border, and the
      2%-of-short-side corner radius looks right at print size.
      **Result:**

- [ ] **MV-4.3 — Compare mozjpeg output against the old encoder.**
      F4 and F7 now write 4:4:4 and F8 writes progressive 4:2:0, verified from the files. mozjpeg's
      quantisation tables differ from the previous encoder's, so the same nominal quality number
      produces a different-looking file.
      **Pass:** a print-size F7 output and an F8 scan conversion are an improvement, not just a
      difference.
      **Result:**

- [ ] **MV-4.4 — F5 caption legibility.**
      Captions use a built-in 5×7 bitmap font, not a real typeface.
      **Pass:** legible at the intended cell sizes and print scale.
      **Result:**

## Phase 6 — Web front end

- [ ] **MV-6.1 — Drive the web UI one-handed on a real phone.**
      `check:layout` asserts that no route scrolls sideways and every control is at least 40 px
      tall, and writes screenshots to `frontend/web/layout-proof/`. That is geometry, not feel.
      **Run:** `npm --prefix frontend/web run check:layout` for the geometry, then a real phone for
      the rest.
      **Pass:** reachable thumb targets, sensible keyboard behaviour on iOS and Android, readable
      contrast in sunlight.
      **Result:**

- [ ] **MV-6.2 — Firebase sign-in end to end.** *(needs a Firebase project)*
      **Run:** set the four `VITE_FIREBASE_*` variables and rebuild.
      **Pass:** Google sign-in completes; the ID token reaches the server; an expired token triggers
      a silent refresh and retry rather than a login screen (§5.3).
      **Result:**

## Phase 7 — Desktop shell · needs a Mac

- [x] **MV-7.1 — The app builds and launches on macOS.** *(do this one first — it gates the rest)*
      **Run:** `cargo tauri dev` from `crates/desktop`, or
      `npm --prefix frontend/desktop run build` then `cargo tauri build`.
      **Pass:** a window opens with the sidebar and the tool routes.
      **Result:** **Passes, after three defects that each stopped it.** `beforeDevCommand` resolved
      its path from the parent of `crates/desktop`, so the front end was never built;
      `icons/icon.icns` and `icon.ico` are 0-byte placeholders and referencing the empty `.icns`
      aborted the process at launch (`NSImage::initWithData(..).expect("creating icon")`); and both
      `vite.config.ts` files set `server.fs.allow` without the package root, so the dev server
      refused its own `index.html`. All fixed. `cargo tauri dev` has since opened the window
      reliably, dozens of times.
      **Not covered:** `cargo tauri build` — the bundle path is MV-14.1 and is still blocked on a
      real icon.

- [x] **MV-7.2 — An F1 date scan through the running app.**
      The scan itself is tested headlessly; the `invoke` round trip and the rendering are not.
      **Pass:** the table appears with plausible dates.
      **Result:** **The table did not exist.** The desktop's `scan_dates` command returned the rows
      and its TypeScript wrapper discarded them, returning an empty job id; the server counted them
      into a summary and dropped them; and `Dates.vue` had nowhere to put them. "Scan only" was
      therefore a no-op with no output at all.
      `scanDates` now returns `ScanResult[]` on both transports and the view renders file, metadata
      date with the tag that supplied it, filesystem date with whether it is a birth or modification
      time, and a per-file state carrying a mark as well as a colour. Confirmed against 39 Nikon
      TIFFs, which read plausible dates once MV-2.1's reader defect was fixed.

- [x] **MV-7.3 — The app survives the server being off.**
      **Run:** stop `phototools-server`, then launch the app.
      **Pass:** it starts, shows "Server offline" with a reason, and the local tools still run.
      **Result:** **Passes.** The app runs with the server stopped and every local tool works —
      most of this session's testing was done that way.
      Two things were fixed along the way. The reason given was useless: any unreachable answer read
      as "Server offline", including a 404 from an entirely different service on port 3000, which
      sent somebody to restart a server that was already running. It now distinguishes "nothing
      answered at <url>" from "something is listening but is not PhotoTools". And the server address
      was held only in memory, so every launch returned to the default — it now persists, with the
      token in the Keychain rather than beside it.

- [ ] **MV-7.4 — The refresh token round-trips through the macOS Keychain.**
      `keyring` has no usable backend in a headless Linux container, so `credentials.rs` is only
      tested for the shape of its failure.
      **Pass:** sign in, quit, relaunch, and the session survives — and the token appears in Keychain
      Access under `com.phototools.master`, **not** in a file.
      **Result:**

- [ ] **MV-7.5 — Job progress arrives as Tauri events (F17).**
      **Run:** start a long rename and watch the bar; quit mid-job and relaunch.
      **Pass:** the bar moves, and the interrupted job is *reported* interrupted rather than
      vanishing.
      **Result:**

## Phase 8 — Card detection · needs a Mac and a real card

- [ ] **MV-8.1 — A real card raises the notification.**
      The text and the count are tested; that macOS raises the notification at all is not, and
      `tauri-plugin-notification` needs the user to have granted permission to the app.
      **Pass:** reads `EOS_DIGITAL — 412 new shots. Review?` with the card's actual label and a
      plausible count.
      **Result:**

- [ ] **MV-8.2 — The debounce is long enough for your reader.**
      Set to 1.5 s, chosen from the shape of the problem rather than from measurement: a mount is
      several events, and looking too early finds an empty directory and concludes there is no
      `DCIM`. **If your reader is slower, detection silently misses cards.**
      **Pass:** no card mounts without raising something. If one does, raise `DEBOUNCE` in
      `crates/desktop/src/detection.rs`.
      **Result:**

- [ ] **MV-8.3 — `/Volumes` is watchable.**
      `notify` uses FSEvents on macOS; the watcher is exercised against a stand-in directory in
      tests, and `/Volumes` specifically is not.
      **Result:**

- [ ] **MV-8.4 — A non-card volume raises nothing.**
      **Run:** mount a backup drive or a USB stick with no `DCIM`.
      **Pass:** silence.
      **Result:**

- [ ] **MV-8.5 — A reinserted card with nothing new raises nothing.**
      Tested against simulated cards, but the volume label it keys on comes from the real mount
      point.
      **Pass:** silence on reinsertion; after shooting more frames, it announces only the new ones.
      **Result:**

- [ ] **MV-8.6 — The card is byte-identical after a scan (G5).**
      The test proves this against a read-only fixture; a real card is the case that matters.
      **Run:** `find /Volumes/CARD -type f -exec shasum {} + | sort > /tmp/before.txt`, scan, then
      the same into `after.txt`, then `diff`.
      **Pass:** no difference.
      **Result:**

- [ ] **MV-8.7 — Time a real 400-shot card scan.**
      The measured 44 ms is on 64×48 fixtures and is not the real figure. §9.1 budgets 10 s; a real
      card is expected to take several seconds and **may exceed it on a slow reader**.
      **Pass:** record the number, whatever it is.
      **Result:**

## Phase 9 — Validation and remediation

- [ ] **MV-9.1 — Look at a resized frame next to its original.**
      The tests prove the capture date survives and the dimensions land under the ceiling; they say
      nothing about whether a 24 MP frame reduced to 10 MP still looks right.
      **Pass:** check a few with fine detail — foliage, fabric, text.
      **Result:**

- [ ] **MV-9.2 — Confirm the 30-day batch spread suits how you shoot.**
      F12 gives 30 days for the camera-clock check, and `BATCH_SPREAD_DAYS` reuses it to decide when
      a frame is "far from the batch median", which the specification gives no figure for. **If you
      routinely leave a card in the camera for six weeks, frames from the start will be flagged.**
      **Result:**

- [ ] **MV-9.3 — Read the clock-offset suggestion before applying it in anger.**
      The shift comes from the median, so **a card holding two shoots with different clock errors
      gets one correction that suits neither.** The dry run shows the resulting date per frame.
      **Result:**

- [ ] **MV-9.4 — Judge the quality ladder's bottom rung.**
      A frame that cannot meet the byte cap at quality 75 is written anyway and reported in
      `still_too_large`. Whether 75 is low enough to publish, or whether such a frame should be
      resized further instead, is a taste question.
      **Result:**

## Phase 10 — RAW to JPEG

Rung 1 — the embedded preview — is fully tested and will handle nearly every real file. What cannot
be tested is how the results *look*, and the two rungs below it.

- [ ] **MV-10.1 — Judge colour quality on real RAW files from each camera body.**
      The build plan names this one. The tests prove a JPEG comes out with the right dimensions and
      the right date; whether the colour and tone are right is a judgement no assertion can make.
      **Pass:** compare a derived JPEG against the camera's own JPEG for the same frame.
      **Result:**

- [ ] **MV-10.2 — Confirm the embedded preview is full-resolution on your bodies.** *(do this before
      trusting any Phase 10 output)*
      The extractor takes the largest embedded JPEG, which on most cameras is the full-size render —
      but **some bodies write only a screen-sized preview, and a 1616×1080 derivative from a 45 MP
      frame would pass every test here while being useless.**
      **Run:** compare the derived file's dimensions against the RAW's own.
      **Result:**

- [ ] **MV-10.3 — Confirm rung 2 runs on macOS.** *(needs a Mac)*
      `sips` is invoked only on macOS and cannot be exercised on Linux at all.
      **Run:** derive from a RAW with no embedded preview.
      **Pass:** the reported rung is `macos_imageio`, not `rawler`.
      **Result:**

- [ ] **MV-10.4 — Judge rung 3's output when it is reached.**
      `rawler`'s development pipeline is a generic demosaic with no camera-specific tuning, so its
      colour will not match the camera's render or Apple's. It is the **only** rung available on the
      Linux server, so the question is whether server-side derivation is worth having at all or
      should be refused in favour of doing it on the Mac.
      **Result:**

- [ ] **MV-10.5 — Confirm CR3 files are declined rather than mangled.**
      F14 puts CR3 out of scope; a CR3 on a card is reported as underivable.
      **Pass:** a clear message, not a crash.
      **Result:**

## Phase 11 — Handoff and ledger · needs a NAS

Every test in this phase writes to a local temporary directory. The real staging directory is an SMB
share, which is a different filesystem with different guarantees — three of these five are about
exactly that.

- [ ] **MV-11.1 — Set `ADMIN_TOKEN` and confirm the desktop can authenticate.** *(do this first —
      nothing else in Phase 11 works without it)*
      The desktop has a Keychain but no Firebase sign-in yet, so the handoff authenticates with
      §5.3's break-glass token, set in `ServerSettings.auth_token`. **Without it every request is a
      401.** Configuration, not code.
      **Result:**

- [ ] **MV-11.2 — Confirm `rename` is atomic enough on your SMB share.**
      The copy writes `<hash>.jpg.partial` and renames it into place, so the server never sees a
      half-written file under its final name. On a local POSIX filesystem that rename is atomic;
      over SMB it is a server-side operation whose atomicity depends on the NAS. If it is not
      atomic the failure is benign — verification fails and the file is recopied — but it happens on
      *every* file, turning one pass into two.
      **Pass:** the `recopied` count on a real card is **zero**.
      **Result:**

- [ ] **MV-11.3 — Confirm the server can read a file the Mac has just finished writing.**
      SMB clients cache writes, so a file can look complete on the Mac before all of it has reached
      the NAS. The recopy path covers this correctly, but if it fires on most files rather than
      none, the handoff is doing double the work and the cause is write caching, not corruption.
      **Pass:** as MV-11.2 — a near-zero recopy count distinguishes the two.
      **Result:**

- [ ] **MV-11.4 — Time a real card, and check the two timeouts against it.**
      Verification hashes every arrival and is allowed 30 minutes (`VERIFY_TIMEOUT`); each HTTP call
      is allowed 60 seconds (`HANDOFF_TIMEOUT`). Neither has been measured against a real card over
      a real network. A 400-frame card is 1–3 GB, hashed against the NAS's own disks.
      **Result:**

- [ ] **MV-11.5 — Fill the NAS and watch what happens.**
      A copy that runs out of space fails the handoff with the underlying I/O error rather than
      reporting a hash mismatch, which is the right answer — but nobody has seen the message it
      produces, and "no space left on device" arriving mid-card is a moment when the wording
      matters.
      **Result:**

## Phase 12 — Google Photos · needs an OAuth client

No test reaches Google: the API is a trait, and the one test of the real HTTP client points at
`127.0.0.1`. That proves the client sends what it intends to. It cannot prove Google wants it.

- [ ] **MV-12.1 — Publish ONE photograph and confirm its capture date survives and it is filed under
      the correct day.** *(specification §6.4 — do this **before** any bulk run)*
      The staged file is uploaded byte for byte — nothing in Phase 12 rewrites EXIF — so this is
      really a check on what Phases 9 and 10 preserved, arriving at the one place that matters.
      **Result:**

- [ ] **MV-12.2 — Confirm the consent screen is "In production", not Testing.**
      **A project in Testing issues refresh tokens that expire after seven days**, and no client
      configuration avoids it. The reconnect path is built and tested regardless, but weekly
      re-authorisation is a weekly interruption. Unverified is fine for personal use: it shows a
      warning screen and caps at 100 users.
      **Result:**

- [ ] **MV-12.3 — Confirm the redirect URI matches the OAuth client exactly.**
      `GOOGLE_OAUTH_REDIRECT_URI` must be registered on the client character for character,
      including scheme and port. **A mismatch fails at Google's end with an error the server never
      sees.**
      **Result:**

- [ ] **MV-12.4 — Confirm the exact upload headers against the live API.**
      `X-Goog-Upload-Protocol: raw`, `X-Goog-Upload-Content-Type` and `X-Goog-File-Name` are what
      the client sends, and the mock asserts it does. Whether Google *requires* that set — and
      whether the file name is taken from the header or only from `simpleMediaItem.fileName` in the
      create call, which is also set — cannot be established without an account. MV-12.1 answers it.
      **Result:**

- [ ] **MV-12.5 — Look at the `oauth` table and confirm the refresh token is unreadable.**
      **Run:** `sqlite3 "$DATABASE_PATH" "SELECT * FROM oauth;"`
      **Pass:** `v1:<hex>:<hex>` and nothing resembling a token. If
      `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` is unset, connecting refuses outright rather than
      storing it in the clear — so a plausible-looking token there means something is badly wrong.
      **Result:**

- [ ] **MV-12.6 — Watch a real `429`.**
      The thirty-second floor and the growth above it are asserted by what the code *asks* to wait,
      not by wall-clock time. Whether a 500-photograph run provokes rate limiting at all, and
      whether Google's `Retry-After` is generous or absent, is unmeasured.
      **Result:**

- [ ] **MV-12.7 — Decide whether photographs should carry a description.**
      `simpleMediaItem.fileName` is set to the stem, so a photograph is findable by the name the
      camera gave it. `description` is left empty, because inventing one is a choice about someone's
      library rather than a technical decision.
      **Result:**

## Phase 13 — Ingest UI · needs a Mac

Every acceptance criterion is measured in a real browser — first paint, filtering, scrolling, the
bulk press and the publish gate. What cannot be checked here is the screen those pieces are
assembled into.

- [ ] **MV-13.1 — Look at the Ingest screen on the Mac.**
      The one part of this phase never rendered. `ShotGrid` and `BulkActions` are measured on their
      own against four hundred shots, and the web build's `/publish` route is driven for real — but
      `Ingest.vue` composes them with the Tauri transport, and Tauri needs macOS.
      **Pass:** the flow (look → scan → review → decide → derive → hand over) makes sense with a card
      in the reader.
      **Result:**

- [ ] **MV-13.2 — Re-take the 400-shot measurement in WKWebView.**
      The numbers were taken in Chromium on Linux at 1280×900: first paint 599 ms, 29 rows built of
      400, filter 8.6 ms, scroll 15.2 ms per frame. Tauri renders in WKWebView, a different engine,
      and **the grid's windowing is the part most likely to behave differently.**
      **Result:**

- [ ] **MV-13.3 — Time a real bulk resize.**
      One press requests a resize of every oversized shot — 360 of 400 in the fixture. Whether that
      is thirty seconds or five minutes on real 24 MP files, and whether the progress bar makes the
      wait tolerable, is unmeasured.
      **Result:**

- [ ] **MV-13.4 — Decide whether the session hand-off between the two screens is good enough.**
      The desktop reports the session id in its handover panel; a person copies it into the web
      publish screen. §8 lists no endpoint that enumerates sessions, so this is the seam as
      specified — but it is a UUID typed by hand. Only somebody using it can say whether that is
      acceptable. See [`known-gaps.md`](known-gaps.md#no-session-discovery).
      **Result:**

- [ ] **MV-13.5 — Check the chips at arm's length.**
      Each carries a mark as well as a colour — ✓, ✕ or ! — because a review screen whose whole
      meaning is "which of these failed" cannot put that meaning in a hue alone.
      **Pass:** the marks are legible at chip size on a real display.
      **Result:**

- [ ] **MV-13.6 — Confirm the grid height suits a real window.**
      The viewport is capped at 60% of the window height, which is a guess about a desktop window
      rather than a measurement.
      **Result:**

## Phase 14 — Packaging

- [ ] **MV-14.1 — The `.dmg` installs and launches on macOS.** *(blocked: needs a real icon)*
      **The application icons are 0-byte placeholders**
      ([`known-gaps.md`](known-gaps.md#the-application-icons-are-placeholders)). Supply a square
      1024×1024 PNG and run `cargo tauri icon <file>` first, or the bundle carries a blank icon.
      **The build is unsigned**, which §9.3 accepts for personal use, so macOS will refuse it on
      first launch. That refusal is expected; what is being checked is that the application works
      once past it.
      **Run:** `cd crates/desktop && cargo tauri build`, then open the `.dmg` from
      `target/release/bundle/dmg/`, drag to Applications, and **right-click → Open**.
      **Pass:** the window opens with the sidebar and the tool routes, as MV-7.1. If it opens from
      `cargo tauri dev` but not from the bundle, the difference is the bundle, not the code.
      **Result:**

- [ ] **MV-14.2 — The container deploys to the NAS and passes its health check.**
      Built, started and driven on an Apple Silicon Mac: the image is 295 MB, the container reports
      `healthy` within 10 s, `/api/health` answers 200, `/` serves the web UI and `/api/storage/ls`
      answers 401. **None of that is the NAS.** What is unverified is its architecture, its Docker
      version, whether a `/volume1/...` bind mount arrives writable by uid 10001, and whether the
      library on its own filesystem behaves like a local mount.
      **Run:** follow [`deployment.md`](deployment.md) §2 on the NAS itself.
      **Pass:** `docker compose ps` shows `healthy`, and `curl http://nas.local:3000/api/health`
      answers from another machine. If it is `unhealthy`, read
      [`deployment.md`](deployment.md#9-when-it-will-not-start) — a `/data` uid 10001 cannot write
      to is the likeliest cause.
      **Result:**

- [ ] **MV-14.3 — Confirm the multi-architecture image runs on the NAS's own architecture.**
      `linux/arm64` was built natively and verified running. `linux/amd64` was built under
      emulation, which proves it compiles and links, **not** that it runs — no emulated container
      was started.
      **Run:** on an amd64 NAS, pull or build the image and start it.
      **Pass:** as MV-14.2.
      **Result:**
