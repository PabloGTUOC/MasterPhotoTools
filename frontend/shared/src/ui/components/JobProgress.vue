<script setup lang="ts">
/**
 * Follows a job's SSE stream to its terminal event, with cancel.
 *
 * Cancelling stops watching; the job keeps running on the server, which is what
 * the button says.
 */
import { computed, onUnmounted, ref, watch } from 'vue';
import type { JobEvent } from '@phototools/shared';
import { api } from '@host/api';

const props = withDefaults(
  defineProps<{
    jobId: string | null;
    canCancel?: boolean;
    /**
     * Show the time per item and an estimate of the time left, from the job's own progress
     * (DV-8). Names the items, e.g. "photograph"; omitted, nothing is shown.
     */
    timingItem?: string;
  }>(),
  {
    canCancel: false,
    timingItem: undefined,
  },
);

const emit = defineEmits<{
  (e: 'cancel'): void;
  (e: 'done'): void;
}>();

const event = ref<JobEvent | null>(null);
/** When the first update arrived, and when the latest did: the timing is measured between them. */
const firstAt = ref<number | null>(null);
const latestAt = ref<number | null>(null);
const failure = ref<string | null>(null);
const watching = ref(false);
let controller: AbortController | null = null;

const isRunning = computed(() => {
  if (!watching.value) return false;
  if (event.value?.terminal) return false;
  const s = event.value?.state;
  return s === undefined || s === 'pending' || s === 'running';
});

function stop() {
  controller?.abort();
  controller = null;
  watching.value = false;
}

watch(
  () => props.jobId,
  async (id) => {
    stop();
    event.value = null;
    failure.value = null;
    firstAt.value = null;
    latestAt.value = null;
    if (!id) return;

    controller = new AbortController();
    watching.value = true;
    try {
      await api.watchJob(
        id,
        (next) => {
          event.value = next;
          const now = Date.now();
          if (firstAt.value === null) firstAt.value = now;
          latestAt.value = now;
          if (next.terminal) {
            emit('done');
          }
        },
        controller.signal,
      );
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        failure.value = e instanceof Error ? e.message : String(e);
      }
    } finally {
      watching.value = false;
    }
  },
  { immediate: true },
);

onUnmounted(stop);

/** `95` → `1 min 35 s`; under a minute, seconds only. */
function duration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${s % 60} s`;
}

/**
 * `1.0 s per photograph · about 2 min 5 s left`. The rate is the time since the first update over
 * the items finished since then; nothing is shown until one has finished.
 */
const timing = computed(() => {
  const e = event.value;
  if (!props.timingItem || !e || e.terminal || firstAt.value === null || latestAt.value === null) {
    return null;
  }
  if (e.progress <= 0 || e.total <= 0) return null;
  const elapsed = (latestAt.value - firstAt.value) / 1000;
  if (elapsed <= 0) return null;
  const per = elapsed / e.progress;
  const left = per * Math.max(0, e.total - e.progress);
  return `${per.toFixed(1)} s per ${props.timingItem} · about ${duration(left)} left`;
});

const percent = () => {
  const e = event.value;
  if (!e || e.total === 0) return null;
  return Math.min(100, Math.round((e.progress / e.total) * 100));
};

/** Cells in the terminal progress bar. Fewer on a phone, where 20 will not fit. */
const CELLS = 20;

/**
 * The bar as characters — `████████░░░░░░` — rather than a filled div.
 *
 * §5.5: progress is a terminal readout. Rendering it as text rather than as a
 * styled element means it is the same object on screen and in a copied log,
 * and it cannot drift out of step with the number beside it.
 */
const bar = () => {
  const p = percent();
  if (p === null) return '';
  const filled = Math.round((p / 100) * CELLS);
  return '█'.repeat(filled) + '░'.repeat(CELLS - filled);
};
</script>

<template>
  <section v-if="jobId" class="job" aria-live="polite">
    <header class="job-head">
      <span class="job-state" :data-state="event?.state ?? 'pending'">
        // {{ event?.state ?? 'starting' }}
      </span>
      <div class="job-actions">
        <button
          v-if="canCancel && isRunning"
          type="button"
          class="ghost danger"
          @click="emit('cancel')"
        >
          Cancel
        </button>
        <button v-if="watching" type="button" class="ghost" @click="stop">
          Stop watching
        </button>
      </div>
    </header>

    <p
      v-if="percent() !== null"
      class="bar"
      role="progressbar"
      :aria-valuenow="percent() ?? 0"
      aria-valuemin="0"
      aria-valuemax="100"
    >
      <span class="bar__cells" aria-hidden="true">[{{ bar() }}]</span>
      <span class="bar__percent">{{ percent() }}%</span>
    </p>

    <p v-if="event?.message" class="job-message">{{ event.message }}</p>
    <p v-if="timing" class="muted job-timing" data-testid="job-timing">{{ timing }}</p>
    <p v-if="failure" class="error">{{ failure }}</p>
    <p v-if="!watching && !event" class="muted">
      Waiting for the first update<span class="cursor">_</span>
    </p>
  </section>
</template>

<style scoped>
.job {
  display: grid;
  gap: var(--space-2);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  padding: var(--space-3);
  background: var(--bg-elevated);
}

.job-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.job-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.job-state {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}
/* A running job is the one that should catch the eye; a finished one should not
   keep shouting, and a failed one has to. */
.job-state[data-state='running'] {
  color: var(--accent);
  text-shadow: var(--glow-phosphor);
}
.job-state[data-state='succeeded'] {
  color: var(--accent);
}
.job-state[data-state='failed'] {
  color: var(--danger);
  text-shadow: var(--glow-red);
}
.job-state[data-state='interrupted'] {
  color: var(--accent-warm);
}
.job-state[data-state='cancelled'] {
  color: var(--accent-warm);
}

.bar {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  font-family: var(--font-body);
  font-size: 14px;
  color: var(--accent);
}
.bar__cells {
  /* The cells are the one place a fixed measure matters: proportional spacing
     between them would make the bar jitter as it fills. */
  letter-spacing: -0.05em;
  overflow: hidden;
  white-space: nowrap;
}
.bar__percent {
  margin-left: auto;
  color: var(--text);
  font-variant-numeric: tabular-nums;
}

.job-message {
  font-family: var(--font-body);
  font-size: 13px;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}
</style>
