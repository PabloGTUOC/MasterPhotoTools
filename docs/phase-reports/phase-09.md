## Phase 9 — Validation and remediation

**Status:** complete. No gates — nothing in F12 or F13 needs hardware, credentials or a Mac.

### Delivered

- **Task 1 — the three rules, each independently testable.** `check_date`, `check_resolution` and
  `check_size` are free functions taking an asset and thresholds. They are called together by
  `validate`, but nothing forces that: each can be exercised on its own, which is what the plan asks
  for and what makes the boundary tests possible.

- **Task 2 — batch median clock detection.** `detect_clock_offset` computes the median capture date
  across the card and, when the median is out of range but the spread is under 30 days, returns a
  single `ClockOffset` carrying `now − median` as an F1 `shift` delta. **The delta is expressed in
  days, not months**, because `exiftool` resolves a month against each file's own date and a month is
  not a fixed length; days are unambiguous.

- **Task 3 — `WARN` versus `FAIL`.** A frame inside the age limit but more than 30 days from the batch
  median warns and carries **no failure class**, so it cannot be swept into a bulk action. F12's
  reason is worth restating: "Frames left over from an earlier shoot on the same card are
  legitimate." A warning is a thing to look at, not a thing to fix.

- **Task 4 — F13's table, and bulk apply.** `actions_for` is the specification's table in one place,
  so a UI cannot offer an action the specification does not sanction or forget one it does.
  `CardValidation::by_failure` groups shot indices by the failure they share, and that grouping *is*
  what "all shots sharing a failure" means — one `BulkRequest` covers the whole class.

  `default_action` encodes F12's consequence: resize is the default for `too_many_pixels`, because a
  10 MP ceiling against a modern body means resizing is the normal path. **Nothing that loses a
  photograph is ever a default** — `Skip` and `PublishAnyway` are always deliberate, and there is a
  test that asserts it for every failure class.

- **Task 5 — resize through the Phase 2 EXIF-preserving path**, with the `plan`/`apply` split build
  plan §7 makes mandatory. `plan` decides everything and touches nothing; the test hashes the card
  before and after planning to prove it.

- **Three server routes and two Tauri commands**, so both front ends have the surface Phase 13 will
  need. `/api/ingest/validate` is synchronous — validation reads no pixels, so it is fast even on a
  full card — while `/api/ingest/remediate` and its Tauri twin **both take `dry_run`**.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — **326 passed, 0 failed**
- [x] `cargo test -p phototools-core` (G2 isolation) — **279 passed, 0 failed**
- [x] **A fixture card where every frame is dated 2019 with a tight spread produces one bulk-shift
      suggestion, not 400 individual failures.** Tested twice: as a unit test at 400 shots, and as an
      integration test over 40 real JPEGs carrying real EXIF.
- [x] **A 24 MP fixture fails the resolution check, and resizing brings it under 10 MP with the
      capture date intact.** The specification calls this test mandatory; see below.
- [x] **A fixture at 10.0 MP exactly passes; 10.1 MP fails.** Asserted as a unit test and again on
      real files at 4000×2500 and 4040×2500.
- [x] **Bulk apply over 50 shots sharing a failure completes as one operation.** One plan, fifty
      rewrites, every one verified under the ceiling and holding its date.

### The mandatory round trip

The specification singles this out: "resizing must preserve EXIF … This requires a dedicated
round-trip test." `a_24mp_frame_fails_and_resizing_brings_it_under_10mp_with_its_date_intact` asserts,
on a real 6000×4000 JPEG:

- the resolution check fails, with class `too_many_pixels`;
- the output is at or under 10 MP;
- the capture date survives **to the second** — `2024:05:30 14:22:11` reads back identically;
- the camera model survives;
- `PixelXDimension`/`PixelYDimension` match the pixels actually written, rather than still
  describing the original — the specification names this explicitly;
- the aspect ratio held.

### A bug this phase found in itself

