<script setup lang="ts">
/** F1 and F2 — scan and repair capture dates. */
import { ref, useTemplateRef } from 'vue';
import type { DateRepairAction, Plan, RepairMode, ScanResult } from '@phototools/shared';
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

/** The last scan's rows. Empty until one has run; `null` while none has. */
const scanned = ref<ScanResult[] | null>(null);

/** What the last preview said would change. */
const plan = ref<Plan<DateRepairAction> | null>(null);

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

/** The request both the preview and the apply are built from. */
function request(dryRun: boolean) {
  return {
    paths: pathList(),
    mode: repairMode(),
    dry_run: dryRun,
    // The same checkbox the scan uses. Sending it only on the scan meant a
    // ticked box did nothing to the operation that actually writes.
    recursive: recursive.value,
  };
}

/**
 * Show what would change, per file, without changing it.
 *
 * A plan rather than a job: the preview writes nothing, and its value is the
 * detail. It used to run as a job that reported only "N would be redated",
 * which is not enough to judge a clock offset before applying it (MV-9.3).
 */
async function preview() {
  if (!pathList().length) {
    page.value?.setFailure('Add at least one path.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    plan.value = await api.planDates(request(true));
    page.value?.setReviewed(true);
  } catch (e) {
    plan.value = null;
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

async function apply() {
  if (!pathList().length) {
    page.value?.setFailure('Add at least one path.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    page.value?.setJob(await api.fixDates(request(false)));
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
  page.value?.setFailure(null);
  try {
    scanned.value = await api.scanDates({ path: first, recursive: recursive.value });
  } catch (e) {
    scanned.value = null;
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

/** How many rows are anything other than settled. */
function needingAttention(rows: ScanResult[]): number {
  return rows.filter((r) => r.status !== 'Ok').length;
}

/** The status as a word, and as a mark that does not depend on colour. */
function statusMark(status: ScanResult['status']): string {
  if (status === 'Ok') return '✓';
  return status === 'Mismatch' ? '!' : '✕';
}

function statusWord(status: ScanResult['status']): string {
  if (status === 'Ok') return 'ok';
  return status === 'Mismatch' ? 'mismatch' : 'no metadata';
}

/** A date as the terminal shows it, or a dash where there is none. */
function stamp(value: string | null): string {
  return value ? value.replace('T', ' ').slice(0, 19) : '—';
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
    @preview="preview"
    @apply="apply"
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

    <template #preview>
      <section v-if="plan" class="scan" aria-live="polite">
        <h2 class="scan__head">
          // {{ plan.actions.length }} WOULD BE REDATED // {{ plan.skipped.length }} SKIPPED
        </h2>

        <p v-if="!plan.actions.length" class="muted">
          Nothing would change. Every file already carries the date this mode resolves to.
        </p>

        <div v-else class="scan__table">
          <div class="scan__row scan__row--plan scan__row--head" role="row">
            <span role="columnheader">File</span>
            <span role="columnheader">Would become</span>
            <span role="columnheader">Shift</span>
          </div>
          <div
            v-for="action in plan.actions"
            :key="action.path"
            class="scan__row scan__row--plan"
            role="row"
          >
            <span class="scan__name" :title="action.path">{{ action.path.split('/').pop() }}</span>
            <span class="scan__date">{{ stamp(action.new_date) }}</span>
            <span class="scan__date">{{ action.shift ?? '—' }}</span>
          </div>
        </div>

        <ul v-if="plan.skipped.length" class="skipped">
          <li v-for="skip in plan.skipped" :key="skip.file">
            <span class="mark" aria-hidden="true">✕</span>
            {{ skip.file.split('/').pop() }} — {{ skip.reason }}
          </li>
        </ul>
      </section>

      <section v-if="scanned" class="scan" aria-live="polite">
        <h2 class="scan__head">
          // {{ scanned.length }} SCANNED // {{ needingAttention(scanned) }} NEEDING ATTENTION
        </h2>

        <p v-if="!scanned.length" class="muted">
          Nothing here this tool reads. Check the folder, or tick Recursive.
        </p>

        <div v-else class="scan__table">
          <div class="scan__row scan__row--head" role="row">
            <span role="columnheader">File</span>
            <span role="columnheader">Metadata date</span>
            <span role="columnheader">Filesystem</span>
            <span role="columnheader">State</span>
          </div>
          <div
            v-for="row in scanned"
            :key="row.path"
            class="scan__row"
            role="row"
            :data-status="row.status"
          >
            <span class="scan__name" :title="row.path">{{ row.name }}</span>
            <span class="scan__date">
              {{ stamp(row.metadata_date) }}
              <small v-if="row.tag" class="muted">{{ row.tag }}</small>
            </span>
            <span class="scan__date">
              {{ stamp(row.fs_date) }}
              <small v-if="row.fs_date_source" class="muted">{{ row.fs_date_source }}</small>
            </span>
            <span class="scan__state">
              <span class="mark" aria-hidden="true">{{ statusMark(row.status) }}</span>
              {{ statusWord(row.status) }}
            </span>
          </div>
        </div>
      </section>
    </template>
  </ToolPage>
</template>

<style scoped>
.row { display: flex; gap: var(--space-3); align-items: center; flex-wrap: wrap; }

.scan {
  display: grid;
  gap: var(--space-3);
}
.scan__head {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.scan__table {
  border: var(--border-hair);
  background: var(--bg-elevated);
  /* A long scan scrolls inside its own box rather than pushing the controls
     off the screen. */
  max-height: 50vh;
  overflow-y: auto;
}
.scan__row {
  display: grid;
  grid-template-columns: minmax(90px, 1.2fr) 150px 150px 116px;
  gap: var(--space-2);
  align-items: center;
  padding: var(--space-2) var(--space-3);
  border-bottom: var(--border-hair);
  font-family: var(--font-body);
  font-size: 13px;
}
.scan__row:last-child { border-bottom: none; }
.scan__row--head {
  position: sticky;
  top: 0;
  background: var(--bg-panel);
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--text-muted);
}
.scan__name {
  color: var(--text-heading);
  overflow-wrap: anywhere;
}
.scan__date {
  display: grid;
  font-variant-numeric: tabular-nums;
}
.scan__date small { font-size: 11px; }
.scan__state {
  display: inline-flex;
  align-items: baseline;
  gap: var(--space-1);
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
/* The state carries a mark as well as a colour: which files are wrong is the
   entire point of the table, and that cannot live in a hue alone. */
.mark { font-weight: 700; }
.scan__row[data-status='Ok'] .scan__state { color: var(--accent); }
.scan__row[data-status='Mismatch'] .scan__state { color: var(--accent-warm); }
.scan__row[data-status='MissingMetadata'] .scan__state { color: var(--danger); }

/* The plan has one column fewer: there is no filesystem date to compare with,
   only what the file would become. */
.scan__row--plan {
  grid-template-columns: minmax(90px, 1.2fr) 170px 130px;
}

.skipped {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  font-size: 12px;
  color: var(--text-muted);
}
.skipped .mark { color: var(--danger); }

/* On a phone the two dates are what fall away last; the verdict never does. */
@media (max-width: 560px) {
  .scan__row { grid-template-columns: minmax(80px, 1fr) 132px 104px; }
  .scan__row > :nth-child(3) { display: none; }
  /* The plan's three columns already fit; nothing is dropped from it. */
  .scan__row--plan { grid-template-columns: minmax(80px, 1fr) 140px 84px; }
  .scan__row--plan > :nth-child(3) { display: block; }
}
</style>
