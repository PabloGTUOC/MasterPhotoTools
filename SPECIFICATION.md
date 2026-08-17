# PhotoTools — System Specification

**Version:** 1.0
**Status:** specification. No implementation yet.

PhotoTools ingests photographs from camera SD cards, validates and prepares
them, publishes them to Google Photos, and provides a set of archive
manipulation tools over a NAS photo library.

This document defines what the system does and how it is structured. It is the
sole reference needed to begin implementation.

---

## 1. Overview

### 1.1 Purpose

Two distinct jobs, done by one system:

1. **Ingest.** A camera SD card is inserted into a Mac. The system detects it,
   reads the photographs, decides which file represents each shot, checks that
   the metadata and dimensions are sane, fixes what is not, and publishes the
   results to Google Photos.
2. **Archive maintenance.** A photo library lives on a NAS. The system provides
   tools to repair dates, rename in bulk, split scanned half-frame film, build
   contact sheets, convert formats, and add print borders — usable from a phone
   while away from the desk.

### 1.2 Users

A single photographer, optionally sharing access with a small number of trusted
people. The system is not multi-tenant: all users see the same library.

### 1.3 Guiding principles

| Principle | Meaning |
|---|---|
| **Write logic once** | Anything that could run in more than one place lives in the shared core library and is implemented a single time. |
| **Move the work, not the data** | Processing happens wherever the bytes already are. Large files should never cross the network to be read. |
| **Rust wherever possible** | Performance-critical work is Rust. Exactly one external binary is permitted (§2.6). |
| **Never damage an original** | Source files are read-only inputs. Every transformation writes something new. |
| **Nothing destructive without a preview** | Every operation that writes or publishes has a dry run, and publishing requires one. |

---

## 2. Architecture

### 2.1 Components

| Component | Kind | Runs on | Responsibility |
|---|---|---|---|
| `phototools-core` | Rust **library** | — | All functionality. No web framework, no UI, no platform assumptions. |
| `phototools-server` | Rust binary (axum) | NAS, in Docker | Backend for the web front end. Owns the upload ledger and the Google Photos credentials. Performs archive work against the library. |
| `phototools-desktop` | Rust binary (Tauri v2) | macOS | Detects and processes SD cards locally. |
| **Web UI** | Vue 3 | Served by `phototools-server` | Archive tools from a phone or any browser. |
| **Desktop UI** | Vue 3 | Inside the Tauri app | Card review and ingest. |

### 2.2 The core is a library, not a service

`phototools-core` is compiled into **both** binaries. This is the single most
important structural decision in the system.

- Inside `phototools-server`, it backs the web front end and performs archive
  operations on files that live on the NAS.
- Inside `phototools-desktop`, it runs **in-process**, so an SD card is read and
  processed on the machine the card is plugged into.

**Why this matters.** A typical 400-frame card holds roughly **17 GB** (a 24 MP
JPEG is ~12 MB, its RAW companion ~30 MB). If the desktop application had to
call a remote service to process a card, every one of those bytes would have to
cross the network first. Processing locally and transferring only the finished
derivatives — **1–3 GB** — is six to fourteen times less traffic, for byte-identical
results, because the same code produced them.

The corollary is a hard rule:

> **No functionality may be implemented in a binary crate.** If `server` and
> `desktop` both need it, it belongs in `core`. If only one needs it today but
> the other plausibly might, it still belongs in `core`.

Binary crates contain only: transport (HTTP handlers, Tauri commands),
platform integration (volume watching, keychain access), and process lifecycle.

### 2.3 Topology

```
   SD card  (~17 GB)
      │
      │  read locally — never crosses the network
      ▼
┌──────────────────────────────┐
│  phototools-desktop   macOS  │
│  Tauri v2 · core             │
│  detect → scan → validate    │
│  → convert → resize          │
└──────────────┬───────────────┘
               │  derivatives + manifest  (~1–3 GB)
               │  written to a staging directory on the NAS share
               ▼
┌────────────────────────────────────┐          ┌──────────────────┐
│  phototools-server    NAS, Docker  │─────────▶│  Google Photos   │
│  axum · core                       │  publish │  append-only     │
│  ledger · jobs · archive tools     │          └──────────────────┘
└──────────────┬─────────────────────┘
               │  HTTPS, authenticated
               ▼
┌──────────────────────────────┐
│  Web UI     phone / browser  │
│  archive tools · monitoring  │
└──────────────────────────────┘
```

