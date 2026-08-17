<script setup lang="ts">
/** F3 — batch rename, with the plan shown before anything moves. */
import { ref, useTemplateRef } from 'vue';
import type { Plan, RenameAction, RenameOrder } from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const paths = ref('');
const date = ref('');
const subject = ref('');
const camera = ref('');
const film = ref('');
const order = ref<RenameOrder>('Capture');
const plan = ref<Plan<RenameAction> | null>(null);
const busy = ref(false);

function request() {
  return {
    paths: paths.value.split('\n').map((p) => p.trim()).filter(Boolean),
    date: date.value || null,
    subject: subject.value || null,
    camera: camera.value || null,
    film: film.value || null,
    order: order.value,
  };
}

async function preview() {
  const body = request();
  if (!body.paths.length) {
    page.value?.setFailure('Add at least one path.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    plan.value = await api.planRename(body);
    page.value?.setReviewed(true);
  } catch (e) {
    plan.value = null;
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

async function apply() {
  busy.value = true;
  try {
    page.value?.setJob(await api.applyRename(request()));
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

const leaf = (p: string) => p.split('/').pop() ?? p;
</script>

<template>
  <ToolPage
    ref="page"
    title="Rename"
    blurb="Rename to a consistent, sortable scheme. Files already at a target name are skipped, never overwritten."
    has-preview
    apply-label="Apply renames"
    :busy="busy"
    @preview="preview"
    @apply="apply"
  >
    <template #form>
      <label class="field">
        <span>Paths, one per line</span>
        <textarea v-model="paths" rows="4" spellcheck="false"></textarea>
      </label>

      <div class="prefix-grid">
        <label class="field"><span>Date</span><input v-model="date" type="text" placeholder="2024-05-01" /></label>
        <label class="field"><span>Subject</span><input v-model="subject" type="text" placeholder="Lisboa" /></label>
        <label class="field"><span>Camera</span><input v-model="camera" type="text" placeholder="PENTAX17" /></label>
        <label class="field"><span>Film</span><input v-model="film" type="text" placeholder="PORTRA400" /></label>
      </div>

      <label class="field">
        <span>Order</span>
        <select v-model="order">
          <option value="Capture">Capture date</option>
          <option value="Numeric">Number in filename</option>
        </select>
      </label>
    </template>

    <template #preview>
      <div v-if="plan" class="plan">
        <h2>{{ plan.actions.length }} to rename, {{ plan.skipped.length }} skipped</h2>

        <div class="table-scroll">
          <table>
            <thead><tr><th>From</th><th>To</th></tr></thead>
            <tbody>
              <tr v-for="action in plan.actions" :key="action.source">
                <td>{{ leaf(action.source) }}</td>
                <td>{{ leaf(action.target) }}</td>
              </tr>
            </tbody>
          </table>
        </div>

        <ul v-if="plan.skipped.length" class="skipped">
          <li v-for="skip in plan.skipped" :key="skip.file">
            <strong>{{ leaf(skip.file) }}</strong> — {{ skip.reason }}
          </li>
        </ul>
      </div>
    </template>
  </ToolPage>
</template>

<style scoped>
.prefix-grid { display: grid; gap: 10px; grid-template-columns: 1fr; }
@media (min-width: 34rem) { .prefix-grid { grid-template-columns: 1fr 1fr; } }
.plan { display: grid; gap: 10px; }
.plan h2 { font-size: 1rem; }
.table-scroll { overflow-x: auto; border: 1px solid var(--rule); border-radius: 8px; }
table { border-collapse: collapse; width: 100%; font-family: var(--mono); font-size: 0.82rem; }
th, td { text-align: left; padding: 7px 10px; border-bottom: 1px solid var(--rule); white-space: nowrap; }
th { color: var(--ink-soft); font-weight: 600; }
tr:last-child td { border-bottom: none; }
.skipped { list-style: none; display: grid; gap: 4px; font-size: 0.85rem; color: var(--ink-soft); }
</style>
