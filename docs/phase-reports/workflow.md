# Two screens — cutting them down to the workflow

**Status:** WF-1 to WF-4 and WF-6 built, gates pass. WF-5 — withdrawing the handoff and F13's bulk
remediation — is gated on MV-16.

Planned in [`workflow-plan.md`](../workflow-plan.md) before any code was written.

---

## Why

The user described the workflow they actually have:

> Ingest reads the card and checks it for sanity — size, resolution, date, geolocation — gives me
> the status so I know what I need to do, and offers to move it to a folder for further working.
> Publish offers to copy content into the publishing folder, or if something is already there,
> copies it into Google Photos and then deletes the content.

Two screens, one job each. Everything else on them belonged to the card-handoff road that folder
publishing replaced — and a screen that asks for a session id the workflow never produces is a
question with no answer.

## Delivered

| | |
|---|---|
| `Ingest.vue` | Reports and copies. The handoff, the staging field, `BulkActions`, the derive button and the handed-over stage are gone. |
| The status list | What needs doing, how many frames need it, and **which tab does it**. |
| `RawToJpeg.vue` | F14 as a tool tab, in both applications. |
| `Publish.vue` | The publishing folder, and nothing else. |
| `routes.ts` | `/raw-to-jpeg`, giving both applications the tab from one definition. |

## The decisions that shaped it

### Ingest reports; the tools act

The card screen used to diagnose *and* repair. That duplicated the Dates tab and, once geotagging
existed, the Geotag tab as well — and the second copy is always the one that falls behind.

What replaces it is a list that names the tab for each thing:

```
// 40 FRAMES // 38 READY //
   2 have no capture date                             → Dates tab
  12 have no location, where others on this card do   → Geotag tab
   3 are RAW with no JPEG beside them                 → RAW tab
```

**Ready** counts frames with nothing *failing*, so the location warning does not hold a card up —
which is the whole reason that check was built to warn rather than fail.

### RAW to JPEG had to go somewhere before it could come off Ingest

Deriving a JPEG from a RAW-only shot was a button on the card screen and **the only way to do it
anywhere in the application**. Removing it in WF-1 as the plan said would have left that capability
unreachable until WF-3 landed — in an application being used between commits. It stayed one step
longer than the plan allowed and left when its replacement existed.

No new machinery was needed: `Card::at` accepts any directory, so the existing command already
derived from a folder.

### Both roads on one screen was right, briefly

The publish-folder plan put the folder beside the session rather than replacing it, at the user's
choice, so the transition would be honest rather than a retirement by stealth. Seeing it on screen
answered the question the other way: a field asking for something the workflow cannot produce is
not honest, it is confusing. WF-4 removed it.

Recorded because the intermediate state was deliberate and the reversal was evidence, not drift.

### The client keeps what the routes keep

`publishDryRun` and `publish` stay on the web client although no view calls them. They are the
transport for routes that are still live, and WF-5 withdraws them together. A live route with no
client that can reach it is worse than either state.

## Defects found while building this

| | |
|---|---|
| **The ceiling fields did not line up at 390 px** | The earlier fix for this on the card screen reserved the *hint*'s line and not the *label*'s. It never showed there: Ingest is desktop-only and `check:layout` does not measure it. Copying the pattern into a measured screen exposed it immediately. |
| **The status list nearly matched the wrong strings** | The wire carries `FailureClass::as_str()` — `date_out_of_range` — not the serde variant name. Checked against the transports rather than assumed; a mismatch would have shown a confident **0** for frames that were really there. |

## What this cost

**F13's bulk actions were a good screen.** One press fixed every shot sharing a failure. The
capability survives in the Dates and Transform tabs over a folder; the card-shaped version does
not, and MV-13.3 retires with it.

**MV-13.4 is answered by removal.** It asked whether typing a session id from one screen into
another was good enough. It was not.

## Acceptance

Every check here is visual or manual — these are screens. MV-13.1 covers the Ingest flow on a Mac,
MV-16 covers publishing end to end, and `check:layout` holds the arithmetic: **11 routes clean at
390 px**, including the new tab.

## Gates

`fmt`, `clippy -D warnings`, 692 workspace tests, 607 in core — **unchanged, because no Rust
changed**. Both typechecks, both builds, both `check:transport` runs, `check:layout` at 11 routes
and `check:ingest` all pass.

## Not done, deliberately

- **WF-5.** `ingest::handoff`, `ingest::staging`, `ingest::remediation`, `BulkActions.vue`, and the
  routes and commands behind them are still here and still work. They go when MV-16 has confirmed
  the replacement, not before.
- **Nothing has been published through the new screens.** MV-16 remains the acceptance for the
  workflow this change exists to serve.
