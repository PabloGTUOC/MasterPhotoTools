# Deployment

Two things get deployed, and they are independent of each other: a **server** in
Docker on the NAS, and a **desktop application** on the Mac. The server can run
without the desktop; the desktop's local tools work without the server, and only
the handoff and publishing need it.

Read this in order the first time. The two sections that catch people are
[roots](#3-roots-the-one-that-refuses-everything) and
[the OAuth redirect](#7-google-photos), and both fail in ways that do not say
what is wrong.

---

## 1. What you need before starting

| For | You need |
|---|---|
| The server | A NAS running Docker, and a directory holding the photo library |
| Sign-in | A Firebase project — free tier is enough |
| Publishing | A Google Cloud project with the Photos Library API enabled |
| The desktop | A Mac on macOS 14 or later, Rust, Node 20+, `exiftool`, Xcode command line tools |

Nothing here needs a paid account except distributing the `.app` to somebody
else, which needs an Apple Developer account for signing.

---

## 2. The server

### Build and run

The build context is the repository root, not `deploy/`:

```sh
docker compose -f deploy/docker-compose.yml build
docker compose -f deploy/docker-compose.yml up -d
```

Compose reads its values from the environment or from a `.env` file **beside
`docker-compose.yml`** — that is `deploy/.env`, not the repository root. Four are
required and the file refuses to start without them:

```sh
# deploy/.env
LIBRARY_PATH=/volume1/photo
DATA_PATH=/volume1/docker/phototools
FIREBASE_PROJECT_ID=your-project-id
ALLOWED_UIDS=aBcDeFgHiJkLmNoPqRsTuVwXyZ12
```

Confirm it came up:

```sh
curl -s http://nas.local:3000/api/health
# {"status":"ok","version":"0.1.0"}
```

`/api/health` is the only route that answers without a token (§5.3), which is
what makes it usable as the container's health check. Everything else is `401`.

### Multi-architecture images

A Synology or QNAP on Intel is `linux/amd64`; most ARM NAS boxes and an Apple
Silicon Mac are `linux/arm64`. One command covers both, and it needs a registry
because a manifest list cannot be loaded into the local daemon:

```sh
docker buildx create --use --name phototools   # once
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f deploy/Dockerfile \
  -t your-registry/phototools-server:0.1.0 \
  --push .
```

To build only for the machine you are on, `--load` instead of `--push` and drop
the second platform. **Building the other architecture under emulation is slow**
— the Rust tree includes `rawler`, `mozjpeg` and `aws-lc`, and the emulated pass
is measured in tens of minutes rather than minutes.

### What is in the image, and why it is not `distroless`

Specification §9.3 asks for a `distroless/cc` base. This image is
`debian:bookworm-slim` instead, and the reason is `exiftool`.

§2.6 makes `exiftool` the one permitted external binary and requires it for
every metadata **write**. Two shipped routes perform one — `POST
/api/tools/dates/fix` (F1) and `POST /api/ingest/derive` (F14). `exiftool` is a
Perl program, and `distroless/cc` ships a C runtime with no interpreter, so on
that base both routes fail at the first write. The choice was a Perl-bearing
base or withdrawing two specified routes. See
[`phase-reports/phase-14.md`](phase-reports/phase-14.md).

### The web UI

The container serves it. The image builds `frontend/web` and the server hands it
to the browser from `WEB_ROOT` (§2.2), so `http://nas.local:3000/` is the
application and `/api/*` is the API underneath it. There is no second container
and no reverse proxy to configure.

In development this is reversed: Vite serves the front end on port 5173 and
proxies `/api` to the server on 3000, so `WEB_ROOT` stays unset and the server
serves API only.

---

## 3. Roots — the one that refuses everything

> **`ROOTS` is empty by default, and an empty `ROOTS` refuses every path.**

Every path arriving from the API or the UI is canonicalised and checked against
`ROOTS` before any filesystem access (G6, §9.2 rule 2). With none configured,
nothing is inside one, and every operation answers *"That path is outside the
folders this application may touch."*

Two further traps:

- **A `ROOTS` entry that does not exist discards the whole configuration.**
  `Config::from_env()` rejects it, but both binaries fall back to
  `Config::default()` on any error — and the default has no roots at all. The
  server logs `ROOTS is empty, so every filesystem request will be refused`; the
  desktop says nothing. Suspect a typo in `ROOTS` before suspecting the path you
  typed.
- **In the container, `ROOTS` is the path *inside* the container.** The compose
  file mounts `LIBRARY_PATH` at `/library` and sets `ROOTS=/library`. Setting
  `ROOTS` to the NAS-side path refuses everything.

---

## 4. Every environment variable

### Read by `phototools-core`, so both the server and the desktop

| Variable | Default | Meaning |
|---|---|---|
| `ROOTS` | *empty* | Colon-separated directories the application may touch. Each is canonicalised at load and **must exist**. Empty refuses everything. |
| `STAGING_DIR` | `/tmp/phototools-staging` | Where F11 copies files off the card. Local scratch on the Mac; **not** the NAS staging directory, which is typed into the Ingest screen. |
| `DATABASE_PATH` | `/tmp/phototools.db` | The SQLite ledger. §7 requires it outside the photo library. Put it somewhere that survives a reboot — the default does not. |
| `MAX_AGE_DAYS` | `90` | F12 rejects a capture date further than this from now. |
| `MAX_MEGAPIXELS` | `0` | F12's resolution ceiling, in megapixels. **Zero means no ceiling**, which is the default: publishing is limited by file size, and a frame inside the byte cap is worth keeping whole. Set it to restore §F12's 10. |
| `MAX_OUTPUT_BYTES` | `10485760` | F12's size ceiling, applied independently of the resolution one. |

A threshold that is set but unparseable is a **startup error**, not a silent
fallback to the default (§9.2 invariant 6).

On the Mac, `~/Library/Application Support/masterphototools/config.json` is used
**instead of** the environment if it exists. The environment is only read when
that file is absent.

### Server only

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `3000` | TCP port. |
| `WEB_ROOT` | *unset* | Directory holding the built web front end. Set to `/srv/web` in the image. Unset means the server serves API only. A path with no `index.html` is logged and ignored. |
| `FIREBASE_PROJECT_ID` | *unset* | Token `iss` must equal `https://securetoken.google.com/<id>` and `aud` must equal the id exactly. |
| `ALLOWED_UIDS` | *empty* | Comma-separated Firebase uids permitted to use the system. **Firebase authenticates any Google account in existence; this list is the only thing restricting access to the library** (§5.3). Empty means nobody. |
| `ADMIN_TOKEN` | *unset* | Break-glass token for when Firebase is unreachable (§5.3). Also what the desktop handoff currently authenticates with — see §6. Empty is treated as unset. |
| `RUST_LOG` | `phototools_server=info,tower_http=info` | Tracing filter. |

### Server only, for publishing

| Variable | Meaning |
|---|---|
| `GOOGLE_OAUTH_CLIENT_ID` | From a **Web application** OAuth client. |
| `GOOGLE_OAUTH_CLIENT_SECRET` | Same client. |
| `GOOGLE_OAUTH_REDIRECT_URI` | Must be registered on that client **character for character**. |
| `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` | 32 bytes, hex: `openssl rand -hex 32`. Connecting refuses outright without it rather than storing the token in the clear. |

All four are read when publishing is used, not at startup, so a server with none
of them set starts and serves everything except Google Photos.

### Web front end, at build time

Vite reads these when `frontend/web` is **built**, so changing one needs a
rebuild, not a restart. In the container that means rebuilding the image.

| Variable | Meaning |
|---|---|
| `VITE_FIREBASE_API_KEY` | From the Firebase web app configuration. |
| `VITE_FIREBASE_AUTH_DOMAIN` | Usually `<project-id>.firebaseapp.com`. |
| `VITE_FIREBASE_PROJECT_ID` | The project id. |
| `VITE_FIREBASE_APP_ID` | The web app id. |
| `VITE_API_BASE_URL` | Where the API is. Leave unset when the server serves the front end, so requests stay same-origin. |

---

## 5. Firebase

1. Create a project at <https://console.firebase.google.com>.
2. **Authentication → Sign-in method → Google → Enable.**
3. **Project settings → Your apps → Web** — register one, and copy the four
   values into the `VITE_FIREBASE_*` variables above.
4. Sign in once in the browser, then read your uid from **Authentication →
   Users**, and put it in `ALLOWED_UIDS`.

Step 4 is the one people skip. Until your uid is in `ALLOWED_UIDS`, sign-in
succeeds and every subsequent request is a `403` — which reads like a broken
deployment rather than a missing list entry.

The server verifies tokens itself against Google's published keys; there is no
Firebase Admin SDK and no service-account key to install (§5.3).

---

## 6. The desktop application

### Build

```sh
npm --prefix frontend/shared ci && npm --prefix frontend/shared run build
npm --prefix frontend/desktop ci
cargo install tauri-cli --version "^2"     # once
cd crates/desktop && cargo tauri build
```

The `.app` and `.dmg` land in `target/release/bundle/`. `cargo tauri dev` runs it
without bundling.

**The build is unsigned.** §9.3 accepts that for personal use. macOS will refuse
to open it on first launch: **right-click → Open**, then confirm, or clear the
quarantine attribute with
`xattr -dr com.apple.quarantine /Applications/MasterPhotoTools.app`. Giving it to
somebody else needs an Apple Developer account for signing and notarisation.

### Configure

Settings live in `~/Library/Application Support/masterphototools/config.json`,
written by the application's own settings screen. It takes precedence over the
environment.

### Point it at the server

The handoff authenticates with the **`ADMIN_TOKEN`**, set in the desktop's server
settings as `auth_token`. This is §5.3's break-glass path, used because the
desktop has a Keychain but no Firebase sign-in yet — see
[`known-gaps.md`](known-gaps.md#the-desktop-has-no-firebase-sign-in). Without it
every handoff request is a `401`. When sign-in is wired, the same field carries
an ID token and nothing else changes.

### Permissions macOS will ask for

- **Notifications** — F10 raises one when a card is detected. If it was refused
  once, no card will announce itself again until it is re-enabled in **System
  Settings → Notifications**.
- **Removable volumes** — reading `/Volumes` for card detection.

### Launch at login

Off by default. It is a macOS LaunchAgent, written by the application when the
setting is turned on. There is no control for it in the UI yet — the commands
exist (`set_launch_at_login`), but no screen calls them.

---

## 7. Google Photos

1. In Google Cloud console, create a project and **enable the Photos Library
   API**.
2. **Credentials → Create credentials → OAuth client ID → Web application.**
3. Add the redirect URI. It must match `GOOGLE_OAUTH_REDIRECT_URI` **character
   for character**, including scheme and port:
   `http://nas.local:3000/api/connectors/google/callback`
   > A mismatch fails at Google's end, with an error this server never sees. If
   > the consent screen returns an error and the logs show nothing, this is why.
4. **OAuth consent screen** — set it to **In production**.
   > A project left in **Testing** issues refresh tokens that expire after
   > **seven days**, and no client configuration avoids it. The reconnect path
   > is built and works, but it becomes a weekly interruption. "In production"
   > without verification is fine for personal use: it shows a warning screen
   > and caps at 100 users.
5. Generate the encryption key and set it:
   ```sh
   openssl rand -hex 32
   ```
6. Connect from the web UI, then confirm the token is not readable:
   ```sh
   sqlite3 "$DATABASE_PATH" "SELECT * FROM oauth;"
   # v1:<hex>:<hex>  — anything resembling a token is badly wrong
   ```

**Before any bulk run, publish one photograph and check the date** (§6.4). The
Google Photos API cannot delete what it creates, so a mistake found on frame 400
cannot be undone.

---

## 8. Backups

Back up **`DATABASE_PATH`**. It is the ledger of everything published, and F16's
deduplication is keyed on it: losing it means the next publish cannot tell which
photographs Google already has, and the API offers no way to remove the
duplicates that follow.

The staging directory is not backed up on purpose — it is derivable, and keeping
it is what lets an interrupted publish resume without re-transferring gigabytes.
Nothing prunes it, so it grows with every card
([`known-gaps.md`](known-gaps.md#nothing-cleans-the-staging-directory)).

---

## 9. When it will not start

| Symptom | Cause |
|---|---|
| Every path is *"outside the folders this application may touch"* | `ROOTS` unset, or one entry does not exist and discarded the rest. On the server, `ROOTS` must be the **container** path. |
| Sign-in works, every request is `403` | Your uid is not in `ALLOWED_UIDS`. |
| Every request is `401` | No token, or `FIREBASE_PROJECT_ID` does not match the token's `aud`. From the desktop, `ADMIN_TOKEN` is unset or does not match. |
| `/` is a 404, `/api/health` works | `WEB_ROOT` holds no `index.html`. The server logs which path it looked at. |
| Connecting Google returns an error the logs never mention | The redirect URI does not match the OAuth client exactly. |
| Publishing refuses to store the token | `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` unset or not 32 hex-encoded bytes. |
| The container is `unhealthy` | The health check is `curl` against `/api/health` inside the container. `docker compose logs server` — a failed `Ledger::open` is the usual reason, and it means `/data` is not writable by uid 10001. |
