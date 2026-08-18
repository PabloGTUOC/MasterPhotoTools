<script setup lang="ts">
/**
 * The review grid: one row per shot, with a status chip per check (Phase 13).
 *
 * **Transport-free.** Rows in, events out, no `@host/api` import — which is what
 * lets it be rendered by whichever build has the shots, and measured standalone
 * against four hundred of them.
 *
 * **Windowed.** Only the rows near the viewport are in the DOM. A 24–45 MP
 * camera fails the resolution check on virtually every frame, so a full card is
 * four hundred *failing* rows, each with several chips — and the acceptance
 * criterion is that it stays responsive, not that it renders at all. Filtering
 * four hundred rows should not cost four hundred component teardowns.
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { Check, ShotRow, ShotVerdict } from '@phototools/shared';

const props = defineProps<{
  shots: ShotRow[];
  /** Verdicts by stem. A shot with none has not been validated yet. */
  verdicts: Record<string, ShotVerdict>;
  /** Show only shots carrying this failure class; empty shows everything. */
  filter?: string;
  /** Rendered height of one row, in pixels. Must match the CSS. */
  rowHeight?: number;
}>();

const ROW_HEIGHT = () => props.rowHeight ?? 44;

/** How many rows to render beyond the viewport, above and below. */
const OVERSCAN = 8;

const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

const visible = computed(() => {
  if (!props.filter) return props.shots;
  return props.shots.filter((shot) =>
    (props.verdicts[shot.stem]?.checks ?? []).some(
      (check) => check.failure === props.filter,
    ),
  );
});

/** The slice of `visible` that is actually built into DOM. */
const window_ = computed(() => {
  const height = ROW_HEIGHT();
  const total = visible.value.length;
  if (viewportHeight.value === 0) {
    // Before the first measurement, render a screenful rather than nothing —
    // otherwise a grid that is never scrolled shows an empty table.
    return { start: 0, rows: visible.value.slice(0, 30) };
  }
  const start = Math.max(0, Math.floor(scrollTop.value / height) - OVERSCAN);
  const count = Math.ceil(viewportHeight.value / height) + OVERSCAN * 2;
  return { start, rows: visible.value.slice(start, Math.min(total, start + count)) };
});

function onScroll(event: Event) {
  scrollTop.value = (event.target as HTMLElement).scrollTop;
}

let observer: ResizeObserver | null = null;

onMounted(() => {
  const element = viewport.value;
  if (!element) return;
  viewportHeight.value = element.clientHeight;
  if (typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(() => {
      viewportHeight.value = element.clientHeight;
    });
    observer.observe(element);
  }
});

onUnmounted(() => observer?.disconnect());

// A filter change can leave the scroll position past the end of a shorter list,
// which would show an empty window over a non-empty result.
watch(visible, () => {
  if (viewport.value && scrollTop.value > visible.value.length * ROW_HEIGHT()) {
    viewport.value.scrollTop = 0;
    scrollTop.value = 0;
  }
});

/** How the shot's files pair up (F11). */
function pairing(shot: ShotRow): string {
  if (shot.asset_count > 1) return 'RAW + JPEG';
  return shot.needs_derivation ? 'RAW only' : 'JPEG';
}

function megapixels(shot: ShotRow): string {
  return shot.megapixels > 0 ? `${shot.megapixels.toFixed(1)} MP` : '—';
}

