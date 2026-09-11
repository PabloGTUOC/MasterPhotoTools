# Two screens — development plan

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

### WF-2 — The status list
The summary above, each line naming the tab that fixes it, computed from the verdicts already
returned.
**Done when** the numbers add up to the frame count and each line's count matches the table.

### WF-3 — RAW to JPEG as a tool
Per the decision above: a route, a view, and the existing `deriveRaw` behind it, pointed at a
folder rather than a card.
**Done when** a folder of RAW files produces JPEGs beside them, and `check:layout` passes with the
new route.

### WF-4 — Publish is the folder, and only the folder
Remove the session panel and its logic. The folder panel becomes the screen.
**Done when** the tab reads the folder on opening, copies in, dry runs, publishes and empties, with
no session anywhere.

### WF-5 — Retire the handoff, and F13's bulk remediation
The desktop commands, the server routes, `ingest::handoff`, `ingest::staging`, the `sessions`
table's card columns and the publish-by-session path — **and** `BulkActions.vue`, `remediate` and
`ingest::remediation`, which WF-1 left with no caller.

Both are specification features being withdrawn, not dead code being swept up, so they go together
in one change that says so.

**Not before MV-16 passes.** Deleting a working subsystem before its replacement has published a
real photograph is the wrong order; this step is written down so the intermediate state is
deliberate. It also retires MV-11 and the session half of MV-12.

### WF-6 — Documents
`known-gaps.md`, a phase report, MV-16 amended for the changed screens, and the test counts.

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
