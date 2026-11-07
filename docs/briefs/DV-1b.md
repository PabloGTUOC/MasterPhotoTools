# `DV-1b` · A default crop wider than rawler's active area, and no panic out of `develop`

> **Status:** briefed
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-1`, a defect found on a real file · **Branch:** `dv-1b`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11) and **Gates**.
- `docs/briefs/DV-1.md` — the neutral development, and what `develop_linear` promises: a file that
  cannot be developed is refused **with its reason**, never a panic, never the embedded preview.
- `crates/rollchrome-core/src/media/develop/mod.rs` — `develop_linear`, steps 4 (demosaic over the
  active area) and 7 (the default crop).
- `crates/rollchrome-core/tests/support/dng.rs` — the handmade DNG writer; it already has
  `active_area`, `default_crop_origin` and `default_crop_size`.

### Goal

The first real RAW put through Develop crashed it. `Target.RAF`, from a Fujifilm X-Pro2, panics
inside `develop_linear`:

```
panicked at rawler-0.7.2/src/imgop/mod.rs:202:5: assertion failed: self.d.w <= master.d.w
   <rawler::imgop::Rect>::adapt
```

What rawler reports for that file:

| | |
|---|---|
| sensor | 6048 × 4038 |
| `active_area` | x 0, y 0, 5920 × 4038 — from rawler's camera table, `data/cameras/fuji/x-pro2.toml`, `active_area = [0, 0, 128, 0]` |
| `crop_area` | x 16, y 21, 6000 × 4000 — from the RAF itself; the camera's JPEG is 6000 × 4000 |

The crop does not fit inside the active area, and step 7 calls rawler's `Rect::adapt`, which
**asserts** containment. That call is outside the `catch_unwind` that guards decoding, so the panic
reaches the caller: in a folder develop it takes down the job rather than skipping one file.

**The crop is right and the table is wrong.** Measured on the raw data: columns 5920–6031 hold
picture (mean level ~1100 over a black of 1023, edges of real detail), and only columns 6032–6047
are empty. A demosaic that stops at 5920 would lose 96 real columns.

Two changes, both needed:

1. **When the default crop is not inside the active area, demosaic the smallest rectangle that
   contains both, clamped to the sensor** — then take the crop from that. When the crop is inside
   the active area, nothing changes. A crop that extends beyond the sensor itself is refused, with
   the numbers in the reason.
2. **`develop_linear` never panics, whatever the bytes.** Nothing after decoding is guarded today;
   any other assertion in rawler, or an index out of range in our own code, would crash the caller
   the same way. Guard the whole of `develop_linear` (not only the decode), so that a panic anywhere
   in it becomes `Error::Internal` with the panic's message, as the decode's already does. This is
   a safety net, not the fix: change 1 is what makes the X-Pro2 develop.

### Change

- `crates/rollchrome-core/src/media/develop/mod.rs`:
  - Stop calling `Rect::adapt` (or any rawler helper that asserts) on metadata from the file. Work
    out the demosaic rectangle and the crop's offset within it with **checked arithmetic of our
    own**.
  - The demosaic rectangle is the active area, or, when the crop is not inside it, the bounding box
    of the active area and the crop, clamped to `0..width` × `0..height`. Both the Bayer (PPG) and
    the X-Trans paths receive it. The CFA phase must stay right: a rectangle that starts at a
    different column from the active area still has to demosaic every photosite as its true colour
    (the X-Trans colour test below proves it).
  - A crop that reaches outside the sensor is `Error::Refused`, with the crop and the sensor size in
    the message.
  - Wrap the body of `develop_linear` so that a panic becomes `Error::Internal("RAW development
    panicked: <message>")`. Keep the existing decode-specific message too, or fold it into this —
    your choice, say which in the report.
  - A comment where the rectangle is chosen saying **why**: rawler's camera tables can understate
    the active area (the X-Pro2's says 5920 wide; the picture is 6016), and the file's own crop is
    the authority on what the camera meant to keep.
- `crates/rollchrome-core/tests/develop_tests.rs`: three new tests, below.
- `docs/known-gaps.md`: nothing — this step closes a defect rather than recording one.

### Do not touch

- rawler itself, and its camera tables (no fork, no patched copy, no `[patch]`).
- `tools/develop.rs`, `develop_folder.rs`, `render.rs`, `xtrans.rs`'s algorithm, the tone and the
  colour steps. Their behaviour does not change.
- Any existing test or threshold (G7).

### Acceptance

- [ ] `a_default_crop_wider_than_the_active_area_still_develops` — a Bayer DNG whose `ActiveArea` is
      narrower than its `DefaultCropOrigin` + `DefaultCropSize` (for example sensor 96 × 64,
      active area 80 columns wide, crop 88 wide) develops without error to exactly the crop's
      dimensions, and its known grey comes out within the tolerance the existing grey test uses.
- [ ] `an_xtrans_crop_beyond_the_active_area_keeps_its_colours` — the same shape on an X-Trans
      DNG, with the known colour patches, within the existing X-Trans patch tolerances. Arrange it
      so the patches lie partly in the columns outside the active area.
- [ ] `a_crop_outside_the_sensor_is_refused_not_panicked` — a DNG whose crop extends beyond the
      sensor returns `Err(Error::Refused(..))`, and the test asserts the message names the crop.
      If rawler itself refuses or clamps such a file before our code sees it, say so in the report,
      and make the test assert whatever error actually comes back is an `Err` and not a panic.
- [ ] **On the real file.** Run the example on the chart RAW that crashed:
      ```sh
      cargo run --release -p rollchrome-core --example develop -- /tmp/dv1b \
        ~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.RAF \
        ~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.DNG
      ```
      Both develop. Report each output's dimensions (`sips -g pixelWidth -g pixelHeight` on the
      PNGs) — expected **6000 × 4000** for both — and each time the example prints. These files
      are not in the repository and must not be added to it.
- [ ] No `adapt(` call left in `crates/rollchrome-core/src/media/develop/`
      (`grep -rn "adapt(" crates/rollchrome-core/src/media/develop/` prints nothing).
- [ ] Test counts: workspace `1066` → `1069`; `rollchrome-core` `241` → `244`; `phototools-core`
      713, `phototools-desktop` 26, `rollchrome-desktop` 15 unchanged. Every test accounted for.

### Gates

```sh
cargo fmt --all --check                                  # clean
cargo clippy --workspace --all-targets -- -D warnings    # clean
cargo build --workspace                                  # clean
cargo test --workspace                                   # 1069 passed, 0 failed
cargo test -p phototools-core                            # 713 passed
cargo test -p rollchrome-core                            # 244 passed
cargo test -p phototools-desktop                         # 26 passed
cargo test -p rollchrome-desktop                         # 15 passed
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual
output** of every gate — paste the `test result:` lines, never just "passed"; the real-file
dimensions and times; and anything you could not verify, said plainly. Do not tick the acceptance
boxes above; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
