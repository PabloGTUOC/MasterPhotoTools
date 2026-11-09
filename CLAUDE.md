# PhotoTools — working notes for Claude Code

Ingest photographs from a camera SD card, validate and prepare them, publish them to Google Photos,
and maintain a photo library on a NAS.

## Where the current work is

**Phases 0–14 are built and pass their gates.** Every phase of the build plan is closed.

**The editor has left for an application of its own, Rollchrome** — a desktop-only film
emulator, in the same repository (RC-1 to RC-3, 2026-11-06). [`docs/rollchrome-plan.md`](docs/rollchrome-plan.md)
has the decisions and the steps; RC-1 to RC-3 are done, and Develop's first step,
[DV-1](docs/briefs/DV-1.md), the neutral RAW development, is done, and so are DV-2a
and DV-2b: Develop's function is complete, DV-3 added twelve built-in looks fitted from chart photographs, and RP-1 gave it its screen. The state
before the split is frozen on the **`FullEdition`** branch.

The active job is **using the application on a Mac and fixing what that finds.** Two sessions of
working through the tabs with real photographs turned up sixteen defects that no test had caught —
a rename that renamed the folder instead of the photographs, a metadata reader that silently
returned nothing for camera TIFFs, tools that reported success having done nothing. Most shared one
cause: **the tools had only ever been exercised with typed file paths, and the folder pickers made
pointing at a folder the normal gesture.**

The numbered checks in [`manual-verification.md`](docs/manual-verification.md) remain the
structured half of that work — 96 for PhotoTools, eight done, and 60 for Rollchrome in
[`docs/rollchrome/manual-verification.md`](docs/rollchrome/manual-verification.md), same ids.

| Read this | For |
|---|---|
| [`docs/testing.md`](docs/testing.md) | Setting up on the Mac, and the order to work through the checks |
| [`docs/manual-verification.md`](docs/manual-verification.md) | The checks, each with a stable id like `MV-8.3` |
| [`docs/known-gaps.md`](docs/known-gaps.md) | What is open **in the code**, as opposed to awaiting a human |
| [`docs/geotag-plan.md`](docs/geotag-plan.md) | The Geotag tab: joining photographs to a phone's GPS track. Built; MV-15 awaits a Mac |
| [`docs/owntracks-plan.md`](docs/owntracks-plan.md) | The phone reporting its own positions to the server, rolled into a track each day. Built; MV-19 awaits a phone |
| [`docs/timeline-sync-plan.md`](docs/timeline-sync-plan.md) | Keeping the Mac's timeline and the NAS's the same. Built; MV-18 awaits both machines. **Geopositions only** |
| [`docs/timeline-plan.md`](docs/timeline-plan.md) | The Timeline tab: the track library on a map, and points placed by hand. Stage A built; MV-17 awaits a browser. Google's location history is Stages B–C, not built |
| [`docs/publish-folder-plan.md`](docs/publish-folder-plan.md) | Publishing a folder rather than a card session, so the tools have somewhere to run. Built; MV-16 awaits a NAS |
| [`docs/workflow-plan.md`](docs/workflow-plan.md) | Cutting the two screens down to the workflow: Ingest reports and copies, Publish is the folder. Built; WF-5 awaits MV-16 |
| [`docs/rollchrome/parity-plan.md`](docs/rollchrome/parity-plan.md) | Rollchrome towards replacing Lightroom: three tabs (Develop, Edit, Batch) and the small and medium gaps, RP-1 to RP-13. RP-1 to RP-13, RP-2b, RP-11b and RP-11c done (MV-24 to MV-36 await real files, models and an MCP client) |
| [`docs/rollchrome-plan.md`](docs/rollchrome-plan.md) | Splitting Edit and Batch Grade out into Rollchrome, a desktop-only film emulator. RC-1 to RC-3 done; Develop next |
| [`docs/develop-plan.md`](docs/develop-plan.md) | Rollchrome's third screen, Develop: RAW files to finished JPEG or TIFF, a film look each. DV-1 to DV-3 done: the function and twelve built-in looks; the screen came with RP-1 (MV-23 and MV-24 await real files) |
| [`docs/rollchrome/edit-plan.md`](docs/rollchrome/edit-plan.md) | Rollchrome's Edit and Batch Grade screens: non-destructive exposure adjustments and 3D LUT grading. Rounds 1 and 2 built (ED-1–ED-18); MV-20 and MV-21 await a Mac. Round 3, masks (ED-19–ED-24), built; MV-20 to MV-22 await the Mac Studio |
| [`docs/deployment.md`](docs/deployment.md) | Deploying the server and the desktop app, and every environment variable |
| [`docs/phase-reports/`](docs/phase-reports/) | One report per phase: what was delivered, what deviated, and why |

