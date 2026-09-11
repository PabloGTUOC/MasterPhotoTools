# Two screens — development plan

> **Built**, except WF-5. WF-1 to WF-4 and WF-6 are done and the gates pass; WF-5 withdraws two
> specification features and is gated on MV-16 confirming folder publishing against real
> photographs. The report is [`phase-reports/workflow.md`](phase-reports/workflow.md).

Cut the Ingest and Publish screens down to what the workflow actually is, and remove what belonged
to the road being retired.

Steps have stable ids (`WF-3`), the way [`geotag-plan.md`](geotag-plan.md) and
[`publish-folder-plan.md`](publish-folder-plan.md) number theirs. This supersedes the screen parts
of the publish-folder plan (PB-7, PB-8), which put the new road *beside* the old one; the decision
now is to remove the old one from both screens.

---

## The workflow, in the user's terms

> **Ingest** reads the card and checks it for sanity — size, resolution, date, geolocation — gives
> me the status so I know what I need to do, and offers to move it to a folder for further working.
>
> **Publish** offers to copy content into the publishing folder, or if something is already there,
> copies it into Google Photos and then deletes the content.

Two screens, one job each. Everything else on them today belongs to the card-handoff road, which
folder publishing replaced.

| | Today | After |
|---|---|---|
| **Ingest** | scan, validate, bulk remediation, RAW derivation, stage, hand a session to the server, then a "the server has taken over" screen | scan, check, **say what needs doing**, copy to a folder |
| **Publish** | a session id typed in, plus the publishing folder | the publishing folder, and nothing else |

## What comes off, and where it went

**The handoff.** Staging, the manifest, the arrival report, the session, and the "server has taken
over" stage. Ingest ends at a folder now; there is nothing to hand over.

**The session panel on Publish.** Two roads on one screen was the right *transitional* answer and
the wrong one to live with — the screen asked for an id the workflow never produces.

**Bulk remediation (F13).** Ingest reports what is wrong; the tools fix it. A card screen that
both diagnoses and repairs duplicates the Dates tab, and now the Geotag tab as well. The status
list points at the tab that does each job, which is what *"so I know what I need to do"* asks for.

**RAW derivation (F14) is the one that cannot simply come off** — see the decision below.

## The decision this plan needs

**Where does RAW to JPEG live?** A RAW-only shot has no JPEG to publish, and F14's derivation is
reachable from exactly one place: the Derive button on the Ingest screen. The TIFF tab converts
scanner output, not RAW. Three options:

| | |
|---|---|
| **A. Its own tool tab** *(recommended)* | "RAW to JPEG", beside TIFF to JPEG. Point it at a folder, it derives what it finds. Consistent with every other tool, and it works on the folder Ingest copied to — which is where the files are by then. |
| **B. Stays on Ingest** | One button survives the cut. Smallest change, but it makes Ingest a screen that both reports and acts, which is the thing being removed. |
| **C. Folded into the copy** | Ingest derives as it copies. Fewest steps, but it makes a copy operation write new files, and hides a slow, lossy decision inside a fast, safe one. |

**A** unless you say otherwise. It is one route and one view reusing `ImageTool`'s shape, and it
keeps the rule that Ingest reports and tools act.

## The status Ingest gives

The four checks already produce per-frame verdicts. What is missing is the sentence that turns them
into work:

```
40 frames. 38 ready.
  2 have no capture date          → Dates tab
 12 have no location              → Geotag tab
  3 are RAW with no JPEG          → RAW to JPEG
  1 is larger than the size limit → it will be resized when published
```

Each line names the tab that fixes it. The per-frame table stays, because a count tells you what to
do and a table tells you which frame — and `ShotGrid` already renders it, with the `location` check
appearing unaided.

**Ready** means no check *failed*. A warning — the odd frame with no coordinates on a card where
others have them — is worth seeing and does not hold the card up.

## Copying is still copying

"Move it to a folder" means **copy, verify by hash, and leave the card alone (G5)**. Nothing in
this plan deletes from a card. The destination is refused if it is inside the card, checked before
a byte moves — already built (PB-3), unchanged here.

---

## Steps

### WF-1 — Ingest reports, and copies ✔
Removed: the handoff and its button, the staging field, the `BulkActions` panel, the handed-over
stage and its styles, and the session the desktop used to report.

**RAW derivation and its output field stayed**, against the letter of this step. Removing them here
would leave a RAW-only shot with nowhere in the whole application to get a JPEG until WF-3 lands,
and the application is being used between commits. They go when the tab that replaces them exists.

The clock-offset notice now points at the Dates tab rather than at the bulk shift it used to offer.

**Orphaned by this step, and deliberately not deleted yet:** `BulkActions.vue` and the `remediate`
client method, route and command are now used by no view. F13 is a specification feature, so
removing it is a decision of its own rather than a tidy-up — **folded into WF-5**, which already
retires a subsystem on the same terms. `check:ingest` still passes: it mounts those components in
its own harness and never depended on the screen using them.

