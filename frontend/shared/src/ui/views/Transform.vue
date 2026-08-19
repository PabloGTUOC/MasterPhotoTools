<script setup lang="ts">
/** F6 — rotate, resize, convert. */
import { computed, ref, useTemplateRef } from 'vue';
import type { TargetFormat } from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import PathListField from '../components/PathListField.vue';
import PathField from '../components/PathField.vue';
import { useRoots } from '../useRoots';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const inputs = ref('');
const outDir = ref('');
const recursive = ref(false);
const rotate = ref('');
const maxLongEdge = ref('');
const format = ref<TargetFormat | ''>('');
const quality = ref('95');
const confirmed = ref(false);
const busy = ref(false);

// The folders the pickers may offer, and the lister they walk with.
const { roots } = useRoots();
const list = (path: string) => api.list(path);

const inputList = computed(() =>
  inputs.value.split('\n').map((p) => p.trim()).filter(Boolean),
);

function numberOrNull(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : null;
}

async function apply() {
  if (!inputList.value.length || !outDir.value.trim()) {
    page.value?.setFailure('Add inputs and an output directory.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    page.value?.setJob(
      await api.transform({
        inputs: inputList.value,
        recursive: recursive.value,
        out_dir: outDir.value.trim(),
        rotate_degrees: numberOrNull(rotate.value),
        max_long_edge: numberOrNull(maxLongEdge.value),
        format: format.value === '' ? null : format.value,
        quality: numberOrNull(quality.value),
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
    title="Transform"
    blurb="Rotate, resize and convert. Resizing only ever shrinks — an image within the limit is left alone."
    has-preview
    apply-label="Transform"
    :busy="busy"
    @preview="() => { confirmed = true; page?.setReviewed(true); }"
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

      <div class="options">
        <label class="field"><span>Rotate (degrees)</span><input v-model="rotate" type="text" inputmode="numeric" placeholder="0" /></label>
        <label class="field"><span>Max long edge (px)</span><input v-model="maxLongEdge" type="text" inputmode="numeric" placeholder="2048" /></label>
        <label class="field">
          <span>Format</span>
          <select v-model="format">
            <option value="">Keep source format</option>
            <option value="Jpeg">JPEG</option>
            <option value="Png">PNG</option>
            <option value="Tiff">TIFF</option>
            <option value="WebP">WebP</option>
          </select>
        </label>
        <label class="field"><span>Quality</span><input v-model="quality" type="text" inputmode="numeric" /></label>
      </div>

      <label class="checkbox"><input v-model="recursive" type="checkbox" /> Include subfolders</label>
    </template>

    <template #preview>
      <p v-if="confirmed && inputList.length" class="confirmed">
        Will transform <strong>{{ inputList.length }}</strong> input(s) into
        <code>{{ outDir }}</code>.
      </p>
    </template>
  </ToolPage>
</template>

<style scoped>
.options { display: grid; gap: 10px; grid-template-columns: 1fr; }
@media (min-width: 34rem) { .options { grid-template-columns: 1fr 1fr; } }
.confirmed {
  border-left: 3px solid var(--accent);
  padding: 8px 12px;
  background: var(--bg-panel);
  border-radius: 0 8px 8px 0;
  font-size: 14px;
}
code { font-family: var(--font-body); word-break: break-all; }
</style>
