# Publishing a folder — development plan

> **Built.** PB-1 to PB-9 are done and the gates pass; eight checks await a NAS and a Google
> account (MV-16). Where the build diverged from this plan the text says so, and the reasons are in
> [`phase-reports/publish-folder.md`](phase-reports/publish-folder.md). The session road is still
> live and is retired separately.

Move publishing off the card-handoff session and onto a folder, so there is somewhere to edit a
photograph between the card and Google Photos.

Steps have stable ids (`PB-4`), the way [`geotag-plan.md`](geotag-plan.md) numbers its steps and
[`manual-verification.md`](manual-verification.md) numbers its checks.

---

## The gap this closes

Today the road to Google runs card → validate → derive → hand off a session → publish that session.
Every tool this application has — geotag, TIFF to JPEG, borders, half-frame split — sits *beside*
that road rather than on it. There is no point in the pipeline where a photograph can be changed
before it is published.

The consequence is not theoretical: **the Google Photos API cannot delete what it creates.**
Publishing first and fixing afterwards is not a recovery, it is a duplicate.

So the road is being re-cut:

| | Today | After |
|---|---|---|
| **Ingest** | the only road to publishing: validate, derive, stage, hand off a session | reads a card (desktop) or a folder (web), checks each frame, **copies what passes to a folder you choose** — and ends there |
| **The tools** | beside the road | on it: they work on that folder |
| **Publish** | takes a handoff session id | copies a chosen folder or files into `Publishing`, or takes what is already there, uploads it and empties it |

## Standing against the specification

**This contradicts the specification rather than merely going beyond it.** §6.3's publish flow, F15
and F16 all assume a session: `plan_publish` takes a manifest, a `SessionPlan` and an arrival
report; `dry_run` records itself against a `session_id`; F16's ledger is keyed on the source hash
carried in a manifest entry.

`SPECIFICATION.md` is not edited (G9). This is recorded in
[`known-gaps.md`](known-gaps.md) under *Places the implementation goes beyond the specification*,
and reported in `phase-reports/` on completion. The decision is the user's, taken deliberately:
the specification describes a pipeline with no editing step, and that is the thing being fixed.

---

## The `Publishing` folder

One folder, configured, that publishing draws from and — **only on success** — empties.

```
/Publishing      # a placeholder. The real NAS path is settled before PB-6.
```

It becomes a field on `Config` beside `roots` and `staging_dir`, so it is never typed:

```rust
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub staging_dir: PathBuf,
    pub publishing_dir: PathBuf,   // new
    pub thresholds: Thresholds,
    pub database: PathBuf,
}
```

Environment variable `PUBLISHING_DIR`, and a key in `config.json`. **Unset means publishing is
refused**, the same way an empty `ROOTS` refuses every path — a destructive default that guesses at
a folder is worse than one that will not start.

### The four rules of the deletion

This is the one irreversible step in the application, and it is irreversible in *both* directions:
Google cannot un-publish, and a deleted file is gone. Each rule exists because of a specific way
this could go wrong.

**1. Only `Publishing` is ever deleted from.** The path is canonicalised and checked to be exactly
the configured folder — not a prefix match on the string typed in, which a symlink or a `..`
defeats. A folder that merely *happens to be called* `Publishing` is refused. This is G6's rule
applied a second time, with a narrower root: G6 asks "is this inside somewhere I may touch?", and
this asks "is this the one folder I may delete from?"

**2. A file is deleted only when Google returned a media item id for it.** Per file, not per job.
"The job finished" is not evidence about any particular photograph — the publish state machine
already tracks each one through `pending → uploaded → created`, and only `created` earns a
deletion. Anything that failed, or whose outcome is unknown because the answer never arrived,
stays where it is. §9.2 invariant 6 in its most literal form: what cannot be verified is not
claimed, and here the claim is destructive.

**3. Everything in `Publishing` is a copy of something else** — so the deletion removes the second
copy and the first is still where you left it. There is no exception, and it is **enforced rather
than trusted**: `Config::resolve_for_create` refuses any tool output path inside the publishing
folder, so borders, conversions and splits cannot write their only copy into the one folder that
gets emptied. The way in is copying, chosen deliberately at the start of a publish, or dropped in
from a file manager.

The one thing this cannot prevent is somebody *moving* files in rather than copying them. That is
why the screen states plainly that everything listed will be deleted after a successful upload.

**4. Nothing is deleted without a dry run.** The list is shown, the upload runs, and the summary
reports exactly which files were removed and which were kept, with the reason. Consistent with
§9.2 rule 3, which already makes a dry run mandatory before publishing.