**Why the server publishes rather than the desktop.** The NAS is always powered
on. Once derivatives reach it, publishing continues if the laptop is closed,
retries happen unattended through rate limits and network interruptions, the
Google refresh token lives on exactly one machine, and a phone can monitor or
restart a run.

**Why the handoff is a staging directory rather than an upload API.** The Mac
already mounts the NAS share over SMB. Writing into a watched directory requires
no upload protocol, no chunking and no resume logic. An interrupted copy leaves a
file whose checksum fails to match the manifest; it is simply recopied.

### 2.4 Crate graph

```
                 ┌──────────────────┐
                 │ phototools-core  │
                 └────────┬─────────┘
                  ┌───────┴────────┐
                  ▼                ▼
        ┌──────────────────┐  ┌───────────────────┐
        │ phototools-server│  │ phototools-desktop│
        └──────────────────┘  └───────────────────┘
```

Dependencies point one way only. `core` never depends on either binary, never
references axum or Tauri, and must compile and pass its tests with no binary
crate present.

### 2.5 Core module layout

```
core/
├── media/        image decode, encode, resize; EXIF read; RAW handling
├── tools/        the archive operations (F1–F9)
├── ingest/       card scanning, pairing, validation, remediation (F10–F14, F16)
├── publish/      Google Photos client (F15)
├── ledger/       SQLite persistence
├── jobs/         long-running work and progress reporting (F17)
└── config/       settings, roots, thresholds
```

`media` is the only module permitted to touch image bytes. Everything above it
works in terms of its types.

### 2.6 Technology

| Concern | Choice |
|---|---|
| Async runtime | `tokio` |
| Data parallelism | `rayon` |
| HTTP server | `axum` |
| Desktop shell | `tauri` v2 |
| Persistence | `rusqlite` |
| Image decode/encode | `image`, `zune-jpeg` |
| Resize | `fast_image_resize` (SIMD) |
| RAW decode | `rawler`; macOS ImageIO where available |
| EXIF read | `nom-exif` |
| HTTP client | `reqwest` |
| Token verification | `jsonwebtoken` |
| Filesystem watching | `notify` |
| Credential storage | `keyring` (macOS Keychain) |
| Front end | Vue 3 + Vite |

**The one permitted external binary: `exiftool`.** Nothing in the Rust ecosystem
writes date metadata correctly across the full range of JPEG, TIFF, HEIC,
QuickTime and RAW containers. It is therefore required for metadata **writing**
only.

It must be driven in persistent mode — `exiftool -stay_open True -@ -` — with a
single long-lived process fed commands over a pipe. **Spawning one process per
file is prohibited:** startup cost is 150–250 ms regardless of file size, which
would add well over a minute of pure process overhead to a 500-file operation.

Metadata **reading** must never invoke `exiftool`. It is done in-process
(§9.1).

### 2.7 Repository layout

```
phototools/
├── SPECIFICATION.md
├── README.md
├── Cargo.toml                  workspace manifest
├── crates/
│   ├── core/
│   ├── server/
│   └── desktop/
├── frontend/
│   ├── shared/                 components + API client used by both UIs
│   ├── web/
│   └── desktop/
└── deploy/
    ├── Dockerfile
    └── docker-compose.yml
```

The two front ends are separate builds over a shared component and API-client
package. The shared client exposes one interface; the desktop build fulfils it
with Tauri `invoke`, the web build with HTTP. Views are written once.

---

## 3. Functional requirements — archive tools

These operate on a photo library on the NAS. They are reachable from the web UI
and, where useful, from the desktop UI.

All of them accept a dry-run flag and report exactly what would change.

---

### F1 — Date scan and repair

Photographs frequently carry wrong or missing capture dates: a camera clock reset
by a flat battery, a scanner that stamps the scan date, an export that dropped
metadata.

**Scan.** Walk a folder, optionally recursively, and report per file: name, path,
best available metadata date, which tag supplied it, the filesystem date, and a
status of `OK`, `Mismatch`, or `Missing Metadata`.

**Tag preference order**, first hit wins:

1. `EXIF:DateTimeOriginal`
2. `EXIF:CreateDate`
3. `QuickTime:CreationDate`
4. `QuickTime:CreateDate`
5. `Keys:CreationDate`
6. `XMP:CreateDate`
7. `QuickTime:ModifyDate`

