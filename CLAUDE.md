# PhotoTools — working notes for Claude Code

Ingest photographs from a camera SD card, validate and prepare them, publish them to Google Photos,
and maintain a photo library on a NAS.

## Where the current work is

**Phases 0–14 are built and pass their gates.** Every phase of the build plan is closed.

The active job is **using the application on a Mac and fixing what that finds.** Two sessions of
working through the tabs with real photographs turned up sixteen defects that no test had caught —
a rename that renamed the folder instead of the photographs, a metadata reader that silently
returned nothing for camera TIFFs, tools that reported success having done nothing. Most shared one
cause: **the tools had only ever been exercised with typed file paths, and the folder pickers made
pointing at a folder the normal gesture.**

The numbered checks in [`manual-verification.md`](docs/manual-verification.md) remain the
structured half of that work — 64 of them, all actionable.

| Read this | For |
|---|---|
| [`docs/testing.md`](docs/testing.md) | Setting up on the Mac, and the order to work through the checks |
| [`docs/manual-verification.md`](docs/manual-verification.md) | The checks, each with a stable id like `MV-8.3` |
| [`docs/known-gaps.md`](docs/known-gaps.md) | What is open **in the code**, as opposed to awaiting a human |
| [`docs/geotag-plan.md`](docs/geotag-plan.md) | The Geotag tab: joining photographs to a phone's GPS track. Built; MV-15 awaits a Mac |
| [`docs/deployment.md`](docs/deployment.md) | Deploying the server and the desktop app, and every environment variable |
| [`docs/phase-reports/`](docs/phase-reports/) | One report per phase: what was delivered, what deviated, and why |

The user may name items directly — *"do MV-8.6"*, *"MV-11.2 came back at 380 of 400, work out
why"*. Look the item up before acting; each says what passing looks like.

## The authority documents

| File | Is the authority on |
|---|---|
| [`SPECIFICATION.md`](SPECIFICATION.md) | **What** the system does. F1–F18, architecture, API, data model. |
| [`BUILDPLAN.md`](BUILDPLAN.md) | **How** it gets built. Fifteen phases, acceptance criteria, ground rules. |

**Do not edit `SPECIFICATION.md`** (G9). If it is wrong or ambiguous, say so and record it in
`docs/known-gaps.md`.

## Ground rules

From the build plan. They apply to any change, not only to the phases already built.

| | |
|---|---|
| **G1** | No functionality in a binary crate. `server` and `desktop` hold transport, platform integration and process lifecycle only. If you are about to write logic in a binary, move it to `core`. |
| **G2** | `core` must compile and pass its tests with **no binary crate present**. CI runs `cargo test -p phototools-core` in exactly that isolation. |
| **G3** | Never invoke `exiftool` to **read** metadata. Reads are in-process, through `nom-exif`. |
| **G4** | Never spawn one `exiftool` process per file. Writes go through the single persistent driver (`media::meta::ExifWriter`). |
| **G5** | **Never write to a source SD card.** Copy, verify by hash, operate on the copy. |
| **G6** | Every path from an API or UI is canonicalised and checked against configured roots before any filesystem access. |
| **G7** | **Never weaken a test to make it pass.** Fix the code or report the problem. Deleting, skipping or `#[ignore]`-ing a failing test is not a fix. |
| **G8** | No dependencies beyond specification §2.6 without recording the reason in a phase report. |
| **G9** | Do not edit `SPECIFICATION.md`. |
| **G10** | No `unimplemented!()`, `todo!()`, or silently swallowed errors on a shipped path. |
| **G11** | Do not invent scope. Ideas for more go in a report, not the code. |

## Gates — everything below must pass before anything is called done

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace          # 632 passing
cargo test -p phototools-core   # 551 passing — G2
```

Front ends:

```sh
npm --prefix frontend/shared run build     # the shared client, consumed as compiled output

npm --prefix frontend/web run typecheck
npm --prefix frontend/web run build
npm --prefix frontend/web run check:transport   # no view reaches the network directly
npm --prefix frontend/web run check:layout      # 10 routes clean at 390 px
npm --prefix frontend/web run check:ingest      # Phase 13 acceptance, measured in a browser

npm --prefix frontend/desktop run typecheck
npm --prefix frontend/desktop run build
npm --prefix frontend/desktop run check:transport
```

`check:layout` and `check:ingest` drive Chromium and write screenshots to
`frontend/web/layout-proof/`. They assert **numbers**, not shapes — a change that makes the grid
slower or unwindows it fails them.

MSRV is **1.80**, enforced by clippy. `std::iter::repeat_n` and friends are too new.

**`Cargo.toml` optimises dependencies *and* `phototools-core` in the dev profile.** Without it a
36 MP TIFF took 7 s to convert instead of 0.26 s — and optimising dependencies alone was not
enough, because `fast_image_resize` is generic and its kernels are monomorphised into `core`. Any
performance figure taken without that section is meaningless.

## Shape

One Rust library, two binaries, two front ends over shared components.

```
crates/core        all functionality — no web framework, no UI, no platform assumptions
crates/server      axum, on the NAS in Docker. Owns the ledger and Google credentials.
                   Also serves the built web front end from WEB_ROOT (§2.2).
