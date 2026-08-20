<script setup lang="ts">
/**
 * F4, F7 and F8 — the tools that take files and an output directory.
 *
 * §8 exposes no dry-run for these, so the gate is an explicit confirmation
 * naming exactly what will be written and where.
 */
import { computed, ref, useTemplateRef } from 'vue';
import type {
  BorderStyle,
  CanvasSizing,
  SplitPreview,
  SplitSettings,
} from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import PathListField from '../components/PathListField.vue';
import PathField from '../components/PathField.vue';
import { useRoots } from '../useRoots';

const props = defineProps<{
  operation: 'split' | 'border' | 'tiffToJpeg';
  title: string;
  blurb: string;
  applyLabel: string;
}>();

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const inputs = ref('');
const outDir = ref('');
const recursive = ref(false);
const confirmed = ref(false);
const busy = ref(false);

// The folders the pickers may offer, and the lister they walk with.
const { roots } = useRoots();
const list = (path: string) => api.list(path);

const inputList = computed(() =>
  inputs.value.split('\n').map((p) => p.trim()).filter(Boolean),
);

/**
 * F4's thresholds, exposed only for the split.
 *
 * The specification's defaults. They have only ever met synthetic fixtures
 * (MV-4.1), and a real lab scan has a soft divider, grain and an uneven
 * surround — so the point of showing them is that they can be moved when the
 * preview says the divider landed somewhere silly.
 */
const settings = ref<SplitSettings>({
  threshold_dark: 25,
  threshold_white: 235,
  border_tol: 0.92,
  max_crop_pct: 0.12,
  margin: 0.2,
  window: 20,
  ratio: 24 / 17,
});

const preview = ref<SplitPreview | null>(null);

/**
 * F7's one parameter, on by default as the tool is.
 *
 * MV-4.2 asks whether a genuinely dark photograph is mistaken for a scan
 * border. Until now there was nothing to turn off when it was.
 */
const trimDarkEdges = ref(true);

/**
 * F8's size and quality, seeded with §F8's defaults.
 *
 * 2048 is a distributable size for scanner output — and throws away 92% of a
 * 36 MP camera TIFF, which was not a choice anybody could make until now.
 */
/**
 * F7's canvas, seeded with §F7's fixed appearance.
 *
 * The specification fixes all of this, and the point of fixing it was that a
 * set of prints looks like a set. These are choices now — the consistency is
 * the operator's to keep or spend.
 */
const borderSizing = ref<CanvasSizing>('FixedCanvas');
const borderColour = ref('#ffffff');
const borderCanvasWidth = ref(3000);
const borderMargin = ref(50);
const borderRadiusPercent = ref(2);

/** `#rrggbb` to the bytes `core` takes. */
function hexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace('#', '');
  return [
    parseInt(clean.slice(0, 2), 16) || 0,
    parseInt(clean.slice(2, 4), 16) || 0,
    parseInt(clean.slice(4, 6), 16) || 0,
  ];
}

function borderStyle(): BorderStyle {
  return {
    sizing: borderSizing.value,
    canvas_colour: hexToRgb(borderColour.value),
    canvas_width: borderCanvasWidth.value,
    min_margin: borderMargin.value,
    // Shown as a percentage because that is how §F7 states it; sent as the
    // fraction the tool works in.
    corner_radius_fraction: borderRadiusPercent.value / 100,
  };
}

const tiffMaxLongEdge = ref(2048);
const tiffQuality = ref(90);

/**
 * Preview the first frame this run would process.
 *
 * The whole input list is sent, not the first line: a line may be a folder,
 * and it is the tool that knows which files inside it it would take.
 */
