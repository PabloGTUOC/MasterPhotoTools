# Develop — RAW files to finished photographs, a film look each

> **Planned, not started.** Rollchrome's third screen, beside the Edit and Batch Grade it inherits
> ([`rollchrome-plan.md`](rollchrome-plan.md)). DV-1 needs RC-1. Nothing here touches PhotoTools.
>
> **Functionality first.** The screen is discussed with the owner before it becomes a step
> (owner, 2026-11-05: *"the GUI we can discuss later, first the functionality"*).

Steps have stable ids, `DV-1` onwards, like RC, ED and MV.

---

## What it does

The owner, 2026-11-05: pick a folder of RAW files; give the photographs a film look — the same one
for all, or different ones for different photographs; press **Develop**; get a finished JPEG or TIFF
of each.

The idea comes from darktable's
[Film Simulation Panel](https://github.com/bastibe/Darktable-Film-Simulation-Panel): a list of
looks, and one click gives the selected photographs one of them. The idea, not its layout.

**Batch Grade stays as it is.** The two differ in where they start and what they deliver:

| | Batch Grade (unchanged) | Develop |
|---|---|---|
| Starts from | the image the file gives — for a RAW, **the camera's own embedded JPEG** | **the sensor data**, developed neutrally |
| Look | one LUT file for the whole list | one per photograph, from a library |
| Delivers | a graded copy | the finished photograph: 8-bit JPEG or 16-bit TIFF |

## Why a RAW needs a development of its own first

Verified in the code at `2a3e085`:

- **A RAW reaches the editor as the camera's embedded preview.** `media/edit/pipeline.rs:1005`
  calls `media::raw::raw_to_jpeg`, whose ladder tries the embedded preview first
  (`media/raw.rs:76`). That is right for F14 — publishing wants the camera's look — and wrong under
  a film look, twice: the camera's look is already baked in, so a Fujifilm file shot in Velvia would
  get Velvia *and* the chosen look; and the data is 8-bit, so a "16-bit TIFF" would hold 8 bits of
  information.
- **rawler's own development mishandles Fujifilm files.** `RawDevelop::develop_intermediate` in
  rawler 0.7.2 sends every RGB sensor pattern through PPG, a Bayer algorithm — X-Trans included,
  because `CFA::is_rgb` (`cfa.rs:193`) only checks which letters the pattern contains. rawler *has*
  X-Trans demosaicing (`imgop/sensor/xtrans/xtrans_markenstein.rs`); its `develop` never calls it.
  What PPG makes of an X-Trans file has not been looked at.
- **Fujifilm's DR200 and DR400 underexpose the RAW by one and two stops**, and the camera's JPEG
  brightens it back. A neutral development shows those files dark — which is why the darktable
  panel carries DR buttons, and why Develop needs an exposure offset per photograph.

So Develop stands on a development that never uses the embedded preview, demosaics X-Trans as
X-Trans, and stays at 16 bits or float until the encode.

## The pipeline, per photograph

1. **Develop the RAW neutrally.** Decode with rawler; demosaic (X-Trans with an X-Trans algorithm);
   white balance as shot; the camera's colour matrix; into linear float. No camera look, no embedded
   preview, and **no falling back to it**: an output made from the preview would look plausible and
   be wrong, so a file that cannot be developed is skipped with its reason (G10).
2. **Exposure offset**, in EV, per photograph. Default 0.
3. **The base tone**: one fixed, documented transform from linear to display, with a highlight
   roll-off. Every built-in look is fitted against exactly this, so changing it means refitting
   them (DV-3).
4. **The look**: a 3D LUT, through the engine that already applies LUTs for Edit and Batch Grade.
5. **Encode** to 8-bit JPEG through the deliverable encoder (as F14), or 16-bit TIFF (as Edit's
   export, `tools/edit.rs:316`). **Never overwrites** — the exclusive create Edit's export already
   uses.
6. **Metadata** — capture date, camera, lens, exposure, GPS — through `carry_metadata`, the call
   Edit's export makes, so that RC-4 replaces both at once (R1 in the Rollchrome plan).

Steps 1–3 are DV-1, 4–6 are DV-2. A film-development stage between 2 and 3 is DV-4, if wanted.

## Where the looks come from

**The LUT library, from the start.** Any `.cube` in the library (`tools/lut_library.rs`, ED-6) is
a look. Develop is usable before a single built-in look exists, with whatever LUTs the owner
already has — including an Ektachrome, which none of the material below is.

**Built-in looks, fitted by us from public-domain chart photographs** (DV-3). The darktable
repository is released into the public domain, and decoding its files shows:

- **Its styles cannot be used as they are.** A `.dtstyle` is parameters for two darktable modules,
  `colorchecker` and `tonecurve`, fitted against *darktable's* rendering of a chart. On Rollchrome's
  rendering they would be off by however much the two renderings differ.
- **They carry their measurements.** Every one of the 20 film styles holds 24 patch pairs — the
  patch as darktable rendered it, and as the camera's look rendered it — plus a tone curve. (The
  other eleven styles are the panel's DR, exposure and shadow buttons, and carry no colour.)
- **The photographs they came from are there too.** A 24-patch ColorChecker shot in RAW on a
  Fujifilm X-Pro2 (`Target.RAF`, X-Trans) and a Ricoh GR III (`Target.DNG`), and each camera's
  in-camera conversion of that same RAW into its looks — the Fujifilm JPEGs carry the RAF's own
  capture timestamp. Plus the chart's layout, `color checker.cht`.

So the fit is ours, against our own rendering. Develop the chart through DV-1, measure the 24
patches, pair them with the same patches in the camera's JPEG, fit a smooth mapping — a tone curve
from the six greys, then a thin-plate spline in Lab, the method `darktable-chart` uses — and bake it
into a `.cube`. The fit lives in `rollchrome-core` with a thin example to run it, so the looks can
be regenerated whenever the base tone changes. The source photographs (50 MB of RAW, 110 MB of
JPEG) stay out of this repository; the `.cube` files, about 1 MB each at 33³, go in.

That gives **14 looks with full source material**: Fujifilm's Provia, Velvia, Astia, Classic
Chrome, Pro Neg Hi, Pro Neg Std and Eterna; Ricoh's Standard, Vivid, Positive Film, Negative Film,
Retro, Bleach Bypass and Cross Processing.

**Four more Fujifilm looks exist only as styles** — Classic Neg, Eterna Bleach Bypass, Nostalgic Neg
and Reala Ace. There is no camera JPEG for them, only the target values inside their styles. DV-3
decides whether those can be fitted honestly; they are not promised.

**Names.** Velvia, Provia, Astia, Classic Chrome, Eterna and Pro Neg are Fujifilm's trademarks —
the first three also its film stocks — and these looks imitate Fujifilm's *digital* simulations,
not the films. Fine for the owner's own use; renamed descriptively before Rollchrome is ever
distributed.

### An idea, not a step: looks from the owner's own film

The README of that repository points out that source and target can come from different cameras.
The owner shoots film and has it scanned by a lab. A chart photographed in RAW on a digital camera
*and* on a roll of Ektachrome, the roll scanned, gives the same fit a real film as its target — an
Ektachrome look measured from Ektachrome, rather than from somebody's imitation of it. Recorded so
it is not lost (G11); nothing is built for it.

## Filmulator — an idea, not code

[Filmulator](https://github.com/CarVac/filmulator-gui) (owner, 2026-11-05, "can also be of
inspiration") does what the looks cannot: it simulates **developing** the film. Exposure activates
crystals; developer is consumed fastest where the image is bright, compressing highlights across the
frame; it diffuses within the emulsion, so a small bright area darkens its surroundings (local
contrast, the adjacency effect); and periodic agitation mixes the layer with a reservoir of fresh
developer. Its controls are few — film size, drama, highlight roll-off.

**It is GPL-3.0-or-later.** This repository has no licence, and Rollchrome may one day go to the App
Store, so **no Filmulator code comes in, not even translated**. The description above is the
specification; DV-4 works from it and not from the C++.

It lends Develop two more things. Its order of operations — demosaic, highlight recovery, white
balance and exposure, development, black and white points, curves — confirms the pipeline above.
And its export queue, where each photograph keeps its own settings until it is exported, is what "a
look per photograph" needs.

A development and a look are different things and combine: the development shapes tone, the look
shapes colour. The catch is that the built-in looks are fitted against the base tone, so a
development stage in front of them changes what they receive. DV-4 settles that before it is built.

---

## Steps

### `DV-1` · The neutral development, in `rollchrome-core`

Needs RC-1. A function in `rollchrome-core`: a RAW file and an exposure offset in, a 16-bit image
out — pipeline steps 1–3. Never the embedded preview. X-Trans through an X-Trans demosaic (rawler's
`xtrans_markenstein`, called directly), Bayer through PPG or better.

**Leaves alone** F14 and `media::raw` in `phototools-core` (publishing wants the camera's look and
has it), and the RAW path of Edit and Batch Grade (open question 1).

Acceptance, in outline — the brief sets the numbers, after measuring:

- **The repository has no RAW files.** Tests build small synthetic DNGs in code, Bayer and X-Trans,
  with a known grey and known colour patches — as `media/raw.rs`'s tests already build TIFFs by
  hand — and assert the developed values.
- The output is genuinely 16-bit: a smooth gradient keeps more than 256 distinct levels.
- Clipped highlights stay neutral rather than turning magenta (see below).
- Time per photograph measured on the Mac for a 24 MP and a 40 MP-plus RAW, and recorded; any
  budget comes from that measurement.
- A manual check (MV) on the owner's own files against the camera's JPEG: similar colour, no camera
  look, no maze pattern on X-Trans.

### `DV-2` · Develop, the function

Needs DV-1; its commands need RC-2. `tools::develop` in `rollchrome-core`: a folder, and for each
photograph a look and an exposure offset, give a plan; executing the plan gives the outputs and a
report, every skipped file with its reason (G10). Looks from the LUT library; JPEG or TIFF; never
overwrites; metadata as in step 6. Tauri commands in `rollchrome-desktop`, transport only (G1). **No
screen** — the function is exercised by tests and the commands.

### `DV-3` · The built-in looks

Needs DV-1. The fit, the 14 `.cube` files, and how they ship inside the application, so that they
need no import into the library. Acceptance: applied to the developed chart, each look lands within
a ΔE of the camera's JPEG on the 24 patches — the number set in the brief once a first fit shows
what is achievable, not guessed beforehand.

### `DV-4` · Film development, after Filmulator — only if the owner wants it

The model described above, as a stage between exposure and base tone, with its few controls, and
an answer to how it sits with the fitted looks. Not started without the owner's say-so.

### The screen — not a step yet

To be discussed with the owner once DV-2 shows what the function needs from it.

---

## Things that will catch you

- **Clipped highlights turn magenta.** White balance scales the channels unequally, so a highlight
  that clipped on the sensor stops being white once balanced. The camera's JPEG hides this; a naive
  development shows it in every sky. DV-1 handles it, and a test proves it.
- **Lens corrections.** A camera's JPEG is corrected for its lens. Whether rawler applies the
  corrections a RAW carries (DNG opcode lists, Fujifilm's correction data) has not been checked:
  DV-1 finds out and records it. For DV-3 it means the patch boxes are placed on each image
  separately — the JPEG's geometry is not the RAW's.
- **The looks are fitted to the base tone.** Change step 3 and every built-in look is off until
  DV-3's fit is re-run. A test pins the base tone, so that the change cannot happen quietly.

## Open questions for the owner

1. **Edit on a RAW today edits the camera's embedded JPEG** — 8-bit, camera look included. Should
   Edit move onto DV-1's development, so that a photograph looks the same in Edit as in Develop's
   output? Recommended: yes, as its own step after DV-1. Batch Grade stays as it is.
2. **Where do outputs go, named how?** Default: a `Developed` folder beside the RAW files,
   `<name>_<look>.jpg` or `.tif`, never overwriting.
3. **The TIFF's colour space.** Default 16-bit sRGB; a wider space if the TIFFs are for further
   editing elsewhere.
4. **DV-4 at all?**
