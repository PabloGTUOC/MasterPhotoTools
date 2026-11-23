# Rollchrome as a desktop application

> **Written 2026-11-23** from the owner's direction: *"Rollchrome is a desktop app, so we need to minimize
> scrolling and behave less like a website and more like an app like Lightroom. For instance I need to scroll
> down to see the develop options after the dry run, that makes no sense."* It sets the layout rules every
> screen follows, and it changes the plan in [`ui-review.md`](ui-review.md).

## The owner's choices (2026-11-23)

| Question | Answer |
|---|---|
| Develop's arrangement | **A filmstrip along the bottom**, the preview in the middle, settings on the right: Lightroom's Develop module |
| Where a dry run's summary and a job's progress go | **A status strip along the bottom of the window**, which expands for details |
| Edit | **A folder and a filmstrip**, like Develop, so photographs change without typing paths |

## The rules

1. **The page never scrolls.**
   - The window is divided into fixed regions: the rail, the top bar, the main area, the side panel, the
     filmstrip and the status strip.
   - Only a region's own list scrolls: a panel's controls, the filmstrip, the details of the strip.
   - `check:shell` already asserts that the page does not scroll; each screen's step adds its own assertions.
2. **The primary actions are always on screen.**
   - Develop's *Dry run* and *Develop*, Edit's *Export* and Batch's *Dry run* and *Grade* sit in a fixed
     action area at the foot of their panel. They are never inside the panel's scrolling part.
   - At 1024×700, with every section open, the action buttons are inside the viewport.
3. **Results appear where the eye already is.**
   - A dry run's summary and a running job's progress go to the **status strip**: one line, always visible,
     for example "Dry run · 5 to develop · 1 skipped" or a progress bar with "3 / 6 · 0:42 left".
   - Clicking the strip opens its **details** (skipped items, failures, destinations) upward, over the
     content, at most 40% of the window's height. Escape or a second click closes them.
   - Nothing is appended below a screen's content.
4. **Photographs are picked from a filmstrip, not typed.**
   - Develop and Edit open a **folder**. Its photographs show as a filmstrip along the bottom of the main
     area: horizontal, scrolling on its own, about 96 px high.
   - Click selects. Cmd-click and Shift-click extend the selection, which Develop already supports in its
     grid. ← and → move between photographs.
   - A path can still be pasted into the folder field.
5. **The photograph gets the space.**
   - The preview fills what is left of the main area, at full height, with `object-fit: contain`.
   - Side panels have a sensible default width and can be resized and remembered, as DV-17's divider is
     (UI-2 does Edit's).
6. **Pickers and confirmations are overlays.** They open over the screen and never push the layout.

## How it fits the code

- **The status strip** is the bottom row of `App.vue`'s main column, replacing today's status bar. It holds
  an empty `<div id="status-strip">`, and the version and clock move to its right end.
- **Screens fill it with `<Teleport to="#status-strip">`.** Develop's review summary and job section, Batch's
  and Edit's export progress keep their markup and `data-testid`s. They are only rendered elsewhere.
- The existing checks mount a screen on its own, without the shell. There, the teleport is **disabled** when
  `#status-strip` does not exist, and the same elements render in place, so the existing checks keep finding
  them.
- **The filmstrip** is one component, `src/components/Filmstrip.vue`, shared by Develop and Edit. It takes
  props in and emits events out (`items`, `thumbnailUrls`, `selection`, `active` → `select`, `activate`), and
  it never calls the API, by the front-end boundary rule.

## The steps, in order

| Step | What | Replaces |
|---|---|---|
| UI-4 | Split `Edit.vue` into section files (with agy now) | — |
| UI-2 | Edit's panel: nothing clipped, resizable, histogram collapsible, toolbar on one row, pickers as overlays, **a fixed action area for Export** | — |
| **UI-6** | **The status strip.** The shell's strip and details drawer. Develop's summary and progress, Batch's, and Edit's export progress move there by teleport. Develop's and Batch's Dry run and Run move to fixed action areas. A check: after a dry run at 1024×700 the summary and the action buttons are visible without scrolling anything | — |
| **UI-7** | **Develop as Lightroom's Develop.** Folder field in a top bar, the preview in the middle, `Filmstrip.vue` along the bottom with the current selection behaviour, settings on the right. DV-17's left photos panel and its divider give way to the filmstrip; Enlarge stays | DV-17's photos column |
| UI-3 | Edit: every Develop look, and RAW files | — |
| **UI-8** | **Edit opens a folder.** `Filmstrip.vue` along the bottom; choosing a photograph opens it as the path field does today, with its own sidecar. Pairs with UI-3's RAW support | Edit's single-file path field (kept for pasting) |
| UI-5 | Batch in the same frame: inputs on the left, settings on the right, actions fixed, results in the strip | — |