function size(shot: ShotRow): string {
  const mb = shot.bytes / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(shot.bytes / 1024)} KB`;
}

function captured(shot: ShotRow): string {
  // The scan reports a naive local datetime; only the date and minute matter
  // here, and the seconds are noise in a four-hundred-row table.
  return shot.capture ? shot.capture.replace('T', ' ').slice(0, 16) : 'no date';
}

function checksOf(shot: ShotRow): Check[] {
  return props.verdicts[shot.stem]?.checks ?? [];
}

/** A short label for a chip. The rule name, not the whole detail. */
function chipLabel(check: Check): string {
  return check.rule.replace(/([a-z])([A-Z])/g, '$1 $2').toLowerCase();
}

/**
 * A mark carrying the verdict, so colour is not the only thing that does.
 *
 * Red against teal at chip size is not a distinction everybody can make, and a
 * review screen whose entire meaning is "which of these failed" cannot put that
 * meaning in a hue alone.
 */
function chipMark(check: Check): string {
  switch (check.status) {
    case 'pass':
      return '\u2713';
    case 'fail':
      return '\u2715';
    case 'warn':
      return '!';
    default:
      return '\u00b7';
  }
}

/** What a screen reader should hear instead of the mark. */
function chipStatusWord(check: Check): string {
  return check.status === 'pending' ? 'not yet checked' : check.status;
}

defineExpose({ visibleCount: () => visible.value.length });
</script>

<template>
  <div class="grid">
    <div class="head" role="row">
      <span role="columnheader">Shot</span>
      <span role="columnheader">Files</span>
      <span role="columnheader">Captured</span>
      <span role="columnheader">Pixels</span>
      <span role="columnheader">Size</span>
      <span role="columnheader">Checks</span>
    </div>

    <div
      ref="viewport"
      class="viewport"
      role="rowgroup"
      :aria-rowcount="visible.length"
      data-testid="shot-viewport"
      @scroll.passive="onScroll"
    >
      <!-- A spacer of the full height, so the scrollbar reflects the whole
           card rather than only the rows currently built. -->
      <div class="spacer" :style="{ height: `${visible.length * ROW_HEIGHT()}px` }">
        <div
          class="rows"
          :style="{ transform: `translateY(${window_.start * ROW_HEIGHT()}px)` }"
        >
          <div
            v-for="shot in window_.rows"
            :key="shot.stem"
            class="row"
            role="row"
            data-testid="shot-row"
            :style="{ height: `${ROW_HEIGHT()}px` }"
          >
            <span class="stem" role="cell">{{ shot.stem }}</span>
            <span class="muted small" role="cell">{{ pairing(shot) }}</span>
            <span class="mono small" role="cell">{{ captured(shot) }}</span>
            <span class="mono small" role="cell">{{ megapixels(shot) }}</span>
            <span class="mono small" role="cell">{{ size(shot) }}</span>
            <span class="chips" role="cell">
              <span
                v-for="check in checksOf(shot)"
                :key="check.rule"
                class="chip"
                :data-status="check.status"
                :title="check.detail"
              >
                <span class="mark" aria-hidden="true">{{ chipMark(check) }}</span>
                <span class="sr-only">{{ chipStatusWord(check) }}:</span>
                {{ chipLabel(check) }}
              </span>
              <span v-if="!checksOf(shot).length" class="muted small">not checked</span>
            </span>
          </div>
        </div>
      </div>

      <p v-if="!visible.length" class="empty muted">
        No shots match this filter.
      </p>
    </div>
  </div>
</template>

<style scoped>
.grid {
  border: 1px solid var(--rule);
  border-radius: 10px;
  overflow: hidden;
  background: var(--surface);
}
.head, .row {
  display: grid;
  /* The stem and the chips take the slack; the measurements are fixed so the
     columns do not shift as rows scroll into view. */
  grid-template-columns: minmax(90px, 1fr) 84px 132px 68px 74px minmax(120px, 1.4fr);
  gap: 8px;
  align-items: center;
  padding: 0 12px;
}
.head {
  height: 38px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--rule);
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--ink-soft);
}
.viewport {
  /* Bounded, so the window has something to be a window onto. */
  max-height: 60vh;
  overflow-y: auto;
  overflow-x: auto;
  position: relative;
}
.spacer { position: relative; }
.rows { position: absolute; top: 0; left: 0; right: 0; }
.row {
  border-bottom: 1px solid var(--rule);
  font-size: 0.88rem;
}
/* Every cell is one line. The window's arithmetic assumes a fixed row height,
   so a wrapping date would put the rows out of step with the spacer that gives
   the scrollbar its length. */
.row > * {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.row:last-child { border-bottom: none; }
.stem { font-weight: 600; white-space: nowrap; }
.mono { font-family: var(--mono); }
.small { font-size: 0.78rem; }
.chips { display: flex; gap: 4px; flex-wrap: nowrap; overflow: hidden; }
.chip {
  display: inline-flex;
  align-items: baseline;
  gap: 4px;
  font-size: 0.68rem;
  padding: 2px 7px;
  border-radius: 999px;
  border: 1px solid var(--rule);
  color: var(--ink-soft);
  white-space: nowrap;
}
.mark { font-family: var(--mono); font-weight: 700; }
.sr-only {
  position: absolute;
  width: 1px; height: 1px;
  padding: 0; margin: -1px;
  overflow: hidden; clip: rect(0 0 0 0);
  white-space: nowrap; border: 0;
}
.chip[data-status='pass'] { color: var(--ok); border-color: var(--ok); }
.chip[data-status='warn'] { color: var(--warn); border-color: var(--warn); }
.chip[data-status='fail'] { color: var(--critical); border-color: var(--critical); }
.empty { padding: 24px 12px; text-align: center; }

/* Narrow screens drop the measurements a phone cannot usefully read, keeping
   the shot, its date and its verdicts. */
@media (max-width: 560px) {
  .head, .row { grid-template-columns: minmax(80px, 1fr) 108px minmax(96px, 1fr); }
  .head > :nth-child(2),
  .row > :nth-child(2),
  .head > :nth-child(4),
  .row > :nth-child(4),
  .head > :nth-child(5),
  .row > :nth-child(5) { display: none; }
}
</style>