All values normalise to `YYYY:MM:DD HH:MM:SS`. The sentinel
`0000:00:00 00:00:00` counts as absent. Timezone suffixes are dropped;
`YYYY-MM-DD` input is accepted and converted. QuickTime timestamps are read as
UTC to prevent double-shifting.

**Media types handled**

- Images: `.jpg .jpeg .png .gif .tif .tiff .heic .heif`
- RAW: `.dng .nef .arw .cr2 .raf`
- Video: `.mov .mp4 .m4v .mts .m2ts .3gp .avi`

**Repair modes**

| Mode | Behaviour |
|---|---|
| `auto` | Take the best available metadata date; write it to metadata and filesystem timestamps |
| `manual` | Force a supplied `YYYY:MM:DD HH:MM:SS` |
| `shift` | Offset all dates by a delta, e.g. `+1:0:0 0:0:0` — for a camera clock that was wrong by a known amount |
| `sidecar` | Take the date from a Google Takeout JSON sidecar (F2) |

Images receive `DateTimeOriginal`, `CreateDate`, `ModifyDate` and `AllDates`.
Video receives `CreateDate`, `ModifyDate`, `MediaCreateDate` and
`TrackCreateDate`. Both additionally receive `FileCreateDate` and
`FileModifyDate`.

**Filesystem timestamps are platform-dependent.** macOS and BSD expose a
creation ("birth") time that can be set; Linux does not. The implementation must
branch on platform, compare against modification time where no birth time
exists, and **never report an outcome it has not verified**.

---

### F2 — Google Takeout sidecar dates

Google Takeout exports carry a `.json` sidecar per media file containing
`photoTakenTime.timestamp` as Unix seconds. Locate the sidecar, read the
timestamp, apply it through F1's write path.

Sidecar filename matching must tolerate Takeout's two known quirks: filename
truncation, and `(1)`-style duplicate suffixes appearing on either the media file
or the sidecar.

Supports single-file and recursive folder operation.

---

### F3 — Batch rename

Rename a set of files to a consistent, sortable scheme.

**Prefix** is assembled from up to four blocks joined by `-`, omitting empty
ones:

```
<date>-<subject>-<camera>-<film>

2024-05-01-Lisboa-PENTAX17-PORTRA400
```

- **Date** accepts `YYYYMM`, `YYYYMMDD` or `YYYY-MM-DD`. Sanitising keeps only
  digits and `-`; the result must be at least 6 characters.
- **Other blocks**: spaces removed, `_` converted to `-`, then any character
  outside `[A-Za-z0-9-]` stripped.

**Ordering** is one of:

- `capture` — by best metadata datetime, falling back to file modification time,
  then filename.
- `numeric` — by the first integer found in the filename, then filename.

Files are numbered from 1, zero-padded to `max(2, digits(count))`, preserving the
original extension in lowercase.

**Two-phase.** The operation produces a **plan** of `(source, new name)` pairs
for review. Applying is a separate call. Duplicate targets within a batch, and
collisions with files already on disk, are skipped and reported — **never
overwritten**.

---

### F4 — Half-frame film split

A half-frame camera exposes two images per 35 mm frame, so a scan contains two
photographs side by side. This operation separates them.

The reference format is the Pentax 17: a 17 × 24 mm frame, always portrait,
aspect ratio 24/17 ≈ 1.41. The ratio is configurable for other cameras.

**Procedure**

1. **Remove the lab border.** Scanning services add a wide white or black
   surround. Scan inward from each edge; a row or column is border if more than
   `border_tol` of its pixels are ≥ `threshold_white` or ≤ `threshold_dark`.
   Never remove more than `max_crop_pct` from any one side.
2. **Locate the divider.** Compute the column-mean brightness profile, ignore
   `margin` at each end, take the darkest column, then refine within `±window`.
3. **Split** at that column.
4. **Trim residual dark bands** from all four sides of each half — but never past
   the point where the result would violate the frame ratio. If a half comes out
   landscape, rotate it 90° first. If the result remains more than 10% taller
   than the target ratio, remove the excess from the bottom only.
5. **Write** `{base}_A.jpg` and `{base}_B.jpg` at JPEG quality 95 with no chroma
   subsampling.

**Defaults**

| Parameter | Value | Meaning |
|---|---|---|
| `threshold_dark` | 25 | Pixel value at or below which a pixel counts as black |
| `threshold_white` | 235 | Pixel value at or above which a pixel counts as white |
| `border_tol` | 0.92 | Fraction of extreme pixels needed to call a line "border" |
| `max_crop_pct` | 0.12 | Maximum proportion removable from one side |
| `margin` | 0.20 | Proportion of width ignored at each end when seeking the divider |
| `window` | 20 | Refinement radius around the darkest column, in pixels |
| `ratio` | 24/17 | Target height ÷ width |

