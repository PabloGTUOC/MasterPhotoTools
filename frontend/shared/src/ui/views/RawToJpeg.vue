<script setup lang="ts">
/**
 * RAW to JPEG (F14) — pointed at a folder.
 *
 * This used to be a button on the card screen, and it was the only way in the
 * whole application to get a JPEG out of a RAW-only shot. WF-3 of
 * `docs/workflow-plan.md` moves it here, because Ingest reports and the tools
 * act — and because by the time you want a JPEG the photographs are in a
 * folder, not on the card.
 *
 * Shared rather than desktop-only: the work is a decode and an encode, and both
 * transports genuinely do it. `deriveRaw` is on `ApiClient` for that reason.
 *
 * **It writes beside nothing and over nothing.** The derived JPEGs land in the
 * output folder; the RAW files are read and left alone.
 */
import { ref, useTemplateRef } from 'vue';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import PathField from '../components/PathField.vue';
import { useRoots } from '../useRoots';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const source = ref('');
const outDir = ref('');
const busy = ref(false);

/**
 * F12's two ceilings, for this run.
 *
 * A derivative faces the same limits as a camera JPEG would, so they are asked
 * for here rather than inherited silently. Zero megapixels means no resolution
 * ceiling at all — publishing is limited by file size, and a 40 MP frame inside
 * the byte cap is worth keeping whole.
 */
const maxMegapixels = ref(0);
const maxOutputMb = ref(10);

const { roots } = useRoots();
const list = (path: string) => api.list(path);

async function apply() {
  const path = source.value.trim();
  const out = outDir.value.trim();

  if (!path) {
    page.value?.setFailure('Choose the folder holding the RAW files.');
    return;
  }
  if (!out) {
    page.value?.setFailure('Choose an output folder — the RAW files are never written over.');
    return;
  }

  busy.value = true;
  page.value?.setFailure(null);
  try {
    page.value?.setJob(
      await api.deriveRaw({
        path,
        out_dir: out,
        thresholds: {
          max_megapixels: maxMegapixels.value,
          max_output_bytes: Math.round(maxOutputMb.value * 1024 * 1024),
        },
      }),
    );
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <ToolPage
    ref="page"
    title="RAW to JPEG"
    blurb="Produce a JPEG for every RAW that has none beside it. A shot with a JPEG already is left alone; the RAW files are read and never written over."
    apply-label="Derive JPEGs"
    :busy="busy"
    @apply="apply"
  >
    <template #form>
      <PathField
        v-model="source"
        label="Folder holding the RAW files"
        placeholder="/mnt/photos/2026/berlin"
        :roots="roots"
        :list="list"
      />

      <PathField
        v-model="outDir"
        label="Output folder"
        placeholder="/mnt/photos/2026/berlin/derived"
        hint="The JPEGs land here. Nothing is written over."
        :roots="roots"
        :list="list"
      />

      <fieldset class="field limits">
        <legend>Limits for the derived JPEGs</legend>
        <p class="muted">
          A derivative faces the same ceilings a camera JPEG would. They are asked for here rather
          than inherited quietly.
        </p>
        <div class="limits__grid">
          <label class="field">
            <span>Size (MB)</span>
            <input v-model.number="maxOutputMb" type="number" min="1" step="1" />
          </label>
          <label class="field">
            <span>Resolution (MP)</span>
            <input v-model.number="maxMegapixels" type="number" min="0" />
            <small class="muted">0 means no limit — the frame is kept whole.</small>
          </label>
        </div>
      </fieldset>
    </template>
  </ToolPage>
</template>

<style scoped>
.limits__grid {
  display: grid;
  gap: var(--space-3);
  grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
  /* Each field is label, input, hint. Without this the shorter of the two
     stretches its input to fill the taller row and they stop lining up. */
  align-items: start;
}
.limits__grid .field > span {
  /* The *label* can wrap too, and a wrapped label pushes its input down while
     the other stays put. The earlier fix on the card screen reserved the hint's
     line and not this one, because that screen is never measured at 390 px. */
  min-height: 2.6em;
}
.limits__grid .field > small {
  /* Reserve the line whether or not the hint wraps, so the inputs stay on one
     baseline at any width. */
  min-height: 2.4em;
}
.limits > .muted {
  margin-bottom: var(--space-2);
}
</style>
