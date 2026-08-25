# Testing on a Mac

Phases 0–14 are built and pass their gates. What is left is everything a machine with no camera, no
Mac, no NAS and no Google account could not settle — the numbered checks in
[`manual-verification.md`](manual-verification.md).

**51 checks in all, and all of them are actionable.**

This file gets you to the point where you can start them.

---

## 1. Prerequisites

| | Version | Install |
|---|---|---|
| Rust | stable ≥ 1.80 | `rustup toolchain install stable` — `rustfmt` and `clippy` come from `rust-toolchain.toml` |
| Node | ≥ 20 (CI uses 22) | `brew install node` |
| `exiftool` | ≥ 12 | `brew install exiftool` — **metadata writing only** (G3) |
| Xcode CLT | current | `xcode-select --install` — Tauri needs it |
| `tauri-cli` | v2 | `cargo install tauri-cli --version "^2"` — not a workspace dependency |

Check the one that actually bites:

```sh
exiftool -ver    # must print 12 or higher
```

## 2. Confirm the Linux gates still pass here

Before touching anything Mac-specific, reproduce what CI already knows. If these fail on your
machine, the problem is the environment, not the phase you are about to test.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace          # expect 504 passed
cargo test -p phototools-core   # expect 423 passed — G2, core in isolation
```

Front ends:

```sh
npm --prefix frontend/shared ci && npm --prefix frontend/shared run build
npm --prefix frontend/web ci
npm --prefix frontend/web run typecheck
npm --prefix frontend/web run build
npm --prefix frontend/web run check:transport   # no view reaches the network directly
npm --prefix frontend/web run check:layout      # 10 routes at 390 px
npm --prefix frontend/web run check:ingest      # Phase 13 acceptance, measured