A **preview** mode returns the border-cropped whole image plus both halves
without writing anything.

---

### F5 — Contact sheet

Build a single JPEG containing a grid of thumbnails from a folder — a proof
sheet for reviewing a shoot or a roll.

- Configurable `cols` (4), `cell_size` (300 px), `spacing` (20 px), `margin`
  (40 px).
- Optional filename caption beneath each cell, using a 30 px label strip and a
  font size of `max(10, cell_size × 0.04)`. Names longer than 28 characters are
  shortened to `name[:18] + "..." + extension`.
- Background black or white; caption colour inverts to match.
- Sort by filename or by modification date.
- Thumbnails preserve aspect ratio, are centred within their cell, and honour
  EXIF orientation.
- **A file that cannot be read gets a red crossed box in its cell.** One bad file
  must never abort the sheet.
- Output at JPEG quality 95, optimised.

**Dimensions**

```
width  = cols × cell_size + (cols − 1) × spacing + 2 × margin
height = rows × (cell_size + label_height) + (rows − 1) × spacing + 2 × margin
```

---

### F6 — Transform

General-purpose conversion over a single file or a whole directory. EXIF
orientation is applied first, then in order:

- **Rotate** by a given angle, expanding the canvas to fit.
- **Resize** so the long edge is at most a given value. Downscale only — never
  enlarge.
- **Convert** to a target format, converting to RGB when the target is JPEG.
- **Quality** (default 95) and **optimise** (default on) for JPEG and WebP.

Accepts `.jpg .jpeg .png .tif .tiff .heic .heif`.

---

### F7 — Print border

Place an image on a fixed white canvas with rounded corners, sized for printing
and for social platforms that crop unpredictably.

1. **Optionally trim dark scan edges.** A side is trimmed while more than 70% of
   a sampled band falls below luma 28, up to a maximum of 40 px, plus a 1 px
   safety inset.
2. **Choose the canvas.** Long side 3000 px. Portrait input yields 4:5
   (3000 × 3750). Landscape input yields 5:4 (3000 × 2400).
3. **Fit the image** inside a minimum 50 px margin, enlarging smaller images to
   fill the space.
4. **Round the corners** with a radius of 2% of the image's short side,
   anti-aliased by rendering the mask at 4× and downsampling.
5. **Centre on white** and save at quality 95 with no chroma subsampling.

---

### F8 — TIFF to JPEG

Convert scanner output to a distributable format.

Multi-page TIFFs produce `{base}_p001.jpg`, `{base}_p002.jpg` and so on; a
single-page TIFF produces `{base}.jpg`. Alpha channels are flattened onto white.
The long edge is capped at 2048 px. Output is JPEG quality 90, 4:2:0 chroma
subsampling, progressive, optimised.

---

### F9 — Library browser

List a directory: name, absolute path, whether it is a directory, and size for
files. Directories sort first, then alphabetically, case-insensitively. A parent
(`..`) entry is included except at the root. Entries that cannot be read are
skipped rather than failing the listing.

**Browsing is confined to configured roots** (§9.2). A path that resolves outside
an allowed root is rejected.

---

## 4. Functional requirements — ingest

Ingest runs on the desktop, except for publishing and the ledger, which are
server responsibilities.

---

### F10 — Card detection

Watch `/Volumes` for newly mounted filesystems. Debounce, then test for a `DCIM`
directory. On a match, raise a native notification:

> **EOS_DIGITAL** — 412 new shots. Review?

A card is identified by its volume label plus a fingerprint computed over the
sorted `(relative path, size, modification time)` tuples of its contents, so a
reinserted card is recognised as one already seen.

Detection is a platform integration concern and therefore lives in the desktop
binary; everything it triggers lives in `core`.

---

### F11 — Card scan and shot pairing

Walk the card's `DCIM` tree in parallel. For each file, read EXIF **in-process**
and record path, size, pixel dimensions, camera model, capture datetime and a
content hash.

**Group files into shots by filename stem.** A camera shooting RAW+JPEG writes
`IMG_1234.JPG` and `IMG_1234.CR2` for one photograph; these are one shot with two
assets.

Per shot, choose the **candidate** — the asset that will be published:

