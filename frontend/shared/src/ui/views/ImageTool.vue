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
  background: var(--bg-panel);
  border-radius: 0 8px 8px 0;
  font-size: 14px;
}
code { font-family: var(--font-body); word-break: break-all; }
</style>
