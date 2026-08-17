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

Specification only. No implementation yet.

## Documents

| File | Purpose |
|---|---|
| [`SPECIFICATION.md`](SPECIFICATION.md) | **What** the system does. Functional requirements F1–F18, architecture, API, data model, non-functional requirements. The authority on behaviour. |
| [`BUILD-PLAN.md`](BUILD-PLAN.md) | **How** it gets built. Fifteen phases with tasks, acceptance criteria and ground rules — written to be handed to a development agent. |

Start with the specification. Build from the plan.