- JPEG present → the JPEG is the candidate. The RAW is recorded but not
  published.
- RAW only → the candidate is produced by F14.

Pixel dimensions come from EXIF metadata, never by decoding the image. Decoding
400 frames to learn their sizes would turn a two-second scan into a two-minute
one.

> **Invariant: the card is never written to.** Candidates are copied to a staging
> directory, verified by hash, and every subsequent operation acts on the copy.

---

### F12 — Validation

Each candidate is checked against three independent rules.

**Date**

- No capture date → `FAIL(no_date)`.
- `|capture − now| > max_age_days` (default **90**) → `FAIL(date_out_of_range)`.
- Within range but far from the batch median → `WARN`, not `FAIL`. Frames left
  over from an earlier shoot on the same card are legitimate.

**Camera clock check.** Compute the median capture date across the whole card. If
the median is out of range but the spread is tight (under 30 days), the camera
clock is offset rather than the photographs being old. Surface a **single bulk
correction of `now − median`** instead of one prompt per file. This reuses F1's
`shift` mode.

**Resolution** — `width × height ≤ max_megapixels × 10⁶`, default **10 MP**.

**Size** — the published file must be `≤ max_output_bytes`, default **10 MB**.

`max_megapixels` and `max_output_bytes` are independent settings, and **both
apply to both the JPEG path and the RAW-derived path.**

> **A consequence to design around.** A 10 MP ceiling means a modern 24–45 MP
> camera fails the resolution check on virtually every frame. **Resizing is the
> normal path, not the exception.** The review UI must therefore be built for
> bulk approval, with auto-resize enabled by default — not a per-file prompt
> repeated four hundred times.

---

### F13 — Remediation

| Condition | Actions offered |
|---|---|
| `no_date` | Enter manually · derive from batch median · use file modification time · skip |
| `date_out_of_range`, isolated | Redate manually · publish anyway · skip |
| `date_out_of_range`, whole batch | **Bulk shift by `now − median`** · publish anyway · skip |
| `too_many_pixels` | Resize to fit · publish anyway · skip |
| `too_large` | Re-encode at lower quality · resize · skip |

**Every action must be available as a bulk apply to all shots sharing a
failure.**

**Resize** preserves aspect ratio:

```
scale = sqrt(max_megapixels × 10⁶ / (w × h))
w′ = floor(w × scale)
h′ = floor(h × scale)
```

The result is encoded as JPEG, stepping quality down `95 → 88 → 82 → 75` until
the byte cap is satisfied.

> **Mandatory: resizing must preserve EXIF.** The metadata block is carried
> forward and `PixelXDimension` / `PixelYDimension` updated. Dropping EXIF at
> this step destroys the capture date that was just validated, and Google Photos
> would then file the photograph under its upload date instead of the date it was
> taken. This requires a dedicated round-trip test.

---

### F14 — RAW to JPEG

For shots with no JPEG companion. An ordered ladder; the first success wins.

1. **Embedded preview.** Nearly every RAW file contains a full-resolution JPEG
   rendered by the camera's own image engine — correct colour, correct tone
   curve, and effectively free to extract. This is the default and the preferred
   result.
2. **macOS ImageIO** (desktop only), via `objc2` bindings or the system `sips`
   utility. This is Apple's own RAW pipeline and produces better results than any
   pure-Rust decoder. Support is per-camera-model and tied to the OS version.
3. **`rawler`** — pure Rust. The fallback, and the only option on the Linux
   server.

The output then passes through F12 validation and F13 resize. Capture date,
camera and lens metadata are copied from the RAW into the JPEG.

**Formats:** `.dng .nef .arw .cr2 .raf`. Canon **CR3** uses an ISO-BMFF container
rather than a TIFF-based one and requires separate handling; it is out of scope
until needed.

---

### F15 — Publish to Google Photos

See §6.

---

### F16 — Deduplication ledger

The Google Photos API cannot be queried for what a library already contains
(§6.1), so deduplication is entirely local.

The server maintains a SHA-256 ledger of every file it has published.
**Re-ingesting a card that has already been processed must publish nothing.**

The desktop sends content hashes in its manifest **before** copying any bytes, so
known duplicates are never transferred.

---

### F17 — Jobs and progress

Scanning, converting and publishing take minutes. Every long-running operation is
a **job** with a persisted state row and a progress stream — Server-Sent Events
from the server, Tauri events on the desktop.

**No operation may block a request until it completes.** A request starts a job
and returns its identifier immediately.