The user may name items directly — *"do MV-8.6"*, *"MV-11.2 came back at 380 of 400, work out
why"*. Look the item up before acting; each says what passing looks like.

## How work is done: architect, implementer, reviewer

**Claude Code is the architect and the reviewer. agy — the Gemini agent — is the implementer.** The
owner runs both in Orca. Claude does not write the implementation unless the owner asks it to: it
decides what to build, writes that down so another agent can build it without this conversation,
and then checks what came back. Claude's own work is briefs, plans, reviews and the documents in
`docs/`.

The unit of work is a **brief**: one file per step in [`docs/briefs/`](docs/briefs/), named for the
step it implements (`RC-1.md`), written from [`TEMPLATE.md`](docs/briefs/TEMPLATE.md). Its three
sections are filled in turn:

| Section | Written by | Holds |
|---|---|---|
| **Brief** | Claude | The goal and why; the files to change and the ones not to touch; acceptance criteria someone else can check, with expected numbers; the exact gate commands. **Self-contained** — agy has none of Claude's context, so anything not written there does not exist for it. |
| **Report** | agy | What was done; what deviated from the brief and why; every gate's **actual output**, counts not "passed"; anything left unverified. |
| **Review** | Claude | **Approved**, or **Changes requested** as a numbered list agy can act on. |

brief → implement → report → review → *(changes requested → implement → report → review)* →
**approved** → merged to `main`.

### Reviewing

The report is a claim; the review checks it. **Re-run the gates on the branch rather than reading
the report's account of them**, then look for what an implementer under pressure to make things
pass tends to do:

- **Weakened tests (G7).** A changed assertion, a loosened timing budget, a new `#[ignore]`, a
  deleted or renamed test, a count below the brief's expected number. Compare counts before and
  after: every test is accounted for, or the work is not done.
- **Scope beyond the brief (G11).** Refactors nobody asked for, renamed things, a new dependency,
  a "while I was here" fix. Useful ideas go in the report, not the code.
- **The ground rules.** Logic in a binary crate (G1); `core` failing alone (G2); exiftool reading
  metadata (G3); a dependency without its reason recorded (G8); `todo!()`, a bare `unwrap()` or a
  swallowed error on a shipped path (G10).
- **Claims without evidence.** "All tests pass" without the numbers is not a report.
- **Documents.** The plan's status line, `CLAUDE.md` and the docs describe what now exists.

A review that finds nothing should say what it checked. *Approved* means the reviewer ran the gates
and read the diff, not that the report sounded convincing.

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
cargo test --workspace          # 1281 passing (1 ignored)
cargo test -p phototools-core   # 714 passing — G2
cargo test -p rollchrome-core   # 436 passing — G2
cargo test -p phototools-desktop   # 26 passing
cargo test -p rollchrome-desktop   # 26 passing
cargo test -p rollchrome-cli       # 8 passing
```

Front ends:

```sh
npm --prefix frontend/shared run build     # the shared client, consumed as compiled output

npm --prefix frontend/web run typecheck
npm --prefix frontend/web run build
npm --prefix frontend/web run check:transport   # no view reaches the network directly
npm --prefix frontend/web run check:layout      # 12 routes clean at 390 px
npm --prefix frontend/web run check:ingest      # Phase 13 acceptance, measured in a browser

npm --prefix frontend/desktop run typecheck
npm --prefix frontend/desktop run build
npm --prefix frontend/desktop run check:transport

npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport   # and no HTTP client
npm --prefix frontend/rollchrome run check:edit    # ED-7 acceptance & render timing budget, measured in a browser
npm --prefix frontend/rollchrome run check:bulk-lut # ED-8 acceptance & lock discipline, measured in a browser
npm --prefix frontend/rollchrome run check:develop # RP-1 Develop acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:viewing # RP-3 viewing acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:colour  # RP-4 colour tools acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:history # RP-5 history & snapshots acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:versions # RP-6 virtual copies acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:detail   # RP-7 detail acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:export   # RP-9 export acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:point    # RP-10 point colour acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:masks    # RP-11 masks acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:local-detail # RP-11b local detail acceptance, measured in a browser
npm --prefix frontend/rollchrome run check:object   # RP-11c object mask acceptance, measured in a browser
```

`check:layout` and `check:ingest` drive Chromium and write screenshots to
`frontend/web/layout-proof/`; `check:edit`, `check:bulk-lut`, `check:develop`, `check:viewing`, `check:colour`, `check:history`, `check:versions`, `check:detail`, `check:export`, `check:point`, `check:masks`, `check:local-detail` and `check:object` drive Chromium and write screenshots to
`frontend/rollchrome/layout-proof/`. They assert **numbers**, not shapes — a change that makes the grid
or preview slower or unwindows it fails them.

MSRV is **1.88**, enforced by clippy (raised from 1.80 by ED-23: ONNX Runtime through `ort` 2.0 needs it).

**The toolchain is pinned to 1.99.0** in `rust-toolchain.toml`, so CI, the Mac and any other machine run the
same compiler and the same lints. It used to say `stable`, and a Rust release once failed `main` with no code
changed. Moving to a newer release is a deliberate commit that fixes whatever new lints it brings.

**`Cargo.toml` optimises dependencies *and* `phototools-core` in the dev profile.** Without it a
36 MP TIFF took 7 s to convert instead of 0.26 s — and optimising dependencies alone was not
enough, because `fast_image_resize` is generic and its kernels are monomorphised into `core`. Any
performance figure taken without that section is meaningless.

## Shape

Two applications in one workspace. **PhotoTools**: one Rust library, two binaries, two front ends
over shared components. **Rollchrome**, the editor: one library on top of PhotoTools', two binaries
(the Tauri desktop app and `rollchrome-cli`), one front end.

```
crates/core               PhotoTools' functionality — no web framework, no UI, no platform assumptions
crates/server             axum, on the NAS in Docker. Owns the ledger and Google credentials.
                          Also serves the built web front end from WEB_ROOT (§2.2).
crates/desktop            Tauri v2 on macOS. Reads the card, processes locally, hands off.

crates/rollchrome-core    the editor: adjustment pipeline, LUTs, masks, presets, batch grading, automation.
                          Depends on core for decoding, RAW and metadata; core never depends on it.
crates/rollchrome-desktop Tauri v2 on macOS. Rollchrome's commands and preview session.
crates/rollchrome-cli     command-line interface and stdio MCP server over rollchrome-core.

frontend/shared           the API client (compiled), the shared views (source), the design language
frontend/web              served by the server; archive tools and publishing
frontend/desktop          inside Tauri; card review and ingest
frontend/rollchrome       inside Tauri; Develop, Edit and Batch. Uses shared components and styles,
                          never the HTTP client
```

The one contract between the two is the **`.photoedit` sidecar's name**
(`phototools_core::sidecar`): a photograph edited in Rollchrome and renamed in PhotoTools keeps its
edits. Rollchrome reads PhotoTools' configuration file and keeps its jobs in a ledger of its own
(`docs/briefs/RC-2.md`, decisions A and B).

`core` modules map to the specification's §2.5: `media` (the only module permitted to touch image
bytes), `tools` (F1–F9), `ingest` (F10–F14, F16), `publish` (F15), `ledger`, `jobs`, `config`.

Two things sit outside that map, both deliberately and both with no F-number, because inventing
one would make the code look as though the specification had asked for something it never did:

- **`tools::geotag`** — the specification does not mention GPS at all
  ([plan](docs/geotag-plan.md)), and `tools::geotag::place` puts points on the timeline that no
  device recorded, which is why a fix carries whether it was **recorded**, **placed** or
  **inferred** ([plan](docs/timeline-plan.md)).
- **`publish::folder`** and **`ingest::deliver`** — publishing a folder, and copying a card's
  passing frames to one. §6.3 publishes a handed-over session
  ([plan](docs/publish-folder-plan.md)).

`ingest::handoff`, `ingest::staging` and `ingest::remediation` are **still present and unused by any
screen**. They are withdrawn at WF-5, once MV-16 confirms the replacement — not before.

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

- **A brief is committed to `main`; its implementation is not.** agy works on a branch named for
  the brief (`rc-1`), cut from `main` once the brief is there, and the branch reaches `main` only
  after the review says **Approved** — the review needs a diff to review, and `main` stays green.
  Briefs, plans and other documents the architect writes go straight to `main`.
- Before this workflow, phases had a branch each, and after Phase 14 work went straight to `main`
  at the owner's instruction.
- Imperative subject under 72 characters; the body explains **why**, referencing the requirement
  (`F14`, `G6`, `§9.2`).
- Do not put a model identifier in commits, PR bodies or code comments.

## House style

Comments and documents explain **why**, not what. If a decision has a reason a reader would not
guess — a threshold, an ordering, a deviation from the obvious — it is written down where the code
is. Test names are sentences: `a_truncated_staged_file_is_recopied_rather_than_failed`.

When something cannot be verified, it is reported as unverified. §9.2 invariant 6 is the rule for the
software and the same rule holds for what gets written in a report.