npm --prefix frontend/desktop ci
npm --prefix frontend/desktop run typecheck
npm --prefix frontend/desktop run build
npm --prefix frontend/desktop run check:transport
```

`check:layout` and `check:ingest` drive a real browser and write screenshots to
`frontend/web/layout-proof/` (gitignored). Look at them.

> The `target/` directory reaches **20 GB or so** on a full debug build — `rawler` and rustls'
> `aws-lc-sys` account for most of it. Budget the disk.

> **The first build is slow on purpose.** `Cargo.toml` optimises dependencies *and*
> `phototools-core` even in the dev profile. Without it a 36 MP TIFF takes 7 s to convert rather
> than 0.26 s, because `fast_image_resize` is generic and its kernels compile inside `core` — so
> optimising dependencies alone does almost nothing. Any timing taken without that section is
> meaningless.

## 3. Configuration — read this before the first run

> ### The one that will catch you
>
> **`ROOTS` is empty by default, and an empty `ROOTS` refuses every path.**
>
> G6 canonicalises every supplied path and rejects anything outside a configured root. With none
> configured, nothing is inside one, and every operation answers *"That path is outside the folders
> this application may touch."* That is correct behaviour and a baffling first run.

Configuration is read by `Config::load()`, which uses
`~/Library/Application Support/masterphototools/config.json` **if it exists**, and otherwise falls
back to environment variables. Copy `.env.example` and fill it in, or write the JSON directly.

> **A `ROOTS` entry that does not exist does not fail loudly.** `Config::from_env()` rejects it, but
> both binaries do `Config::load().unwrap_or_else(|_| Config::default())` — so one bad entry
> discards the *whole* configuration and falls back to defaults, which have **no roots at all**.
>
> The server at least logs *"ROOTS is empty, so every filesystem request will be refused"*. **The
> desktop says nothing**, and every path you type is refused with no clue why. If that happens,
> suspect a typo in `ROOTS` before suspecting the path you just entered.

The variables that matter on day one:

| Variable | Default | What it does |
|---|---|---|
| `ROOTS` | *empty* | Colon-separated directories the app may touch. Each entry is canonicalised at load, so each must exist — see the warning below. |
| `DATABASE_PATH` | `/tmp/phototools.db` | The SQLite ledger. Put it somewhere that survives a reboot. |
| `STAGING_DIR` | `/tmp/phototools-staging` | Local scratch for F11's copy off the card. **Not** the NAS staging directory — that one is typed into the Ingest screen. |
| `MAX_MEGAPIXELS` | `0` | F12's resolution ceiling. **Zero means no ceiling** — the default. |
| `MAX_OUTPUT_BYTES` | `10485760` | F12's size ceiling. |
| `MAX_AGE_DAYS` | `90` | F12's capture-date window. |

Server-only, and needed from Phase 11 onwards:

| Variable | Needed for |
|---|---|
| `ALLOWED_UIDS` | **The only thing restricting access to the library** (§5.3). Not optional. |
| `FIREBASE_PROJECT_ID` | Token verification |
| `ADMIN_TOKEN` | The desktop→server handoff today — see **MV-11.1** |
| `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET` / `_REDIRECT_URI` | Phase 12 |
| `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` | Phase 12. `openssl rand -hex 32`. Publishing refuses to store a token without it rather than storing it in the clear. |

A sanity check that the roots took:

```sh
ROOTS=/Users/you/Pictures cargo run -p phototools-server
# then, in another shell:
curl -s localhost:3000/api/health
```

`/api/health` is the one unauthenticated route. Everything else answers `401` without a token.

## 4. Running the two applications

**Desktop** — from `crates/desktop`:

```sh
cargo tauri dev      # builds the Vue front end and opens the window
cargo tauri build    # produces the .app and .dmg
```

**Server** — from the repository root:

```sh
cargo run -p phototools-server     # PORT defaults to 3000
```

**Web front end** in development:

```sh
npm --prefix frontend/web run dev  # proxies /api to 127.0.0.1:3000
```

> Deploying for real — the container on the NAS, the `.dmg`, Firebase and the OAuth client — is
> [`deployment.md`](deployment.md). This file is for running from a checkout while verifying.

## 5. Suggested order

Each session unblocks the next. Doing them out of order mostly means discovering the same setup
problem later and more confusingly.

| # | Session | Items | Why here |
|---|---|---|---|
| 1 | **Get it running** | MV-7.1 | Nothing Mac-specific can be checked until the window opens. |
| 2 | **Desktop basics** | MV-7.2 – MV-7.5 | Keychain, job events, offline behaviour. No card needed. |
| 3 | **A real card** | MV-8.1 – MV-8.7 | Detection, the debounce, **G5 byte-identity**, and the real scan time. |
| 4 | **Real files** | MV-2.1 – MV-2.3, MV-9.1 – MV-9.4, MV-10.1 – MV-10.5 | Metadata, resize quality, RAW colour. Needs photographs, not hardware. |
| 5 | **The NAS** | MV-11.1 – MV-11.5 | Set `ADMIN_TOKEN` first; the rest is SMB behaviour. |
| 6 | **Google Photos** | MV-12.1 – MV-12.7 | Needs an OAuth client. **MV-12.1 is one photograph, before any bulk run.** |
| 7 | **The UI** | MV-13.1 – MV-13.6, MV-6.1 | Judgement about screens, once there is real data behind them. |
| 8 | **Scans** | MV-4.1 – MV-4.4 | Independent of everything above; do whenever you have scans. |
| 9 | **Packaging** | MV-14.1 – MV-14.3 | Last, because it is the only session that needs everything else to have worked. |

### If you only have an hour

Three checks carry more weight than the rest, because each can invalidate work already treated as
done:

1. **MV-10.2** — are your cameras' embedded previews full-resolution? A screen-sized preview passes
   every automated test in Phase 10 and produces derivatives nobody would want to keep.
2. **MV-8.6** — is the card byte-identical after a scan? G5 is the invariant the whole ingest design
   rests on.
3. **MV-12.1** — publish one photograph and check the date. §6.4 asks for this before any bulk run,
   and the API cannot delete what it creates.

## 6. Recording what you find

Tick the box in `manual-verification.md` and replace the `**Result:**` line with what happened. A
failure is a result: write the number, the message, or the sentence, and open the work separately.

If a check finds a defect, [`known-gaps.md`](known-gaps.md) is where the open ones live — add it
there so it does not exist only in a ticked box.

## 7. Working with Claude Code on this

[`CLAUDE.md`](../CLAUDE.md) at the repository root carries the project's ground rules, the gate
commands and the current state, so a session in your IDE starts oriented. Naming items works:

> *"Do MV-8.6 with me — set up the before-and-after hashes."*
>
> *"MV-11.2 came back with a recopy count of 380 out of 400. Work out why."*

The ground rules in `CLAUDE.md` — G1 to G11 — apply to an agent's changes as much as to the phases
already built. **G7 in particular: a failing test gets the code fixed or the problem reported. It
never gets the test weakened.**