Empty subdirectories left behind are removed; anything with a file still in it is left alone,
because a file still in it is a file that did not publish.

### Subfolders are included

Everything under `Publishing`, at any depth. The alternative — top-level files only — silently
ignores what somebody put there, and a publish that quietly skips half a folder is worse than one
that uploads more than expected. Deletion is per-file-uploaded regardless of depth, so recursion
does not widen what rule 2 permits.

---

## What "already published" can mean now

F16 keys the ledger on the **source** hash — the bytes the camera wrote, which nothing rewrites.
A folder of processed files has no source hash, and the problem is sharper than it first looks:
**geotagging rewrites the file**, so one photograph has a different hash after it is tagged than
before. Bordering, converting and splitting all do the same.

So folder publishing can only answer *"have I uploaded exactly these bytes before?"* It catches the
common accident — the same folder published twice — and it will **not** recognise one photograph
processed two different ways.

What actually prevents double-publishing is emptying `Publishing` after every successful run: the
folder being empty *is* the record of what has gone. That is a real safeguard and a weaker one than
F16's, and the screen says which it is rather than letting "already published" be read as more than
it means.

The existing `published` table is kept and still keyed on `source_sha256`; folder publishing writes
its own rows with the derived hash and a flag saying which kind of key it is, so the two cannot be
confused by a later reader.

---

## Ingest becomes a check-and-copy

The card scan, the pairing and the validation stay exactly as they are. What changes is what
happens after: instead of staging, a manifest and a handoff, it **copies the frames that pass to a
folder you name** — local when you intend to work on them, the NAS when you do not.

**G5 is unchanged and unchanged-able: the card is never written to.** "Move it off the card" means
copy, verify by hash, and leave the card alone; nothing in this plan deletes from a card.

### A fourth check: location

Alongside `Date`, `Resolution` and `Size`, a `Location` rule reporting whether the frame carries
GPS.

**It warns; it never fails.** Plenty of photographs legitimately have no coordinates, and a check
that failed them would block a card over a non-problem — and F13's bulk actions group by
`FailureClass`, so a new failure class would offer a remediation that does not exist. A warning
tells you which frames want the Geotag tab before you have moved anything, which is the whole
value of knowing.

```rust
pub enum Rule { Date, Resolution, Size, Location }   // Location added
// FailureClass is untouched: a warn carries none.
```

---

## Shape

```
crates/core/src/config.rs                  publishing_dir, and its resolution
crates/core/src/ingest/validation.rs       the Location rule
crates/core/src/ingest/deliver.rs          new: check-and-copy to a chosen folder
crates/core/src/publish/folder.rs          new: plan a folder, publish it, empty it
crates/core/src/publish/publisher.rs       the state machine, reused unchanged
crates/core/src/ledger.rs                  migration 8: folder publish sessions and rows
crates/server/src/api.rs                   routes for folder publishing
crates/desktop/src/commands.rs             commands for check-and-copy
frontend/shared/src/ui/views/Publish.vue   moves from web-only to shared? — see PB-8
```

### What happens to the session road

The desktop→server handoff — manifests, staging, arrival reports, verification by hash, the
`sessions` table — becomes unused the moment Ingest stops handing off. That is a working, tested
subsystem and several MV items (MV-11.x) exercise it.

**It is not deleted in this work.** Two roads to publishing is a bad end state, but deleting the
old one before the new one has met real photographs is worse. The order is: build folder
publishing, verify it (MV-16), then remove the session road in a separate change that also retires
the MV items it covered. Recorded here so that the intermediate state is deliberate rather than an
oversight, and so nobody "tidies" the handoff away halfway through.

---

## Steps

### PB-1 — `publishing_dir` in configuration
The field, `PUBLISHING_DIR`, the `config.json` key, and a `Config::resolve_for_publishing` that
canonicalises a path and admits it **only if it is exactly the configured folder or inside it**.
**Done when** a symlink inside `Publishing` pointing outward is refused, `..` is refused, a
different folder of the same name is refused, and an unset `publishing_dir` refuses everything.

### PB-2 — the Location check ✔
`Rule::Location`, from the GPS fix `read_meta` already returns, carried on `ScannedAsset` as
`has_location`.

**Warns only on the odd one out.** The plan said warn-when-absent; measured against how the rule
would actually read, that is an amber mark on nearly every card this application will see, since
most cameras have no receiver — and a mark on every card is a mark on none. It follows the date
rule's shape instead: a card recording no positions at all *passes* with the absence stated, and a
frame with none on a card where the others have them *warns*, because that one is the odd frame out
and the one that will look wrong in Google Photos beside the rest.

