# Phase 14 — Packaging and deployment

**Status:** complete with gates — two acceptance criteria need a human (MV-14.1, MV-14.2).

Shippable artefacts: a multi-architecture server image, a compose deployment, the macOS bundle, and
a deployment document. Two things had to be built that the phase's task list does not mention, and
both are recorded below rather than folded in quietly.

---

## Delivered

| | |
|---|---|
| `deploy/Dockerfile` | Four stages — `chef`, `planner`, `builder`, `runtime` — with `cargo-chef` dependency caching, plus a Node stage building the web front end. |
| `deploy/docker-compose.yml` | Volume mounts, the full environment, a health check, and `no-new-privileges`. |
| `.dockerignore` | Build context 1.6 MB. Without it the context is `target/`, which is 10–20 GB. |
| `docs/deployment.md` | Server, desktop, Firebase, Google OAuth, every environment variable, and a table of failures with their causes. |
| `crates/desktop/tauri.conf.json` | `app` and `dmg` targets, icons, category, `minimumSystemVersion` 14.0. |
| Launch at login | `tauri-plugin-autostart`, off by default, with `get_launch_at_login` / `set_launch_at_login`. |
| `crates/server` static serving | §2.2's "web UI served by `phototools-server`", which no phase had implemented. |

## Measurements

Taken on an Apple Silicon Mac, Docker 29.4.

| | |
|---|---|
| Image size | **295 MB** |
| `linux/arm64` build, cold | **~4 min** (`cargo chef cook` 67 s of it) |
| `linux/arm64` build, source-only change | dependency layer cached; server compile only |
| Build context | **1.6 MB** |
| `exiftool` in the image | **12.57** (§3 requires ≥ 12) |
| Container health | **healthy** within 10 s of start |
| Runs as | uid 10001, non-root |

Verified against the running container: `/api/health` 200; `/` 200 `text/html`; `/publish` 200
`text/html` (client-side route, not a 404); `/assets/index-*.js` 200 `text/javascript`, 286 KB;
`/api/no-such-thing` 404 `{"code":"not_found"}`; `/api/storage/ls` 401 `missing_token`.

---

## Deviations

### The runtime base is `debian:bookworm-slim`, not `distroless/cc`

Specification §9.3 and build plan task 1 both name `distroless/cc`. **`exiftool` is a Perl program
and `distroless/cc` has no interpreter.** §2.6 makes `exiftool` mandatory for every metadata write,
and two shipped server routes perform one — `POST /api/tools/dates/fix` (F1) and
`POST /api/ingest/derive` (F14, which copies metadata onto each derivative).

On a `distroless/cc` base both routes fail at their first write: a capability the API offers and the
image cannot honour, which §9.2 invariant 6 and the project's own rule about type-system
capabilities both argue against shipping.