crates/desktop     Tauri v2 on macOS. Reads the card, processes locally, hands off.

frontend/shared    the API client (compiled) and the shared views (source)
frontend/web       served by the server; archive tools and publishing
frontend/desktop   inside Tauri; card review and ingest
```

`core` modules map to the specification's §2.5: `media` (the only module permitted to touch image
bytes), `tools` (F1–F9), `ingest` (F10–F14, F16), `publish` (F15), `ledger`, `jobs`, `config`.

`tools::geotag` is the one module with no F-number: the specification does not mention GPS at all,
so inventing one would misrepresent it. See [`docs/geotag-plan.md`](docs/geotag-plan.md).

### The front-end boundary

Read [`frontend/shared/src/ui/README.md`](frontend/shared/src/ui/README.md) before touching a view.
In short:

- `shared/src/ui/views/` — **rendered twice**. May use only `ApiClient` methods *both* transports
  implement.
- `shared/src/ui/components/` — a **library**. Transport-free: props in, events out, never
  `@host/api`. This is what makes them measurable on their own.
  `FolderPicker`, `PathField` and `PathListField` are the path controls every tool uses: they take
  `roots` and a `list` function as props rather than reaching for a client, which is why they work
  over HTTP, over Tauri and in a harness. `useRoots` is the view-side composable that supplies
  them.
- `web/src/views/` and `desktop/src/views/` — screens that genuinely belong to one application.
  `Publish` is web-only (the Google refresh token lives on one machine); `Ingest` is desktop-only
  (the card reader is on the Mac).

A capability that exists in the type system and fails at runtime is worse than one the type system
never offered. Do not add methods to `ApiClient` that one transport has to throw for.

## The design language

Both front ends are styled to a retro-brutalist terminal aesthetic. It is not decoration: several
of its rules will be broken by accident by anyone who does not know them.

`frontend/shared/src/ui/styles/` holds it, behind the single `style.css` both applications import:

| File | Owns |
|---|---|
| `tokens.css` | Every colour, font, space, radius, duration and z-index. **Components reference tokens, never a raw hex.** |
| `fonts.css` | VT323, Share Tech Mono, IBM Plex Mono, Orbitron — **self-hosted**, 88 KB of woff2 |
| `base.css` | Document surface, scanlines, CRT power-on, blink cursor, corner brackets |
| `components.css` | Buttons, terminal inputs, field labels, badges, directory listings, breadcrumbs |

The rules that matter:

- **Dark is canonical**, on bare `:root` rather than behind a preference query. Light is opt-in
  under `[data-theme="light"]` and is never the default.
- **Border-radius above 2px is prohibited.** Depth comes from borders and luminance; there are no
  drop shadows, and the only "shadow" is a phosphor or amber glow.
- **Fonts are self-hosted, not linked.** A CDN stylesheet falls back to system monospace whenever
  the machine is offline — which for the desktop application is exactly when somebody is travelling
  with a card reader (MV-7.3 expects it to work with the server off).
- **No glow on body text or small type.** It is for headings at display size, active navigation and
  status only; at chip size it smears the glyph that carries the meaning.
- **Form controls stay at 16px**, the one deliberate departure from the type scale: iOS zooms the
  viewport when a focused field is smaller, and `check:layout` asserts every control clears 40px.
- Scanlines are **one** `body::after` for the whole application, never duplicated per component.

## Conventions that are load-bearing

- **A dry run is mandatory before publishing** (§9.2 rule 3), checked against the **database**, not a
  flag in memory. The Google Photos API cannot delete, so a safeguard a restart forgets is not a
  safeguard.
- **`plan_publish` computes a plan; `dry_run` computes it and records that somebody looked.** Using
  the latter to build a plan inside a publish would let every publish satisfy its own precondition.
- **Never hold the shared `Arc<Mutex<Ledger>>` across work that reports progress.** `SinkProgress`
  locks the same non-reentrant mutex, so it deadlocks. Long jobs open their own `Ledger`; `open` sets
  a busy timeout for exactly that.
- **The deduplication key is the *source* hash** — the file on the card, which the camera wrote and
  nothing rewrites. The derived hash verifies transfers. Conflating them breaks one of the two jobs.
- **Tests assert the claim, not a proxy for it.** Where a requirement is about ordering, a call is
  counted; where it is about a wait, the requested duration is recorded rather than slept.

## Commit and branch

- One branch per phase while phases were being built. Phase 14 closed the last of them, and work
  since has gone straight to `main` at the user's instruction.
- Imperative subject under 72 characters; the body explains **why**, referencing the requirement
  (`F14`, `G6`, `§9.2`).
- Do not put a model identifier in commits, PR bodies or code comments.

## House style

Comments and documents explain **why**, not what. If a decision has a reason a reader would not
guess — a threshold, an ordering, a deviation from the obvious — it is written down where the code
is. Test names are sentences: `a_truncated_staged_file_is_recopied_rather_than_failed`.

When something cannot be verified, it is reported as unverified. §9.2 invariant 6 is the rule for the
software and the same rule holds for what gets written in a report.