**Done:** a located frame passes; a card with no coordinates anywhere passes and still counts as
clean; the odd frame out warns; nothing ever fails, and no `FailureClass` is added — F13 groups
bulk actions by failure class and no action on a card can add a location. The front end needed no
change: it renders a rule's name from the wire.

### PB-3 — check-and-copy ✔
`ingest::deliver`: take the frames that pass, copy them to a chosen folder, verify each by hash,
report what landed and what did not. Never touches the card.

A sibling of `staging` rather than a reuse of it, for three reasons that turned out to matter:
**the camera's filenames are kept** (a working folder is a place a person opens, and
`a3f91c….jpg` is not a photograph anybody can find), which brings back the collision staging's
content-hash names avoid — so **nothing is ever overwritten**: identical content already there is a
frame already delivered and is skipped, and different content under the same name is reported and
left alone. And **the destination is refused if it is inside the card**, checked on canonicalised
paths before a byte is copied: this is the one operation whose destination somebody types, and the
card is the folder they are looking at.

**Done:** twelve tests, including the card hashed before and after (G5 asserted, not assumed), a
symlink into the card refused, two cards' `IMG_0001.JPG` not overwriting each other, one failure
not abandoning the other frames, and no `.partial-copy` left behind. `deliver_card` on the
desktop copies only the shots that did not fail validation.

### PB-4 — planning a folder publish ✔
`publish::folder`: walk `Publishing`, decide what is publishable, check the byte-hash ledger,
produce a plan. Writes nothing.

**The state machine is reused rather than rebuilt.** `Publisher` reads each file as
`staging_dir.join(file_name)`, so a folder publish is that same publisher with `staging_dir`
pointed at the publishing folder and `file_name` a path relative to it. Retries, resumption and the
dry-run check come along unchanged; a second publisher would have drifted from the first.

**The session id is a fact about the contents, not the folder** — stronger than this plan asked
for. It hashes every file's relative path and content hash in order, which binds §9.2 rule 3's dry
run to exactly what was reviewed: add a file afterwards, or geotag one (which rewrites it), and the
id changes, the recorded dry run no longer matches, and publishing refuses until somebody looks
again. A partial failure leaves the same files in place, so the same id, so a resumed run continues
its own rows rather than doubling them.

Migration 8 adds `published.key_kind` — `source` for F16's rows, `file` for these — so a later
reader cannot mistake "these bytes were uploaded" for "this photograph was published". It also adds
`sessions.folder`, rather than putting a folder path in a column called `card_id`.

**Done:** twelve tests, including both dry-run invalidations, the two key kinds staying distinct,
subfolders keeping their paths, non-photographs reported rather than uploaded, and 51 files needing
two batchCreate calls (§6.1).

### PB-5 — publishing, and emptying ✔
The upload through the existing state machine, then the deletion under the four rules.

`publish_folder` is the *same* `Publisher` with `staging_dir` pointed at the publishing folder and
`key_kind: "file"`. Retries, resumption, rate-limit backoff and the mandatory dry-run check come
along unchanged; the only thing that differs between publishing a card and publishing a folder is
which question the resulting ledger row answers.

Rule 1 is checked twice — once on the folder before anything is examined, and again **per file**,
because a name from the plan is not a location until it is resolved. Rule 2 reads the **publish
row** per file rather than the run's outcome: "the job finished" is not evidence about any
particular photograph. Rule 4 comes free, and is tested rather than assumed — the token provider
panics if it is ever asked, so the refusal demonstrably happens before any network work.

**Done:** eleven tests on the deletion, including a run where everything failed deleting nothing, a
file that arrived after the plan was made surviving, an uploaded-but-unconfirmed file surviving, and
a symlink out of the folder not being followed — the archive it points at is untouched. Emptied
subfolders are tidied away; one still holding a file is not; the publishing folder itself always
survives, because it is configuration rather than content.

### PB-6 — the transports ✔
Four routes — `GET /api/publish/folder`, and `POST` for `fill`, `plan` and `publish` — plus
`fill_publishing` on the desktop. Every path through `Config`; the publishing folder never typed,
only read from configuration.

**Only `fillPublishing` is on `ApiClient`.** Filling the folder is copying, which both machines do.
Uploading is the server's alone — the refresh token lives on one machine (§2.3) — so the three that
publish live on the web client, and a view that needs them has to declare itself a web view rather
than discover at runtime that its transport cannot oblige.

