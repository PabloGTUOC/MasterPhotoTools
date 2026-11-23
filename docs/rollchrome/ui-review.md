# Rollchrome interface review

> **Written 2026-11-23** by the architect, at the owner's request: *"in the tab the menus are not responsive,
> there are options that fall outside the window, the tab selector on the left does not scroll down if the
> image is big and I need to scroll up to see them again, please review the full UI user interface as is far
> from good."* In the same message the owner asked for every Develop look to be available in Edit, and for
> RAW files to open in Edit. That reverses RP-2b ("RAW only in Develop"), which the owner says was their own
> earlier choice.

## How it was reviewed

- **Measured, not just looked at.** A harness mounts the whole application (`App.vue`, the router and all
  three screens) in Chromium. It is backed by the three screens' existing stand-in APIs, merged.
- **Window sizes:** 1024×700, 1280×800, 1440×900 and 1920×1080.
- **Content:** a landscape and a portrait photograph in Edit; six RAWs in Develop.
- **In Edit**, every collapsed section was opened. The script recorded the controls that run past the panel
  that clips them, the page's height against the window, and where the sidebar ends up.
- **Not covered:** the harness uses stand-in images, so the real app with the owner's photographs was not
  driven. What the owner describes matches what was measured.

## Findings

### A. The application shell: the page scrolls, so the navigation scrolls away

`App.vue` gives the shell and the main column `min-height: 100vh`, not a height. Any screen taller than the
window makes the **whole page** scroll, sidebar included:

| Screen | Window | Page height | Sidebar after scrolling |
|---|---|---|---|
| Develop, six photographs | 1440×900 | 1667 px | Scrolls away |
| Edit, sections opened | 1440×900 | 1088 px | 188 px off the top |
| Batch | 1024×700 | 1028 px | Scrolls away |

This is the owner's *"the tab selector on the left does not scroll down"*.

The sidebar also costs **232 px** of width to show three tabs, and the status bar repeats the app's name
and a clock. On a 1280 px window, a fifth of the width goes to navigation.

### B. Edit's adjustments panel: controls cut off at every window size

The panel is a fixed column, about 340 px of content. At **every** window size the same controls run past
its right edge and are clipped:
- the masks' **"+ Brush", "+ Subject", "+ Sky"** (the third column of a three-column grid);
- HSL's **Magenta** swatch;
- Colour grading's **"Global"** tab, which shows as "GLOE".

Also in the panel:
- **The colour-grading wheel** is cut at the top when its section scrolls into view.
- **Number fields show the browser's spin arrows** on top of their values ("0▴▾" beside Blacks and
  Luminance).
- **The histogram is pinned** to the top of the panel and takes about 200 px of it. At 800 px of window
  height only about 400 px remain for the controls, so the panel is almost all scrolling.
- **The panel cannot be made wider.** A bigger window gives the extra width to the image, never to the
  controls that are cut off.

This is the owner's *"options that fall outside the window"*.

### C. Pickers open inline and push the screen around

"Choose…" next to the photograph's path opens the folder browser **inline**: the image moves down about
180 px and the page grows. The export destination's picker opens inside the adjustments panel the same way.
A picker should open over the screen and leave the layout where it was.

### D. Edit's image area

- The view toolbar under the image (Before/After, Side by side, Split, Fit, 100 %, zoom, clipping) wraps
  onto **two rows** at 1280 and 1440, which takes height from the image.
- A portrait fits by height. With the path field and the two-row toolbar, it gets about 610 px of a 900 px
  window.

### E. Develop

Its preview is a 400 px column; DV-17, in progress, rebuilds that layout. Develop also grows the page with
its folder (finding A), which DV-17's own scrolling photographs panel and the shell fix both address.

### F. Batch

Batch works, but it is a long single column of forms. Most of it scrolls off a 700–800 px window, and the
"Run is unavailable" notice sits at the very bottom. It is the least used screen; once the shell is fixed,
this is cosmetic.

### G. Maintainability

`Edit.vue` is **8,349 lines**: the screen, the panel, every section and their state in one file. Every
layout change to Edit has to be made inside it. That has been workable for the implementer so far, but it is
why problems like B survive: no section is ever rendered or measured on its own.

## What Edit lacks that the owner asked for

1. **Every Develop look in Edit.**
   - Edit's look picker lists only the LUT library. Develop also offers the 12 built-in looks and the 18
     film simulations.
   - DV-6 already renders a film simulation **as a 3D LUT** (`media::film::film_lut`), so Edit can offer
     all of them through the LUT path it already has. The LUT version leaves out grain and halation, which
     are spatial.
   - Whether Edit should also add grain is a separate decision.
2. **RAW files in Edit.**
   - Withdrawn by RP-2b. Bringing it back means opening a RAW by developing it with Develop's neutral (the
     same picture Develop shows for "None"), then editing that.
   - The sidecar stays `NAME.RW2.photoedit`, so a RAW and its JPEG never share edits.
   - Export develops the RAW at full size again.

## Proposed steps

Each is one brief for agy, in this order. The first two are what the owner hit; the third is what they
asked for.

| Step | What | Size |
|---|---|---|
| **UI-1** | **The shell.** The window, not the page, is the frame: shell `height: 100vh`, each screen scrolls inside its own areas, and the navigation never moves. The sidebar becomes a 56 px icon-and-number rail; the status bar goes. A check asserts the navigation's position after scrolling every screen, at 1024–1920. | Small |
| **UI-2** | **Edit's panel and image area.** No clipped control at 1024–1920: masks in two columns, HSL swatches wrapping, grading tabs fitting or scrolling, the wheel not cut. No spin arrows. Histogram collapsible and unpinned. The panel resizable between 320 and 560 px and remembered. The view toolbar on one row with a compact overflow. Pickers open as overlays. A check measures every control against the panel that clips it, as this review did. | Medium |
| **UI-3** | **Looks and RAW in Edit.** The look picker groups Built-in looks, Film simulations and the LUT library; a film look applies as its LUT. RAW opens through the neutral development and exports at full size. | Medium |
| UI-4 | **Split `Edit.vue`** into the screen, the panel and one component per section, with no change in behaviour. Optional; it makes UI-2 and later work cheaper and safer. It could go first if the owner prefers. | Medium |
| UI-5 | Batch laid out in two columns, with the notice beside Run. | Small |

DV-17 (Develop's preview) is already with agy and is consistent with UI-1. UI-1 goes after it, so the two
do not edit the same screen at once.

## The harness

The whole-application harness and its measuring script are kept outside the repository for now (the
architect's scratch copy). UI-1's brief adds a version of them to `frontend/rollchrome/scripts/` as
`check:layout`, so every later step is measured the same way.