Jobs survive a restart: an interrupted job resumes or reports failure, and never
silently disappears.

---

### F18 — Authentication

See §5.

---

## 5. Authentication and authorisation

### 5.1 Two separate systems

> **Firebase Authentication does not grant access to Google Photos.** These are
> independent concerns and must be designed as such.

Firebase's Google sign-in returns a **Firebase ID token** — a JWT identifying the
user to this system — and a short-lived Google OAuth **access token**, valid
about an hour. It **does not return a refresh token** for Google API scopes. A
service that uploads to Google Photos unattended requires a refresh token, and
the only way to obtain one is a separate OAuth 2.0 authorization-code flow with
`access_type=offline` (§6.2).

| Concern | Mechanism | Question answered |
|---|---|---|
| **Application access** | Firebase Authentication | "May this person use PhotoTools?" |
| **Google Photos** | Own OAuth 2.0 flow, offline access | "May PhotoTools add photographs to this Google account?" |

### 5.2 Design

- **Firebase Authentication** with the Google provider. Email/password may be
  enabled as a secondary option.
- Both front ends sign in and obtain a Firebase **ID token**.
- Every request to `phototools-server` carries
  `Authorization: Bearer <id-token>`.
- The server verifies tokens itself; no Firebase Admin SDK is required:
  1. Fetch and cache Google's public signing certificates.
  2. Verify the RS256 signature.
  3. Check `iss` equals `https://securetoken.google.com/<project-id>`, `aud`
     equals `<project-id>`, `exp` is in the future, and `sub` is present.
  4. Check `sub` against a configured **allow-list of permitted UIDs**.

  Approximately 100 lines using `jsonwebtoken` plus a cached key store.

- The desktop application signs in the same way and stores its refresh token in
  the **macOS Keychain**.

### 5.3 Consequences to design for

**Authentication is not authorisation.** Firebase will successfully authenticate
any Google account in existence. The **UID allow-list is the only thing
restricting access to the library.** It is not optional.

**ID tokens expire after one hour.** Front ends must refresh transparently. The
server must return a `401` with a distinguishable reason code so a client
refreshes and retries rather than dropping the user to a login screen.

**Sign-in requires internet access; verification does not.** Once signing
certificates are cached, token verification works offline — but a *fresh* sign-in
must reach Firebase. If the internet is down, nobody can newly authenticate to a
service sitting on the local network. A configured local administrative token
provides a documented break-glass path.

---

## 6. Google Photos integration

### 6.1 Constraints

These are properties of the Google Photos API and shape the design.

| Constraint | Consequence |
|---|---|
| The only usable scope is **`photoslibrary.appendonly`**. Broader read and sharing scopes were withdrawn on 31 March 2025. | Uploading media and creating albums work. Nothing else does. |
| **The library cannot be read.** | Deduplication must be local — F16. |
| **Nothing can be deleted through the API.** | A mistaken bulk publish must be cleaned up by hand in the Google Photos interface. **A dry run is therefore mandatory before any publish.** |
| Upload is two steps: `POST /v1/uploads` returns an upload token, then `POST /v1/mediaItems:batchCreate` creates items, **maximum 50 per call**. | 500 photographs ≈ 510 requests. |
| Quota is **10,000 requests per project per day**. | Comfortable at this volume. |
| `429` responses require **at least 30 seconds** before retrying. | Exponential backoff with a 30-second floor. |
| Photographs may be up to 200 MB. Uploads count against the account's storage quota. | Well within the 10 MB output cap. |

### 6.2 Authorisation

Use a **Web application** OAuth client, since the consent flow is driven from the
web UI served by the NAS.

1. The web UI sends the user to Google's consent screen with
   `scope=photoslibrary.appendonly`, `access_type=offline`, `prompt=consent`.
2. Google redirects to a server callback with an authorization code.
3. The server exchanges the code for an access token **and a refresh token**.
4. The refresh token is encrypted at rest using a key supplied by environment
   variable. It is never committed and never written inside the photo library.

> **A trap worth knowing about.** A Google Cloud project whose consent screen is
> left in **"Testing"** status issues refresh tokens that **expire after seven
> days**. This is a property of the *project*, not of the client type — no client
> configuration avoids it. Publish the consent screen to "In production";
> unverified is acceptable for personal use, showing a warning screen and capping
> at 100 users.
>
> Google documents only the Testing-status expiry, so **implement the reconnect
> path regardless**: catch `invalid_grant`, mark the connector disconnected, and
> prompt to re-authorise rather than failing silently.

