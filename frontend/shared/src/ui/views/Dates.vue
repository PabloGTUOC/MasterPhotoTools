<script setup lang="ts">
/** F1 and F2 — scan and repair capture dates. */
import { ref, useTemplateRef } from 'vue';
import type { RepairMode } from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import PathListField from '../components/PathListField.vue';
import { useRoots } from '../useRoots';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const paths = ref('');
const recursive = ref(true);
const mode = ref<'Auto' | 'Manual' | 'Shift' | 'Sidecar'>('Auto');
const manualDate = ref('');
const shiftDelta = ref('+0:0:0 0:0:0');
const busy = ref(false);

// The folders the pickers may offer, and the lister they walk with.
const { roots } = useRoots();
const list = (path: string) => api.list(path);

function pathList(): string[] {
  return paths.value
    .split('\n')
    .map((p) => p.trim())
    .filter(Boolean);
}

function repairMode(): RepairMode {
  switch (mode.value) {
    case 'Manual':
      return { Manual: manualDate.value };
    case 'Shift':
      return { Shift: shiftDelta.value };
    case 'Sidecar':
      return 'Sidecar';
    default:
      return 'Auto';
  }
}

async function run(dryRun: boolean) {
  const list = pathList();
  if (!list.length) {
    page.value?.setFailure('Add at least one path.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    const id = await api.fixDates({ paths: list, mode: repairMode(), dry_run: dryRun });
    page.value?.setJob(id);
    if (dryRun) page.value?.setReviewed(true);
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

async function scan() {
  const first = pathList()[0];
  if (!first) {
    page.value?.setFailure('Add a folder to scan.');
    return;
  }
  busy.value = true;
  try {
    page.value?.setJob(await api.scanDates({ path: first, recursive: recursive.value }));
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
    title="Dates"
    blurb="Repair wrong or missing capture dates. The preview reports what would change without writing anything."
    has-preview
    apply-label="Apply dates"
    :busy="busy"
    @preview="run(true)"
    @apply="run(false)"
  >
    <template #form>
      <PathListField
        v-model="paths"
        label="Paths, one per line"
        placeholder="/mnt/photos/2024/roll-01.jpg"
        :roots="roots"
        :list="list"
      />

      <fieldset class="field">
        <legend>Repair mode</legend>
        <label class="radio"><input v-model="mode" type="radio" value="Auto" /> Auto — best available metadata date</label>
        <label class="radio"><input v-model="mode" type="radio" value="Manual" /> Manual — a supplied date</label>
        <label class="radio"><input v-model="mode" type="radio" value="Shift" /> Shift — offset by a delta</label>
        <label class="radio"><input v-model="mode" type="radio" value="Sidecar" /> Sidecar — a Google Takeout JSON</label>
      </fieldset>

      <label v-if="mode === 'Manual'" class="field">
        <span>Date</span>
        <input v-model="manualDate" type="text" placeholder="2024-05-01T12:00:00" />
      </label>

      <label v-if="mode === 'Shift'" class="field">
        <span>Delta</span>
        <input v-model="shiftDelta" type="text" placeholder="+5:0:0 0:0:0" />
        <small class="muted">Years:months:days hours:minutes:seconds, signed.</small>
      </label>

      <div class="row">
        <label class="checkbox"><input v-model="recursive" type="checkbox" /> Recursive</label>
        <button type="button" class="secondary" :disabled="busy" @click="scan">Scan only</button>
      </div>
    </template>
  </ToolPage>
</template>

<style scoped>
.row { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
</style>
