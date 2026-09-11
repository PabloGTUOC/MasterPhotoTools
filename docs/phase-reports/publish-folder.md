# Publishing a folder — moving the road so the tools can stand on it

**Status:** PB-1 to PB-9 built, gates pass. Eight checks await a Mac, a NAS and real photographs
(MV-16). The session road is still live and is retired separately.

Planned in [`publish-folder-plan.md`](../publish-folder-plan.md) before any code was written, and
reported here on the same terms as a phase.

---

## Why this exists

The road to Google ran card → validate → derive → hand off a session → publish that session. Every
tool this application has — geotag, TIFF to JPEG, borders, half-frame split — sat *beside* that
road rather than on it. There was no point in the pipeline where a photograph could be changed
before it was published.

That is not a theoretical complaint. **The Google Photos API cannot delete what it creates**, so
publishing first and fixing afterwards is not a recovery, it is a duplicate. The tools were
unreachable from the only workflow that ended at Google.

## Standing against the specification

**This contradicts the specification rather than extending it.** §6.3's publish flow, F15 and F16
all assume a session: `plan_publish` takes a manifest, a `SessionPlan` and an arrival report;
`dry_run` records itself against a `session_id`; F16's ledger is keyed on the source hash carried
in a manifest entry.

`SPECIFICATION.md` was not edited (G9). Recorded in [`known-gaps.md`](../known-gaps.md). The
decision is the user's, taken deliberately: the specification describes a pipeline with no editing
step, and that is the thing being fixed.

## Delivered

| | |
|---|---|
| `config.rs` | `publishing_dir`, `resolve_for_publishing`, and the refusal of tool output into that folder |
| `ingest/validation.rs` | `Rule::Location` |
| `ingest/deliver.rs` | check-and-copy to a chosen folder, keeping the camera's names |
| `ingest/scanner.rs` | `has_location`, and `scan_paths` for a mix of folders and files |
| `publish/folder.rs` | the walk, the plan, the dry run, the publish, and the emptying |
| `ledger.rs` | migration 8: `published.key_kind`, `sessions.folder` |
| `crates/server/src/api.rs` | four routes |
| `crates/desktop/src/commands.rs` | `deliver_card`, `fill_publishing` |
| `frontend` | the Ingest destination, and both roads on the Publish screen |

## The decisions that shaped it

### Everything in `Publishing` is a copy — enforced, not trusted

The deletion is safe only because nothing in that folder is the only copy of anything. The plan
made that a convention; the user's clarification — *nobody writes to Publishing* — made it possible
to enforce, and `Config::resolve_for_create` now refuses any tool output path inside it. Borders,
conversions and splits cannot put their only copy in the one folder that gets emptied.

What that cannot reach is a person *moving* files in from a file manager rather than copying them.
No code can tell the difference afterwards, so the screen says plainly that everything listed will
be removed.

### The dry run is bound to the bytes, not to the folder

The session id for a folder publish is a hash of every file's relative path and content hash, in
order. §9.2 rule 3 makes a dry run mandatory; this makes it mandatory **for what was actually
reviewed**. Add a file after the review, or geotag one — which rewrites it — and the id changes,
the recorded dry run no longer matches, and publishing refuses.

An id that was a fact about the *folder* would have let a file slip in between the review and the
upload. Given an API that cannot delete, that gap is the whole reason the rule exists. The screen
holds the reviewed id and says so; the server refuses independently.

### Deduplication answers a weaker question, and says so

F16 keys on the **source** hash — the bytes the camera wrote, which nothing rewrites — so a row
means *this photograph has been published*. A folder holds files that have been through the tools,
and geotagging changes a photograph's hash, so folder publishing can only key on the bytes it
uploaded: *this file has been uploaded*.

Both rows live in one table because both mean "do not upload this again". Migration 8's `key_kind`
is what stops a later reader believing the weaker claim is the stronger one. What actually prevents
double-publishing is that a successful publish empties the folder — the empty folder is the record.

### The publisher is reused, not rebuilt

`Publisher` reads each file as `staging_dir.join(file_name)`, so a folder publish is that same
publisher with `staging_dir` pointed at the folder and `file_name` relative to it. Retries,
resumption, rate-limit backoff and the dry-run check come along unchanged. The component that
uploads photographs to a service that cannot delete them is not the one to have two of.

### The location check warns only on the odd one out

The plan said warn whenever a frame has no coordinates. Built that way and looked at, it is an
amber mark on nearly every card this application will see — most cameras have no receiver — and a
mark on every card is a mark on none. Worse, `passing()` counts a warn as not-passing, so every
clean card would have left the headline count over a fact about the camera.

It follows the date rule's shape instead: flag the exception. A card recording nothing passes with
the absence stated; a frame with none where the others have them warns, because that is the frame
that will look wrong in Google Photos beside the rest.

## Deviations from the plan

| | |
|---|---|
| The Location check | Warn-on-absent became warn-on-odd-one-out, above. |
| Rule 3 of the deletion | Went from a convention to an enforced check, after the user ruled that nothing writes to `Publishing`. |
| The session id | The plan said "a lightweight session row"; it is bound to the folder's contents instead, which is strictly stronger. |
| The Publish screen | Both roads on one screen, at the user's choice, rather than replacing the session flow. |
| PB-7's front-end work | Almost none needed: the card table renders a rule's name from the wire, so `Location` appeared unaided. |

## Defects found while building this