### 6.3 Idempotency

`batchCreate` is not idempotent — a retry after a network timeout can create a
duplicate. Persist the state machine and resume only from the recorded state:

```
pending  →  uploaded (upload token held)  →  created (mediaItem id recorded)
```

### 6.4 Acceptance criterion

Before the first bulk run: publish one photograph and confirm that its capture
date survives the round trip and that Google Photos files it under the correct
day.

---

## 7. Data model

SQLite, held by the server, at a path outside the photo library.

```
users       (uid, display_name, added_at)
              -- Firebase UIDs on the allow-list

cards       (id, volume_label, fingerprint, first_seen, last_seen)

shots       (id, card_id, stem, candidate_asset_id, status)
              -- one row per photograph

assets      (id, shot_id, rel_path, kind{jpeg|raw|video},
             bytes, sha256, capture_datetime, width, height, camera)
              -- one row per file; a RAW+JPEG pair is two assets, one shot

checks      (shot_id, name, status{pass|warn|fail}, detail)

derived     (shot_id, staged_path, sha256, bytes, width, height)
              -- what will actually be published

publishes   (shot_id, upload_token, media_item_id,
             state{pending|uploaded|created|failed}, attempts, error)

jobs        (id, kind, state, progress, total,
             started_at, finished_at, error)

settings    (key, value)

oauth       (provider, encrypted_refresh_token, scope, expires_at)
```

`assets.sha256` is the authoritative deduplication key.

---

## 8. API

All endpoints require a valid Firebase ID token (§5.2) except `/api/health`. All
supplied paths are validated against the configured roots (§9.2).

```
# Archive tools — operate on library paths
POST   /api/tools/dates/scan              → job
POST   /api/tools/dates/fix               → job   { mode, dry_run, … }
POST   /api/tools/rename/plan             → plan
POST   /api/tools/rename/apply            → job
POST   /api/tools/split                   → job
POST   /api/tools/contact-sheet           → job
POST   /api/tools/transform               → job
POST   /api/tools/border                  → job
POST   /api/tools/tiff-to-jpeg            → job
GET    /api/storage/ls?path=              → listing

# Ingest — the desktop application is the producer
POST   /api/ingest/sessions               manifest → which hashes are new
POST   /api/ingest/sessions/{id}/ready    staged files written; begin server work
GET    /api/ingest/sessions/{id}/shots    results with per-check status
POST   /api/ingest/shots/{id}/action      resize | redate | skip | override
POST   /api/ingest/sessions/{id}/bulk-action
POST   /api/ingest/sessions/{id}/publish  { dry_run } → job

# Jobs
GET    /api/jobs/{id}                     → state
GET    /api/jobs/{id}/events              → SSE progress stream

# Google Photos connector
GET    /api/connectors/google/status
POST   /api/connectors/google/connect     → consent URL
GET    /api/connectors/google/callback
POST   /api/connectors/google/disconnect

GET    /api/health                        → { status, version }   unauthenticated
```

The desktop application calls `core` directly through Tauri `invoke` for local
work, and this API for anything the server owns.

> **Implementation note.** The desktop application's HTTP calls to the server are
> made from the **Rust side** using `reqwest`, not from the webview's JavaScript.
> This avoids CORS entirely, avoids mixed-content restrictions, and means plain
> HTTP over the local network requires no certificate.

---

## 9. Non-functional requirements

### 9.1 Performance

| Operation | Target |
|---|---|
| Scan a 400-frame card (metadata only) | < 10 s |
| Date scan of 500 library files | < 5 s |
| Contact sheet from 200 images | < 20 s |
| Resize and encode one 24 MP JPEG | < 150 ms |

These are achievable only under three rules:

1. **Metadata reading is in-process.** Never by invoking an external program.
2. **CPU-bound batch work is parallelised** with `rayon` across available cores.
3. **Pixel work is done over slices, not per-pixel through an abstraction.**
   Edge detection, trimming and profiling operate on row and column slices so the
   compiler can vectorise them.

### 9.2 Safety invariants

1. **An SD card is never written to.** Copy, verify by hash, process the copy.
2. **Filesystem access is confined to configured roots.** Every supplied path is
   canonicalised and rejected unless it resolves inside an allowed root.
   Traversal via `..` must be impossible.
3. **Every destructive operation supports a dry run.** Publishing requires one
   first.