A fill checks **each source separately**: the hazard is a chosen folder that *contains* the
publishing folder, and copying a folder into a folder inside itself is a loop `deliver_all` refuses
before a byte moves. `ingest::scanner::scan_paths` is new for this — `scan_files` walks one root,
which is what a card is, but choosing what to publish means a folder *or* three particular frames.

**Done:** both `check:transport` runs pass, both typechecks, and the desktop returns the same
counts the server does rather than a summary sentence with fabricated numbers beside it.

### PB-7 — the Ingest screen ✔
A **Copy the photographs to** field and a *Copy to a folder* action beside the handover. The
Location check needed nothing: the card table renders a rule's name from the wire.

Copies the shots that did not *fail*. A warning — the odd frame with no coordinates — is something
to see in the table and decide about, not a reason to leave a photograph behind.

### PB-8 — the Publish screen ✔
**Both roads on one screen**, at the user's choice: the publishing folder on top, filled and
prominent, and the handed-over session below it, transparent and muted. Honest about the transition
rather than retiring the old road by stealth — the plan retires it deliberately after MV-16.

The folder panel reads the folder on opening the tab, because whatever is in it is what would be
published however it got there. It states plainly that everything listed will be **removed from
this folder** after a successful upload, which is the mitigation for the one case the code cannot
prevent: somebody *moving* files in rather than copying them.

**The dry run is bound to the bytes.** The screen holds the session id it reviewed, and compares it
with the folder's current one; add a file, or geotag one, and it says the review no longer counts
rather than letting the publish button stay lit.

Publish stays web-only — the refresh token lives on one machine — so only `fillPublishing` is on
`ApiClient`. **Done:** `check:layout` clean at 390 px with both roads, both builds, both transport
checks.

### PB-9 — documents ✔
`known-gaps.md` records the contradiction with §6.3 and its three consequences;
`phase-reports/publish-folder.md` reports what was delivered and where the build diverged from this
plan; **MV-16** adds eight checks, and `testing.md` gives them a session of their own.

The plan is a placeholder no longer only in one respect: `PUBLISHING_DIR` still has no value
anywhere, and publishing refuses until it does.

---

## Tests

**Configuration** — the symlink, the `..`, the same-named folder, the unset value. Each refused,
each with a reason naming which rule refused it.

**The Location check** — present, absent, and a video; none of them failing a card.

**Check-and-copy** — byte-identical arrival; a mid-copy failure reported per file; *the card is
unchanged*, hashed before and after.

**The deletion, which is where the tests earn their keep:**
- a file whose upload failed is **still there** afterwards
- a file whose upload succeeded is gone
- a run where every upload failed deletes **nothing**
- a symlink inside `Publishing` pointing at a file outside it is not followed
- the folder itself survives being emptied
- a file added to `Publishing` *after* the plan was made is not deleted, because it was not uploaded

**The ledger** — a byte-hash row and a source-hash row are distinguishable by a later reader.

## MV-16 — what only real photographs and a real NAS can settle

| Id | Check |
|---|---|
| MV-16.1 | A card's passing frames arrive in the chosen folder byte-identical, and the card is unchanged |
| MV-16.2 | The Location check marks the right frames, and fails none of them |
| MV-16.3 | A folder published from `Publishing` appears in Google Photos with its dates and locations |
| MV-16.4 | `Publishing` is empty afterwards — and the source folder the files were copied from is not |
| MV-16.5 | A deliberately failed upload leaves that file in `Publishing` and publishes the rest |
| MV-16.6 | Pointing publishing at a folder that is not `Publishing` is refused |
| MV-16.7 | A tool writing output straight into `Publishing` publishes and clears correctly |
| MV-16.8 | One photograph first, checked in Google Photos, before any bulk run (§6.4) |

## Risks

| | |
|---|---|
| **The deletion is the whole risk** | Four rules, six tests, a dry run, and per-file evidence. The residual risk is a Google response we misread as success; MV-16.5 is the check that a failure really does leave the file. |
| **A tool's output in `Publishing` is the only copy** | Stated on the screen before the run. Regenerable from the original, but it will be gone. |
| **Weaker deduplication than F16** | Stated on the screen, and the empty folder is the real safeguard. |
| **Two roads to publishing during the transition** | Deliberate and recorded above; the session road is retired in a separate change after MV-16. |
| **`/Publishing` is a placeholder** | The real NAS path is settled before PB-6. Nothing else depends on its value. |