| | |
|---|---|
| **The desktop lied to the type system** | `fillPublishing` returned the real summary sentence with three fabricated zeroes beside it, because the counts were the server's shape. The command now returns the same shape. |
| **A duplicate helper broke the running app** | A second `fn yes()` in `commands.rs`; the desktop dev session died rebuilding on it. Caught by the build, but only after it had killed a session somebody was using. |
| **A shipped migration was edited** | `sessions.folder` was appended to migration 8 *after* 8 had run, so every database that had seen the old 8 stayed at `user_version = 8` and never gained the column — while a fresh one got it on creation. The warning against exactly this is written at the top of the migration list. It surfaced as `table sessions has no column named folder` the first time somebody published a folder: an error naming neither cause nor remedy. Split into migration 9, and the schema-equality test now walks **every** version rather than starting at 6, which is what would have caught it. |

## Acceptance

| Claim | How it is held up |
|---|---|
| Only the publishing folder is ever deleted from | Ten config tests: sibling, shared name prefix, same name elsewhere, symlink out, `..` out, unset, missing |
| A failed upload leaves the file where it is | `a_photograph_whose_upload_failed_is_still_there_afterwards` |
| A run where everything failed deletes nothing | `a_run_where_everything_failed_deletes_nothing` |
| Uploaded but unconfirmed is not deleted | `a_photograph_uploaded_but_never_confirmed_is_kept` |
| A file that arrived after the plan is not deleted | `a_file_that_arrived_after_the_plan_was_made_is_not_deleted` |
| A symlink out of the folder is not followed | `a_symlink_out_of_the_folder_is_not_followed` — the archive it points at survives |
| The publishing folder itself survives | `the_publishing_folder_itself_survives_being_emptied` |
| A dry run cannot be skipped | `publishing_a_folder_refuses_without_a_dry_run` — the token provider panics if asked, so the refusal precedes any network work |
| Editing the folder invalidates the review | `adding_a_file_changes_the_session_and_so_invalidates_the_dry_run`, `changing_a_files_contents_also_invalidates_the_dry_run` |
| The card is never written to | `the_card_is_not_touched`, hashed before and after; `a_destination_inside_the_card_is_refused_before_anything_is_copied` |
| Nothing is overwritten in a working folder | `a_different_photograph_of_the_same_name_is_never_overwritten` |
| A tool cannot write into `Publishing` | `a_tool_cannot_write_its_output_into_the_publishing_folder` |

## Gates

| | Before | After |
|---|---|---|
| `cargo test --workspace` | 632 | **692** |
| `cargo test -p phototools-core` (G2) | 551 | **607** |

`fmt`, `clippy -D warnings`, both typechecks, both builds, both `check:transport` runs,
`check:layout` at 10 routes and `check:ingest` all pass.

## Not done, deliberately

- **The session road is still live.** It goes unused the moment Ingest stops handing off, but
  removing a working, tested subsystem before its replacement has met real photographs is the wrong
  order. It is retired in a change of its own after MV-16, which also retires the MV-11 items it
  covered.
- **`PUBLISHING_DIR` has no value anywhere.** `/Publishing` was a placeholder throughout and never
  became a default; publishing refuses until somebody sets it.
- **Nothing has published a real photograph this way.** Every claim above is about the pipeline
  being faithful to its own rules. Whether Google receives what we think we sent is MV-16.3, and
  whether the folder empties correctly afterwards is MV-16.4.

## MV-16 on the real thing — 2026-09-11

Five of the eight run against the live server, the real ledger and the connected account.
**16.4, 16.5 and 16.7 pass.** 16.3 sent a photograph and Google confirmed it; **nobody has looked
in Google Photos to see what date and position it arrived with**, which is the only question that
item asks. 16.6's refusal is confirmed, its screen is not. 16.1, 16.2 and 16.8 need a card in a
reader.

Both defects the run found are in what the software **says**, not in what it does. That is worth
naming: every rule held. The failure was isolated, the folder was emptied of exactly what Google
confirmed, the tool refused to write into `Publishing`, and the edited folder refused to publish.
What was wrong each time was the sentence describing it.

**A count is not a report.** MV-16.5's 0-byte file failed and was kept, correctly, and the summary
said `1 failed`. It did not say which, or why. Which was recoverable — the failed file is the one
still sitting in the folder. **Why was not recoverable from anywhere**: Google's reason lived in
`PublishOutcome::failed` for the length of the job and was then dropped, and it is the only thing
that decides whether to retry the file, repair it or give up on it. `describe` now names up to
three with their reasons and counts the rest; a run where four hundred failed has one cause, and a
summary listing all four hundred is not read.

**Google's answer is a pretty-printed JSON document.** Named, the reason arrived as
`broken.jpg (Google refused the request (400): {` — then two more lines — in the middle of a
one-line job summary. `ApiError`'s `Display` now flattens whitespace and caps the body at 200
characters. The cap is not for Google, whose errors are short: it is for the day a proxy or a
captive portal answers instead, with an HTML page.

**"No dry run" was true and useless.** A folder session id *is* a hash of the folder's contents, so
editing the folder makes a session nobody reviewed — refused, correctly. But the refusal came from
the generic `Publisher`, which said *"session folder-564fb… has had no dry run"* to somebody who
had run one thirty seconds earlier. A safeguard that describes itself as a bug gets worked around.
`publish_folder` now answers for itself and offers the explanation that is true nine times in ten:
a file has been added, removed or edited since.

One thing that passed for a reason worth recording. MV-16.7 refuses a tool writing into
`Publishing` — and the configured folder is deliberately **outside** the library roots, so the
refusal could have come from G6 with the message *"outside the configured library roots"*: true,
unhelpful, and pointing at the wrong problem. The publishing check runs first, so it says what it
means.

Gates: `fmt`, `clippy -D warnings`, **705 workspace, 620 core**. Seven new tests, all of them
asserting what a message says, because that is what broke.

