# PhotoTools

Ingest photographs from a camera SD card, validate and prepare them, publish
them to Google Photos — and maintain a photo library on a NAS from anywhere.

## Shape

One Rust library, compiled into two applications:

- **`phototools-core`** — a library holding all the functionality. Image
  handling, metadata, card scanning, validation, the Google Photos client and
  persistence. No web framework, no UI.
- **`phototools-server`** — runs on the NAS in Docker. Backs the web front end,
  owns the upload ledger and Google credentials, and performs archive work
  against the library.
- **`phototools-desktop`** — a macOS application (Tauri v2). Detects an SD card,
  reads and processes it locally, and hands the results to the server to publish.

Two front ends over a shared component library: a **web UI** served by the server
for archive work from a phone, and a **desktop UI** inside the Tauri application
for card review and ingest.

The core is a library rather than a service so that a card is processed on the
machine it is plugged into. A 400-frame card is roughly 17 GB; only the finished
derivatives — 1–3 GB — ever cross the network.

Authentication is Firebase. Google Photos uses its own OAuth flow; the two are
separate systems.

## Status

**Phases 0–14 are built.** Every phase of the build plan is closed.

504 tests across the workspace, 423 of them in `phototools-core` with no binary crate present.
Both front ends typecheck and build; the web UI's layout and the ingest grid's performance are
measured in a real browser rather than asserted. The server image builds and the container passes
its health check — deploying it to the NAS is still a human step.

**The tests were never the hard part.** Two sessions of actually using the application on a Mac,
with real photographs, found sixteen defects none of them had caught: a rename that renamed the
folder rather than the photographs inside it, a metadata reader that silently reported no date for
camera TIFFs that plainly carried one, several tools that reported success having done nothing at
all. Most had a single cause — every tool had only ever been exercised with typed file paths, and
adding folder pickers made pointing at a folder the ordinary thing to do.

What is left is more of that, plus the structured half: **51 numbered checks** needing a Mac, a
camera, a NAS, a Google account or somebody's judgement about how a photograph looks. Start at
[`docs/testing.md`](docs/testing.md).

## Documents

| File | Purpose |
|---|---|
| [`SPECIFICATION.md`](SPECIFICATION.md) | **What** the system does. Functional requirements F1–F18, architecture, API, data model, non-functional requirements. The authority on behaviour. |
| [`BUILDPLAN.md`](BUILDPLAN.md) | **How** it gets built. Fifteen phases with tasks, acceptance criteria and ground rules. |
| [`CLAUDE.md`](CLAUDE.md) | Working notes for an agent session: ground rules, gate commands, and where the current work is. |
| [`docs/testing.md`](docs/testing.md) | **Start here on a Mac.** Setup, the gate commands, and the order to verify in. |
| [`docs/manual-verification.md`](docs/manual-verification.md) | The checks themselves, numbered and tickable. |
| [`docs/deployment.md`](docs/deployment.md) | Deploying the server, installing the desktop application, Firebase and Google OAuth setup, every environment variable. |
| [`docs/known-gaps.md`](docs/known-gaps.md) | What is open in the code — missed criteria, awkward seams, places the specification is incomplete. |
| [`docs/phase-reports/`](docs/phase-reports/) | One report per phase: delivered, deviations, measurements, gates. |

Start with the specification. Build from the plan.