`rewrite()` originally passed `u64::MAX` as the byte cap to
`reencode_preserving_exif_within`. That meant **the quality ladder never stepped past its first rung**
— which is the entirety of what `ReencodeLower` exists to do. Every test still passed, because none
of them checked the output size against the cap.

`PlannedAction` now carries `max_bytes`, set from the thresholds for both `Resize` and
`ReencodeLower` (F13 puts the ladder in the resize section, so it applies to both). Two assertions
were added that would have caught it: the plan must carry the cap, and the re-encoded output must
actually be at or under it. A third test covers the case where the cap **cannot** be met at quality
75 — the file is still written, because a smaller file is better than none, and it is reported in
`still_too_large` rather than passed off as fixed.

### Measurements

No benchmark is specified for this phase. For reference, the 50-shot bulk resize runs in about 65
seconds in a debug build — decode, resize and encode fifty times, unoptimised. It is the slowest test
in the suite and is dominated by the encoder, not by the bulk machinery.

### Gates

None.

### Deviations

1. **`BATCH_SPREAD_DAYS` decides "far from the batch median", and the specification does not give a
   figure for it.** F12 says a frame in range but far from the median warns, without saying how far.
   Rather than invent a second threshold I reused the 30 days F12 already gives for the camera-clock
   check. **This is a judgement call the specification leaves open**, and it is on the verification
   list because it depends on how long a card sits in a camera between shoots.
2. **Spread is `max − min`, which is the plain reading of the word and is sensitive.** One frame left
   over from an earlier shoot widens the spread past 30 days and suppresses the clock-offset
   suggestion, even when the other 399 plainly share one offset. A robust measure — median absolute
   deviation — would not have that weakness, but it is not what F12 says. Implemented literally and
   recorded here.
3. **`CheckStatus::Pending` is a fourth status the specification does not name.** A RAW-only shot's
   candidate does not exist until F14 derives it, so its *size* cannot be checked — a 30 MB RAW says
   nothing about the JPEG that will come out of it. Reporting `Pending` is more honest than passing a
   check that was not run or failing one that cannot be. Its dimensions are real, so resolution is
   still checked.
4. **Unknown dimensions warn rather than fail.** F11 forbids decoding to learn an image's size, so a
   file whose metadata omits it is genuinely unknown. That is not evidence of being over the ceiling,
   and treating 0×0 as a 0 MP pass would be worse. This is the behaviour I flagged before starting
   the phase.
5. **`/api/ingest/remediate` takes `dry_run`**, which is what specification §9.2 rule 3 and §3
   require of every destructive operation. **The five image-tool endpoints from Phase 6 still do
   not** — that gap is unchanged and still open.
6. **`chrono` added to `crates/desktop`** to parse dates at the command boundary. Recorded per G8.
7. **`RemediationTool` implements `Tool`, but `plan_bulk`/`apply_bulk` are the real entry points.**
   The trait's associated `Params` type cannot carry a lifetime, so the trait impl needs
   `RemediationParams<'static>` while callers work with borrowed shots. The free functions are
   lifetime-generic and are what the server and desktop call; the trait impl exists so the tool
   satisfies §7's shape.

### Added to manual-verification.md

Four Phase 9 entries, all judgement rather than correctness: whether a resized 24 MP frame still
looks right, whether 30 days suits how the cards are actually shot, whether a clock-offset suggestion
should be trusted before applying it to four hundred frames, and whether quality 75 is low enough to
publish.

### Notes for the next phase

- **Phase 10 (F14) plugs into `needs_derivation`.** Every RAW-only shot is already flagged, and its
  size check already reports `Pending`. When F14 produces the candidate, re-running `validate` decides
  the pending checks with no change to this code.
- **F13's resize is the same path Phase 10 needs.** A derived JPEG passes through validation and
  resize exactly as a camera JPEG does, which is what F12 means by "both apply to both the JPEG path
  and the RAW-derived path".
- **The dry-run gap on the five image-tool endpoints is now the only place the API contradicts §9.2
  rule 3**, since ingest remediation has one. Worth closing before Phase 13, which needs a stronger
  version of the same guarantee.
