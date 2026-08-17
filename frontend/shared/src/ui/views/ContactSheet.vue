<script setup lang="ts">
/** F5 — contact sheet. Writes one file, so it takes a path rather than a folder. */
import { computed, ref, useTemplateRef } from 'vue';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const inputs = ref('');
const outPath = ref('');
const recursive = ref(false);
const confirmed = ref(false);
const busy = ref(false);

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
      <label class="field">
        <span>Inputs — files or folders, one per line</span>
        <textarea v-model="inputs" rows="4" spellcheck="false"></textarea>
      </label>
      <label class="field">
        <span>Output file</span>
        <input v-model="outPath" type="text" placeholder="/mnt/photos/contact.jpg" />
      </label>
      <label class="checkbox"><input v-model="recursive" type="checkbox" /> Include subfolders</label>
    </template>

    <template #preview>
      <p v-if="confirmed && inputList.length" class="confirmed">
        Will build one sheet from <strong>{{ inputList.length }}</strong> input(s) at
        <code>{{ outPath }}</code>.
      </p>
    </template>
  </ToolPage>
</template>

<style scoped>
.confirmed {
  border-left: 3px solid var(--accent);
  padding: 8px 12px;
  background: var(--surface-2);
  border-radius: 0 8px 8px 0;
  font-size: 0.9rem;
}
code { font-family: var(--mono); word-break: break-all; }
</style>
