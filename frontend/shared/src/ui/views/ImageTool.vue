<script setup lang="ts">
/**
 * F4, F7 and F8 — the tools that take files and an output directory.
 *
 * §8 exposes no dry-run for these, so the gate is an explicit confirmation
 * naming exactly what will be written and where.
 */
import { computed, ref, useTemplateRef } from 'vue';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';

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

const inputList = computed(() =>
  inputs.value.split('\n').map((p) => p.trim()).filter(Boolean),
);

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
        ? api.split(body)
        : props.operation === 'border'
          ? api.border(body)
          : api.tiffToJpeg(body);
    page.value?.setJob(await start);
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

function confirm() {
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
      <label class="field">
        <span>Inputs — files or folders, one per line</span>
        <textarea v-model="inputs" rows="4" spellcheck="false"></textarea>
      </label>

      <label class="field">
        <span>Output directory</span>
        <input v-model="outDir" type="text" placeholder="/mnt/photos/out" />
      </label>

      <label class="checkbox"><input v-model="recursive" type="checkbox" /> Include subfolders</label>
    </template>

    <template #preview>
      <p v-if="confirmed && inputList.length" class="confirmed">
        Will write output from <strong>{{ inputList.length }}</strong>
        input{{ inputList.length === 1 ? '' : 's' }} into
        <code>{{ outDir }}</code>. Originals are never modified.
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