4. **Secrets never enter the photo library and never enter the repository.**
5. **Originals are never modified in place by ingest.** Remediation writes new
   files.
6. **An operation reports only what it has verified.** If a write cannot be
   confirmed, it is reported as unconfirmed — never as success.

### 9.3 Deployment

**Server** — multi-stage Docker on a `distroless/cc` base, with `cargo-chef`
dependency-layer caching configured from the first commit. Built for multiple
architectures in one command:

```
docker buildx build --platform linux/amd64,linux/arm64
```

**Desktop** — `.app` and `.dmg` bundles. An unsigned build is acceptable for
personal use; distribution to others requires an Apple Developer account for
signing and notarisation.

### 9.4 Testing

- `core` compiles and passes its tests with no binary crate present.
- Every archive tool has fixture-based tests over sample images.
- The EXIF-preservation round trip (F13) has a dedicated test.
- Path-confinement (§9.2 rule 2) has tests covering `..`, symlinks and absolute
  paths.
- The Google Photos client is testable against a mock, with the 429 backoff and
  the `invalid_grant` reconnect path both covered.

---

## 10. Milestones

Estimates are focused working days for one developer.

| # | Milestone | Days | Delivers |
|---|---|---|---|
| 0 | Workspace and core skeleton | 2 | Cargo workspace, SQLite, configuration, error types, CI |
| 1 | Media layer | 4–5 | In-process EXIF, decode/encode, resize, persistent `exiftool` driver |
| 2 | Archive tools | 8–12 | F1–F9 in `core`, with fixture tests |
| 3 | Server and authentication | 4–5 | axum, Firebase token verification, UID allow-list, jobs and SSE, Docker |
| 4 | Web front end | 5–6 | Mobile-first UI over F1–F9 |
| 5 | Desktop shell | 3–4 | Tauri v2, shared components, `invoke` bridge |
| 6 | Card detection and scan | 3–4 | F10, F11 |
| 7 | Validation and remediation | 4–5 | F12, F13 |
| 8 | RAW to JPEG | 2–3 | F14 |
| 9 | Staging handoff and ledger | 3–4 | F16, manifest protocol, staging directory |
| 10 | Google Photos | 5–7 | F15, OAuth, reconnect path, dry run |
| 11 | Ingest UI | 4 | Review grid, bulk actions, progress |
| 12 | Packaging | 2–3 | `.dmg`, multi-architecture image, deployment documentation |

**Total ≈ 49–65 focused days.**

**Suggested order of proof.** Milestones 0–2 produce a working core with the
archive tools and no UI at all — this retires the largest technical risk, since
everything else builds on that layer. Milestone 3 makes the system usable.
Milestones 6–11 add ingest.

---

## 11. Open decisions

| # | Decision | Proposal |
|---|---|---|
| 1 | Does the web UI need ingest views, or is ingest desktop-only? | **Desktop-only.** The phone gets archive tools and publish monitoring. Simplifies both front ends. |
| 2 | Are full-resolution originals archived to the NAS? | **Yes**, copied in the background after publishing, since that transfer blocks nothing. |
| 3 | Album strategy in Google Photos | One album per card, one per date, or straight to the library — undecided. |
| 4 | Auto-resize default | **On.** A 10 MP ceiling means nearly every modern frame needs it. |
| 5 | Date validation reference point | **Relative to now**, with the batch-median clock check in F12. |
| 6 | Firebase project | New project, or reuse an existing one — undecided. |
| 7 | Code signing | Unsigned build for personal use, or an Apple Developer account. |

---

## Appendix — external references

- [Updates to the Google Photos APIs](https://developers.google.com/photos/support/updates)
- [Upload media — Google Photos Library API](https://developers.google.com/photos/library/guides/upload-media)
- [mediaItems.batchCreate](https://developers.google.com/photos/library/reference/rest/v1/mediaItems/batchCreate)
- [Google Photos API limits and quotas](https://developers.google.com/photos/overview/api-limits-quotas)
- [Authenticate Using Google with JavaScript — Firebase](https://firebase.google.com/docs/auth/web/google-signin)
- [Manage User Sessions — Firebase Authentication](https://firebase.google.com/docs/auth/admin/manage-sessions)
- [Using OAuth 2.0 to Access Google APIs](https://developers.google.com/identity/protocols/oauth2)
- [Tauri v2](https://v2.tauri.app/)
- [rawler](https://lib.rs/crates/rawler)
