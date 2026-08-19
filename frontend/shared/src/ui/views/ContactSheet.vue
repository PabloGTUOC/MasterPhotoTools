<script setup lang="ts">
/** F5 — contact sheet. Writes one file, so it takes a path rather than a folder. */
import { computed, ref, useTemplateRef } from 'vue';
import type { SheetStyle } from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import PathListField from '../components/PathListField.vue';
import PathField from '../components/PathField.vue';
import { useRoots } from '../useRoots';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const inputs = ref('');
const outPath = ref('');
const recursive = ref(false);
const style = ref<SheetStyle>('Filmstrip');
const confirmed = ref(false);
const busy = ref(false);

// The folders the pickers may offer, and the lister they walk with.
const { roots } = useRoots();
const list = (path: string) => api.list(path);

const inputList = computed(() =>
  inputs.value.split('\n').map((p) => p.trim()).filter(Boolean),
);

async function apply() {
  if (!inputList.value.length || !outPath.value.trim()) {
    page.value?.setFailure('Add inputs and an output file path.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    page.value?.setJob(
      await api.contactSheet({
        inputs: inputList.value,
        recursive: recursive.value,
        out_path: outPath.value.trim(),
        style: style.value,
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
    title="Contact sheet"
    blurb="A grid of thumbnails from a folder. A file that cannot be read gets a crossed box rather than aborting the sheet."
    has-preview
    apply-label="Build sheet"
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
        v-model="outPath"
        label="Output file"
        placeholder="/mnt/photos/contact.jpg"
        hint="Choosing a folder keeps the file name."
        keep-file-name
        file-name-fallback="contact.jpg"
        :roots="roots"
        :list="list"
      />
      <fieldset class="field">
        <legend>Layout</legend>
        <label class="radio">
          <input v-model="style" type="radio" value="Filmstrip" />
          Film strip — frames on 35mm, five to a strip, numbered on the rebate
        </label>
        <label class="radio">
          <input v-model="style" type="radio" value="Grid" />
          Grid — uniform cells with filename captions
        </label>
      </fieldset>

      <label class="checkbox"><input v-model="recursive" type="checkbox" /> Include subfolders</label>
    </template>

    <template #preview>
      <div v-if="confirmed && inputList.length" class="confirmed">
        <p>
          Will build one sheet from the <strong>{{ inputList.length }}</strong>
          path{{ inputList.length === 1 ? '' : 's' }} listed, at
          <code>{{ outPath }}</code>.
        </p>
        <small class="muted">A folder counts as one path here; it contributes the files inside it, and subfolders only when "Include subfolders" is ticked.</small>
      </div>
    </template>
  </ToolPage>
</template>

<style scoped>
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