### WF-2 — The status list ✔
Built from the verdicts already returned, in the order somebody would act on it. Lines with no
frames behind them are dropped — a list of zeroes reads as work outstanding.

**Ready** counts frames with nothing *failing*, so the location warning does not hold a card up.

One thing worth recording because it would have failed silently: the wire carries
`FailureClass::as_str()` — `date_out_of_range`, not the serde name `date_out_of_range_isolated` —
and `Rule` arrives as its lowercased `Debug`. Both were checked against the transports rather than
assumed; a mismatch would have shown a confident **0** for a line that should have had frames in
it, which is worse than showing nothing.

The RAW line points at *Derive, below* until WF-3 gives it a tab. Pointing at a tab that does not
exist yet would be a promise the screen cannot keep.

### WF-3 — RAW to JPEG as a tool ✔
Option A. A shared view, a route in `sharedToolRoutes` so both applications get the tab, and the
existing `deriveRaw` behind it.

**No new machinery was needed.** `Card::at` accepts any directory — it is what makes "no DCIM
folder; treating it as a plain directory" work on the card screen — so the command already derived
from a folder. The change is a view and a route.

Shared rather than desktop-only because a decode and an encode are something both transports
genuinely do, and `deriveRaw` was already on `ApiClient`.

With the tab in place, the Derive button, its output-folder field and the `derive` function came
off Ingest, and the status line now points at the RAW tab rather than at a button.

**Done:** `check:layout` clean at **11 routes**. One defect fixed on the way — the two ceiling
fields did not line up at 390 px, because the earlier fix for this on the card screen reserved the
*hint*'s line and not the *label*'s, and that screen is never measured at 390 px.

### WF-4 — Publish is the folder, and only the folder ✔
The session panel, the plan-detail panel it rendered, the `sessionId`, `plan`, `reviewed` and
`canPublish` state, the `dryRun`, `publish` and `onSessionChanged` functions, and the two style
rules that framed one road against the other — all gone. With one road left there is nothing to
distinguish it from, so the panel's own heading went too: the screen is the folder.

The `publishDryRun` and `publish` methods stay on the web client. They are the transport for routes
that are still live, and they are withdrawn with those routes in WF-5 rather than half-retired
here.

**Done:** typecheck, build, transport check, and 11 routes clean at 390 px.

### WF-5 — Retire the handoff, and F13's bulk remediation
The desktop commands, the server routes, `ingest::handoff`, `ingest::staging`, the `sessions`
table's card columns and the publish-by-session path — **and** `BulkActions.vue`, `remediate` and
`ingest::remediation`, which WF-1 left with no caller. Also `hand_off_card`, `stage_card`,
`ServerConnection`, `server_status` and the desktop's server-settings commands: nothing on that
machine makes an HTTP request any more.

Both are specification features being withdrawn, not dead code being swept up, so they go together
in one change that says so.

**The desktop's reachability banner went early**, because it was not code being retired but a
warning being given for a dependency that no longer existed — and a false alarm teaches somebody to
ignore the next real one.

**Not before MV-16 passes.** Deleting a working subsystem before its replacement has published a
real photograph is the wrong order; this step is written down so the intermediate state is
deliberate. It also retires MV-11 and the session half of MV-12.

### WF-6 — Documents ✔
`known-gaps.md` records both withdrawn features and why their code is still present;
`phase-reports/workflow.md` reports what was built and the two defects found on the way.

MV-13.1 is rewritten for the screen that now exists. **MV-13.3 and MV-13.4 are retired rather than
done** — their subjects no longer exist, and MV-13.4's question ("is typing a session id between
two screens good enough?") is answered by the removal. MV-11 gains a note saying it covers a road
being retired. MV-12.1 points at the publishing folder.

Test counts are unchanged: no Rust changed in WF-1 to WF-4.

---

## What this costs

**Removing F13's bulk actions removes a good screen.** `BulkActions` groups failures by class and
applies one fix to all of them — a genuinely nice thing that MV-13 covers. The Dates tab can shift
a whole folder, so the capability survives; the *card-shaped* version of it does not. Recorded
rather than quietly dropped.

**MV items change meaning.** MV-11 (handoff) and MV-12's session publishing go with WF-5; MV-13's
bulk-action checks go with WF-1. MV-16 becomes the acceptance for the whole workflow.

**`ShotGrid` and `BulkActions` are measured by `check:ingest`**, which puts 400 shots through them
and asserts numbers. `ShotGrid` stays; if `BulkActions` goes, that gate needs amending rather than
deleting — it is the only performance measurement of the review screen.

## Risks

| | |
|---|---|
| **Deleting the handoff too early** | WF-5 is gated on MV-16, explicitly. |
| **RAW-only shots becoming unpublishable** | The decision above; option A keeps them reachable. |
| **`check:ingest` losing its subject** | Amend it to the new screen rather than dropping a measurement. |
| **The card screen losing its table** | It does not: `ShotGrid` is what makes the status actionable per frame. |
