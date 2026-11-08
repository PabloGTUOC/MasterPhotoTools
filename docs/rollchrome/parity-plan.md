# Rollchrome towards a Lightroom replacement — plan

> **Status (2026-11-08):** RP-1 to RP-7 and RP-2b done (MV-24 to MV-28 await real files / Mac Studio); RP-8 next. Steps have stable ids (`RP-4`), each built from a
> brief in [`docs/briefs/`](../briefs/).

**The owner's goal is to stop paying for a Lightroom subscription.** The list comes from comparing
Rollchrome and PhotoTools with [LightCraft](https://github.com/storytold/lightcraft) (MIT/Apache),
whose own checklist runs row by row against Lightroom. This plan covers the gaps sized small or
medium. It is not the whole road: see *Not in this plan*.

## Three tabs

| Tab | Takes | Gives | For |
|---|---|---|---|
| **Develop** | RAW files | JPEG or 16-bit TIFF, in `Developed/` beside them | Applying a film simulation to each RAW |
| **Edit** | One JPEG or TIFF | Non-destructive adjustments, an exported file | Detailed work on one photograph |
| **Batch** | A chosen group of JPEGs and TIFFs, or folders | Graded copies | Applying `.cube` LUTs or a preset to many |

The film looks live in Develop because they are fitted to a RAW's ProPhoto development
([DV-3](../briefs/DV-3.md)): applied to a finished sRGB JPEG they would be wrong. Batch grades with
the LUT library, which is sRGB.

**What this model corrects.** Today a RAW opened in Edit or Batch is not developed: `decode_image`
takes the file's embedded camera JPEG through the F14 ladder, so the edit starts from 8 bits with
the camera's look baked in. And neither Edit nor Batch reads an ICC profile, so a Develop TIFF, which
is ProPhoto, would be taken for sRGB and show wrong colours. RP-2 closes the second before anyone
relies on the Develop → Edit path. Edit itself works in sRGB, so colours a ProPhoto TIFF holds beyond
sRGB are clipped on opening it there; the TIFF keeps them, and carrying them through Edit would mean
rebuilding its pipeline, which no step here does.

## Steps

In this order, because the tab that does not exist comes first, and the handover between tabs must
be right before anything is built on top of it. Later steps get their brief when they come up.

| Id | Step | Contains | Size |
|---|---|---|---|
| **RP-1** | Three tabs and the Develop screen | Develop, Edit, Batch in the navigation. Folder of RAWs, thumbnails, a look per photo or for a selection, preview, dry run, run | Medium |
| **RP-2** | Edit and Batch read developed files correctly | Honour the embedded ICC profile, so a ProPhoto TIFF is right on screen and on export. | Small to medium |
| **RP-2b** | RAW only in Develop | The owner's decision: Edit and Batch take JPEG or TIFF and nothing else. Follows RP-3, which is on the same screen | Small |
| **RP-3** | Viewing | Zoom and pan in Edit up to real pixels, which needs a full-resolution preview stage; side-by-side and split before/after | Medium (first sized small, before that was seen) |
| **RP-4** | Colour tools | White balance eyedropper, auto settings, black & white with an 8-band mix | Small to medium |
| **RP-5** | Undo and history | General undo/redo, edit history, named snapshots | Medium |
| **RP-6** | Virtual copies | Several versions of one photograph's edits in its sidecar. The sidecar's *name* is the contract with PhotoTools and does not change | Medium |
| **RP-7** | Detail | Sharpening, luminance and colour noise reduction, hot pixels | Medium |
| **RP-8** | Effects | Clarity, texture, dehaze | Medium |
| **RP-9** | Export options | Long edge or megapixels, quality, JPEG/TIFF/PNG/WebP/AVIF, output sharpening (needs RP-7), naming templates | Small to medium |
| **RP-10** | Point colour | Pick a colour, shift only that colour | Medium |
| **RP-11** | Masks | Colour and luminance range, add/subtract/intersect, background, more local sliders, click selection | Medium |
| **RP-12** | More RAW formats | CR3, RW2, ORF, PEF, SRW. Only what `rawler` decodes, and each format only once a real file has been developed. Sized medium to large because that is unknown until tried | Medium to large |
| **RP-13** | Automation | A command line and an MCP server over `rollchrome-core` | Medium |

**The risk across RP-7 and RP-8.** Today's pipeline works pixel by pixel. Sharpening, noise reduction,
clarity and dehaze look at neighbours, and `check:edit` holds the preview to a timing budget that
these must meet without being loosened (G7).

## Not in this plan

Sized large, and needed before the subscription can really go: the **library** (ratings, flags,
keywords, albums, search, compare), **retouching** (heal, clone, remove), **optics and geometry** (lens
corrections, Upright), **output modules** (print, slideshow, watermark), and the advanced group (HDR,
merge, faces, GPU). The Windows, Linux and browser builds would reverse the desktop-on-a-Mac decision
and are not planned at all.