async function previewSplit() {
  if (!inputList.value.length) {
    page.value?.setFailure('Add an input to preview.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    preview.value = await api.splitPreview({
      inputs: inputList.value,
      recursive: recursive.value,
      settings: settings.value,
    });
    // Seeing where the divider landed *is* the review this tool needs.
    confirmed.value = true;
    page.value?.setReviewed(true);
  } catch (e) {
    preview.value = null;
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

/** A divider far from the middle is the tell that it found a dark picture. */
const dividerLooksWrong = computed(() => {
  const f = preview.value?.divider_fraction;
  return f !== undefined && (f < 0.35 || f > 0.65);
});

async function apply() {
  if (!inputList.value.length) {
    page.value?.setFailure('Add at least one input.');
    return;
  }
  if (!outDir.value.trim()) {
    page.value?.setFailure('Choose an output directory.');
    return;
  }

  busy.value = true;
  page.value?.setFailure(null);
  const body = {
    inputs: inputList.value,
    recursive: recursive.value,
    out_dir: outDir.value.trim(),
  };

  try {
    const start =
      props.operation === 'split'
        ? api.split({ ...body, settings: settings.value })
        : props.operation === 'border'
          ? api.border({
              ...body,
              trim_dark_edges: trimDarkEdges.value,
              style: borderStyle(),
            })
          : api.tiffToJpeg({
              ...body,
              max_long_edge: tiffMaxLongEdge.value,
              quality: tiffQuality.value,
            });
    page.value?.setJob(await start);
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

function confirm() {
  // The split has a real preview to look at; the others have only this.
  if (props.operation === 'split') {
    void previewSplit();
    return;
  }
  confirmed.value = true;
  page.value?.setReviewed(true);
}
</script>

<template>
  <ToolPage
    ref="page"
    :title="props.title"
    :blurb="props.blurb"
    has-preview
    :apply-label="props.applyLabel"
    :busy="busy"
    @preview="confirm"
    @apply="apply"
  >
    <template #form>
      <PathListField
        v-model="inputs"
        label="Inputs — files or folders, one per line"
        :roots="roots"
        :list="list"
      />

      <PathField
        v-model="outDir"
        label="Output directory"
        placeholder="/mnt/photos/out"
        :roots="roots"
        :list="list"
      />

      <label class="checkbox"><input v-model="recursive" type="checkbox" /> Include subfolders</label>

      <fieldset v-if="props.operation === 'tiffToJpeg'" class="field settings">
        <legend>Output size and quality</legend>
        <p class="muted">
          §F8's defaults suit scanner output being sent somewhere. A 36 MP camera TIFF reduced to
          2048 px keeps about 8% of its pixels — raise the long edge to keep more.
        </p>
        <div class="settings__grid">
          <label class="field">
            <span>Longest edge (px)</span>
            <input v-model.number="tiffMaxLongEdge" type="number" min="1" />
          </label>
          <label class="field">
            <span>JPEG quality</span>
            <input v-model.number="tiffQuality" type="number" min="1" max="100" />
          </label>
        </div>
      </fieldset>

      <label v-if="props.operation === 'border'" class="checkbox">
        <input v-model="trimDarkEdges" type="checkbox" />
        Trim dark scan edges first
      </label>
      <p v-if="props.operation === 'border'" class="muted">
        A side is trimmed while more than 70% of a sampled band falls below luma 28, up to 40 px.
        Turn it off for a photograph that is genuinely dark at the edges.
      </p>

      <fieldset v-if="props.operation === 'border'" class="field settings">
        <legend>Canvas</legend>

        <label class="radio">
          <input v-model="borderSizing" type="radio" value="FixedCanvas" />
          Fixed canvas — every output the same size and shape, the photograph scaled to fit
        </label>
        <label class="radio">
          <input v-model="borderSizing" type="radio" value="ImagePlusMargin" />
          Image plus margin — the photograph at its own size, the canvas grown around it
        </label>

        <p class="muted">
          <template v-if="borderSizing === 'FixedCanvas'">
            §F7's canvas: 4:5 for portrait, 5:4 for landscape, so a set looks like a set and a feed
            cannot crop it unpredictably. The photograph is rescaled to fit — a 36 MP frame on a
            3000 px canvas comes out near 6 MP, and a smaller one is enlarged.
          </template>
          <template v-else>
            Nothing is rescaled, so nothing is lost, and the shape follows the photograph. The
            canvas width is ignored — the output is the image plus the margin on every side.
          </template>
        </p>
        <div class="settings__grid">
          <label class="field">
            <span>Colour</span>
            <input v-model="borderColour" type="color" class="colour" />
          </label>
          <label v-if="borderSizing === 'FixedCanvas'" class="field">
            <span>Canvas width (px)</span>
            <input v-model.number="borderCanvasWidth" type="number" min="1" />
          </label>
          <label class="field">
            <span>Margin (px)</span>
            <input v-model.number="borderMargin" type="number" min="0" />
          </label>
          <label class="field">
            <span>Corner radius (%)</span>
            <input v-model.number="borderRadiusPercent" type="number" min="0" max="50" step="0.5" />
          </label>
        </div>
      </fieldset>

      <fieldset v-if="props.operation === 'split'" class="field settings">
        <legend>Split settings</legend>
        <p class="muted">
          The specification's defaults. Move them when the preview shows the divider in the wrong
          place — a lab scan with a soft divider or a heavy border may need it.
        </p>
        <div class="settings__grid">
          <label class="field">
            <span>Frame ratio (height ÷ width)</span>
            <input v-model.number="settings.ratio" type="number" step="0.01" />
          </label>
          <label class="field">
            <span>Dark threshold</span>
            <input v-model.number="settings.threshold_dark" type="number" min="0" max="255" />
          </label>
          <label class="field">
            <span>White threshold</span>
            <input v-model.number="settings.threshold_white" type="number" min="0" max="255" />
          </label>
          <label class="field">
            <span>Border tolerance</span>
            <input v-model.number="settings.border_tol" type="number" step="0.01" min="0" max="1" />
          </label>
          <label class="field">
            <span>Max crop per side</span>
            <input v-model.number="settings.max_crop_pct" type="number" step="0.01" min="0" max="1" />
          </label>
          <label class="field">
            <span>Search margin</span>
            <input v-model.number="settings.margin" type="number" step="0.01" min="0" max="0.49" />
          </label>
          <label class="field">
            <span>Refine window (px)</span>
            <input v-model.number="settings.window" type="number" min="0" />
          </label>
        </div>
      </fieldset>
    </template>

    <template #preview>
      <section v-if="preview" class="split-preview">
        <h2 class="split-preview__head">
          // DIVIDER AT {{ preview.divider_x }} PX //
          {{ Math.round(preview.divider_fraction * 100) }}% ACROSS
        </h2>
        <p class="muted" :title="preview.source">
          Previewing {{ preview.source.split('/').pop() }} — the first frame this run would take.
        </p>

        <p v-if="dividerLooksWrong" class="error">
          That is a long way from the middle. A half-frame pair puts the divider near 50%, so this
          is more likely a dark part of the picture than the gap between frames — raise the search
          margin, or the dark threshold.
        </p>

        <figure class="split-preview__whole">
          <img :src="preview.cropped.src" alt="The scan with its lab border removed" />
          <figcaption class="muted">
            Border removed — {{ preview.cropped.width }}×{{ preview.cropped.height }}
          </figcaption>
        </figure>

        <div class="split-preview__halves">
          <figure>
            <img :src="preview.a.src" alt="The left half" />
            <figcaption class="muted">A — {{ preview.a.width }}×{{ preview.a.height }}</figcaption>
          </figure>
          <figure>
            <img :src="preview.b.src" alt="The right half" />
            <figcaption class="muted">B — {{ preview.b.width }}×{{ preview.b.height }}</figcaption>
          </figure>
        </div>

        <p class="muted">Nothing has been written. Apply writes both halves into the output folder.</p>
      </section>

      <div v-if="confirmed && inputList.length && props.operation !== 'split'" class="confirmed">
        <p>
          Will write output from the <strong>{{ inputList.length }}</strong>
          path{{ inputList.length === 1 ? '' : 's' }} listed into
          <code>{{ outDir }}</code>. Originals are never modified.
        </p>
        <small class="muted">A folder counts as one path here; it contributes the files inside it, and subfolders only when "Include subfolders" is ticked.</small>
      </div>
    </template>
  </ToolPage>
</template>

<style scoped>
/* A colour well, not a text field: it is the one input here that is a value
   rather than a number, and it should look like one. */
.colour {
  min-height: 44px;
  padding: var(--space-1);
  cursor: pointer;
}

.settings__grid {
  display: grid;
  gap: var(--space-3);
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
}
.settings > .muted {
  margin-bottom: var(--space-2);
}

.split-preview {
  display: grid;
  gap: var(--space-3);
}
.split-preview__head {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.split-preview figure {
  margin: 0;
  display: grid;
  gap: var(--space-1);
}
.split-preview img {
  width: 100%;
  height: auto;
  border: var(--border-hair);
  /* The scan is the thing being judged; a background behind it would change
     how its own edges read. */
  background: var(--bg-panel);
}
.split-preview__halves {
  display: grid;
  gap: var(--space-3);
  grid-template-columns: 1fr 1fr;
}
.confirmed {
  display: grid;
  gap: var(--space-1);
  border-left: 3px solid var(--accent);
  padding: 8px 12px;
  background: var(--bg-panel);
  border-radius: 0 8px 8px 0;
  font-size: 14px;
}
code { font-family: var(--font-body); word-break: break-all; }
</style>
