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
  checks what is on it, and copies the frames that passed to a folder where the
  tools can work on them. For now it also carries **Edit** and **Batch Grade**,
  which are leaving for an application of their own — see
  [Next: Rollchrome](#next-rollchrome).

Two front ends over a shared component library: a **web UI** served by the server
for archive work from a phone, and a **desktop UI** inside the Tauri application
for card review and ingest.

The core is a library rather than a service so that a card is processed on the
machine it is plugged into. A 400-frame card is roughly 17 GB, and nothing that
large needs to cross the network.

## The workflow

**Ingest** reads a card, checks each frame for size, resolution, date and
location, and says what needs doing and which tab does it. Then it copies the
frames that passed to a folder you choose.

**The tools** work on that folder, and the tabs are numbered in the order the
work is done: dates, then names, then the timeline and geotagging, then TIFF and
RAW conversion, then half-frame splitting, then borders, with a contact sheet and
a general-purpose transform after the chain. Step 01 is Ingest and lives on the
Mac; the last, step 12, is Publish and lives on the web, so neither application
shows the whole run.

**Publish** takes one designated folder. Copy photographs into it, review what
would go, and it uploads them to Google Photos and empties the folder of
everything Google confirmed receiving.

This is not the pipeline the specification describes, which publishes a card
session handed from the desktop to the server. The reasons are in
[`docs/publish-folder-plan.md`](docs/publish-folder-plan.md) and
[`docs/workflow-plan.md`](docs/workflow-plan.md); the short version is that
every tool sat beside that pipeline rather than on it, and the Google Photos API
cannot delete what it creates.

Authentication is Firebase. Google Photos uses its own OAuth flow; the two are
separate systems.

## Status

**Phases 0–14 are built.** Every phase of the build plan is closed. The work since has gone beyond
the specification, each piece recorded in a plan of its own rather than folded in quietly:

- a **Geotag** tab, placing photographs from a phone's GPS track by matching on time
  ([plan](docs/geotag-plan.md));
- a **Timeline**: the track library on a map, with points placed by hand for days nothing recorded
  ([plan](docs/timeline-plan.md)); the phone **reporting its own positions** to the server through
  OwnTracks ([plan](docs/owntracks-plan.md)); and the Mac and the NAS **keeping the same timeline**
  ([plan](docs/timeline-sync-plan.md));
- **publishing a folder** instead of a card session, so the tools have somewhere to run
  ([plan](docs/publish-folder-plan.md));
- **cutting the two screens down to the workflow** above ([plan](docs/workflow-plan.md));
- an **editor** on the desktop: exposure and colour, curves, grading, looks, 3D LUTs applied to a
  whole folder, and masks, automatic subject and sky masks included ([plan](docs/edit-plan.md)).

What each contradicts in the specification is in
[`docs/known-gaps.md`](docs/known-gaps.md); `SPECIFICATION.md` itself is not edited.

**1,011 tests across the workspace, 901 of them in `phototools-core` with no binary crate present,
all passing in CI.** The Rust toolchain is pinned, so CI and every machine run the same compiler and
lints. Both front ends typecheck and build; the web UI's layout and the ingest grid's performance are
measured in a real browser rather than asserted. The server image builds and the container passes
its health check — deploying it to the NAS is still a human step.

**The tests were never the hard part.** Two sessions of actually using the application on a Mac,
with real photographs, found sixteen defects none of them had caught: a rename that renamed the
folder rather than the photographs inside it, a metadata reader that silently reported no date for
camera TIFFs that plainly carried one, several tools that reported success having done nothing at
all. Most had a single cause — every tool had only ever been exercised with typed file paths, and
adding folder pickers made pointing at a folder the ordinary thing to do.

What is left is more of that, plus the structured half: **121 numbered checks**, eight of them
done, needing a Mac, a
camera, a NAS, a Google account or somebody's judgement about how a photograph looks. Start at
[`docs/testing.md`](docs/testing.md).

**The new road has now carried photographs to Google Photos** — with the date the camera recorded
and the position the Geotag tab wrote, both confirmed in Google Photos rather than inferred from
our own logs (MV-16.3). The folder emptied afterwards and the folder it was copied from did not
(MV-16.4), a file Google rejected stayed behind while the rest went (MV-16.5), and a tool pointed
at the publishing folder is refused (MV-16.7). What still needs a card in a reader is MV-16.1,
MV-16.2 and the whole-roll run, MV-16.8.

## Next: Rollchrome

The editor is leaving. **Rollchrome**, a film emulator, desktop only with an iPad kept possible,
takes Edit and Batch Grade and becomes an application of its own, in this repository so that the
RAW decoding and metadata handling it shares with PhotoTools exist once. PhotoTools keeps
everything else. The last state with both in one application is frozen on the **`FullEdition`**
branch. The decisions, the boundary as measured and the steps are in
[`docs/rollchrome-plan.md`](docs/rollchrome-plan.md).

## How work is done

Two agents, run by the owner in Orca. **Claude Code is the architect and the reviewer**: it writes a
brief for each step in [`docs/briefs/`](docs/briefs/). **agy, the Gemini agent, implements it** on a
branch of its own and reports back with the gates' actual results. Claude re-runs those gates and
reviews the diff, and only an approved branch reaches `main`. The rules, and what a review looks
for, are in [`CLAUDE.md`](CLAUDE.md).

## Documents

| File | Purpose |
|---|---|
| [`SPECIFICATION.md`](SPECIFICATION.md) | **What** the system does. Functional requirements F1–F18, architecture, API, data model, non-functional requirements. The authority on behaviour. |
| [`BUILDPLAN.md`](BUILDPLAN.md) | **How** it gets built. Fifteen phases with tasks, acceptance criteria and ground rules. |
| [`CLAUDE.md`](CLAUDE.md) | Working notes for an agent session: how the architect, implementer and reviewer divide the work, the ground rules, the gate commands, and where the current work is. |
| [`docs/testing.md`](docs/testing.md) | **Start here on a Mac.** Setup, the gate commands, and the order to verify in. |
| [`docs/manual-verification.md`](docs/manual-verification.md) | The checks themselves, numbered and tickable. |
| [`docs/deployment.md`](docs/deployment.md) | Deploying the server, installing the desktop application, Firebase and Google OAuth setup, every environment variable. |
| [`docs/known-gaps.md`](docs/known-gaps.md) | What is open in the code — missed criteria, awkward seams, places the specification is incomplete. |
| [`docs/phase-reports/`](docs/phase-reports/) | One report per phase, and one per piece of work since: delivered, deviations, measurements, gates. |
| [`docs/rollchrome-plan.md`](docs/rollchrome-plan.md) | **The next work**: splitting the editor out into Rollchrome. |
| [`docs/briefs/`](docs/briefs/) | One file per step being built: the architect's brief, the implementer's report, the review. |
| [`docs/geotag-plan.md`](docs/geotag-plan.md) · [`timeline-plan.md`](docs/timeline-plan.md) · [`owntracks-plan.md`](docs/owntracks-plan.md) · [`timeline-sync-plan.md`](docs/timeline-sync-plan.md) · [`publish-folder-plan.md`](docs/publish-folder-plan.md) · [`workflow-plan.md`](docs/workflow-plan.md) · [`edit-plan.md`](docs/edit-plan.md) | The plans written since the phases closed. Each was written before its code and corrected where the build diverged. |

Start with the specification. Build from the plan.