The alternatives considered were copying a Perl runtime into a distroless image — brittle across
architectures and Perl versions, and failing at runtime on the NAS rather than at build time — or
withdrawing the two routes from the server build, which is a larger decision than a base image and
one the specification does not authorise. Recorded in
[`known-gaps.md`](../known-gaps.md#93s-distrolesscc-base-cannot-run-26s-exiftool).

The cost is image size: 295 MB against roughly 120 MB for a distroless equivalent.

### `tauri-plugin-autostart` is a new dependency (G8)

Build plan task 4 asks for launch at login. §2.6 lists no mechanism for it, and the macOS mechanism
is a LaunchAgent. The plugin is the maintained Tauri v2 way to write one, and is registered with
`MacosLauncher::LaunchAgent`.

**It is off by default.** A login item nobody asked for is a bad surprise, so registering the plugin
only makes the setting available. `set_launch_at_login` reports the state it could confirm rather
than the state it was asked for (§9.2 invariant 6) — the write can fail on its own, and a UI that
answers "on" because "on" was requested would be lying after the next reboot.

**No UI control calls these commands.** Adding one is Phase 13's screen work, not this phase's
(G11). The commands exist and the plugin works; nothing in the application turns it on yet.

---

## Defects found while building this

### The notification plugin was never registered — F10's notification would panic

`tauri-plugin-notification` was a dependency, `notification:default` was granted in
`capabilities/default.json`, and `main.rs` called `handle.notification()` — but nothing ever called
`.plugin(tauri_plugin_notification::init())`.

`NotificationExt::notification()` resolves through `Manager::state`, which **panics** when the type
was never managed: `state() called before manage() for ...`. So the first detected card would panic
the detection thread. The `if let Err(e)` around `.show()` could not catch it, because the panic
happens before `.show()` is reached.

Fixed by registering the plugin. This is a macOS-only path with no automated coverage — **MV-8.1 is
what confirms it**, and it would have failed there with a puzzling silence.

### `check:ingest` could not run on macOS

The Phase 13 acceptance script started a Vite dev server and then navigated to a hardcoded
`http://127.0.0.1:5173`. Vite binds to `localhost`, which Node resolves in the host's DNS order —
`::1` first on macOS, `127.0.0.1` first on the Linux CI runner. The gate therefore passed in CI and
failed here with `ERR_CONNECTION_REFUSED`.

It now asks the socket where it actually bound, which also fixes a latent second bug: it read the
*requested* port from the Vite config rather than the bound one, so it would have pointed at the
wrong port whenever 5173 was already taken. **No budget or assertion was changed** (G7).

---

### `cargo tauri dev` and `build` never ran the front-end command

`beforeDevCommand` was `npm --prefix ../../frontend/desktop run dev`, a path relative to
`crates/desktop`. **Tauri runs these commands from the parent of the app directory**, so it resolved
to `<repo>/../frontend/desktop` — outside the repository — and npm reported a missing
`package.json`. Established with a `pwd` probe rather than inferred.

Both commands now use the object form with an explicit `cwd`, which *is* resolved against the
config file. `beforeBuildCommand` had the identical fault, so `cargo tauri build` — and therefore
the `.dmg` — could not have worked either.

### The application icons are empty placeholders, and one of them aborts the process

`icons/icon.icns` and `icons/icon.ico` are **0 bytes**; `icons/icon.png` is 1×1. On macOS Tauri
sets the application icon through `NSImage::initWithData(...).expect("creating icon")`, and an
empty file makes that `None` — a panic in a non-unwinding context, so the process aborts before the
window is created.

`bundle.icon` now lists only the three valid PNGs, which is what let the window open. **This is a
partial fix.** Build plan task 4 asks for an application icon, and a `.dmg` wants a real `.icns`;
supplying one is a decision about how the application should look, not a packaging step (G11). Open
in [`known-gaps.md`](../known-gaps.md#the-application-icons-are-placeholders) and blocking
**MV-14.1**.

### Neither Vite dev server could serve its own `index.html`

Both `vite.config.ts` files set `server.fs.allow` to `[src, ui]`. Naming `allow` **replaces** Vite's
default of the package root, and `index.html` sits at that root — so the dev server refused the entry
document. Neither dev server had ever been started: CI builds the front ends and drives the *built*
output, and `check:layout` and `check:ingest` serve `dist` from their own HTTP server.

Fixed in `frontend/desktop` and `frontend/web` alike.

## Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 467 passing (461 + 6 new)
- [x] `cargo test -p phototools-core` — 400 passing (G2 isolation, unchanged)
- [x] Front-end gates — 8 of 8, including `check:ingest` now that it runs here
- [x] The desktop application builds and opens a window on macOS (`cargo tauri dev`)
- [x] The image builds for `linux/arm64` and the container passes its health check
- [~] `linux/amd64` **compiles and links** under emulation (3 m 11 s). The local `--load` export
      then failed after a 95-minute emulated layer export with a containerd lease error
      (`failed to open writer: lease does not exist`). That is the daemon, not the build — but
      **no amd64 container has been started**, so it is not a pass. `--push` to a registry is the
      documented path and does not go through this exporter. **MV-14.3.**
- [ ] **MV-14.1** — the `.dmg` installs and launches on macOS *(human)*
- [ ] **MV-14.2** — the container deploys to the NAS and passes its health check *(human, needs the NAS)*

## Gates

**MV-14.2 was verified locally, not on the NAS.** The container was built, started and driven on an
Apple Silicon Mac. What that does not establish is the NAS: its architecture, its Docker version,
whether `/volume1/...` bind mounts arrive with ownership uid 10001 can write to, and whether the
SMB-backed library behaves as a local mount. The `unhealthy` row in `deployment.md`'s failure table
exists because a non-writable `/data` is the likeliest form of that.

**The `.dmg` is unsigned** (§9.3 accepts this for personal use). macOS will refuse it on first
launch until it is opened through the context menu or the quarantine attribute is cleared;
`deployment.md` says so. Distribution to anyone else needs an Apple Developer account.

## Ideas not acted on (G11)

- **A `cargo-chef` toolchain layer.** `rust-toolchain.toml` pins `channel = "stable"`, so `rustup`
  re-syncs the toolchain in the `planner` and `builder` stages even though the base image already
  carries one. Materialising it in the `chef` stage would remove that from both children. It is a
  build-time saving, not a correctness matter.
- **Staging retention.** `known-gaps.md` parks this with "Phase 14's deployment decisions". Deleting
  safely needs the ledger to know a file is published *and* unreferenced, which is a retention
  policy rather than a packaging task. `deployment.md` says what to back up and what not to, and
  leaves the policy open.
