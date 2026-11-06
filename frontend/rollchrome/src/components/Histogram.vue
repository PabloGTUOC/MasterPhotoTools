<script setup lang="ts">
/**
 * Histogram of the frame on screen (ED-15).
 *
 * Transport-free: the counts arrive as props, already made by `core` from the
 * pixels being shown, so the shape here is never an estimate of a different frame.
 *
 * - The vertical scale ignores the two end bins. A photograph with any clipping
 *   piles thousands of pixels into bin 0 or 255; scaled to that spike, the rest
 *   of the histogram flattens into a line, which hides exactly the tonal shape
 *   the person is adjusting. The spike is drawn to the top instead, and the
 *   clipping it represents is stated in words underneath.
 * - In RGB the three channels share one scale and are screen-blended, so where
 *   they overlap reads as their mix, as in every darkroom application.
 */
import { computed } from 'vue';

export type HistogramMode = 'rgb' | 'luma' | 'red' | 'green' | 'blue';

export interface HistogramCounts {
  red: ArrayLike<number>;
  green: ArrayLike<number>;
  blue: ArrayLike<number>;
  luminance: ArrayLike<number>;
  pixels: number;
  highlightClipped: number;
  shadowClipped: number;
}

const props = defineProps<{
  histogram: HistogramCounts | null;
  mode: HistogramMode;
}>();

const emit = defineEmits<{
  'update:mode': [mode: HistogramMode];
}>();

const MODES: { key: HistogramMode; label: string; name: string }[] = [
  { key: 'rgb', label: 'RGB', name: 'Red, green and blue' },
  { key: 'luma', label: 'Luma', name: 'Luminance' },
  { key: 'red', label: 'R', name: 'Red' },
  { key: 'green', label: 'G', name: 'Green' },
  { key: 'blue', label: 'B', name: 'Blue' },
];

/** Height of the drawing in SVG units; the width is one unit per bin. */
const H = 100;

type Channel = 'red' | 'green' | 'blue' | 'luminance';

const channels = computed<Channel[]>(() => {
  switch (props.mode) {
    case 'rgb':
      return ['red', 'green', 'blue'];
    case 'luma':
      return ['luminance'];
    default:
      return [props.mode];
  }
});

const scale = computed(() => {
  const h = props.histogram;
  if (!h) return 1;
  let inner = 0;
  let all = 0;
  for (const c of channels.value) {
    const bins = h[c];
    for (let i = 0; i < bins.length; i++) {
      const v = bins[i];
      if (v > all) all = v;
      if (i > 0 && i < bins.length - 1 && v > inner) inner = v;
    }
  }
  // A frame entirely at the ends (pure black and white) has no inner peak to scale to.
  return inner > 0 ? inner : Math.max(all, 1);
});

function pathFor(bins: ArrayLike<number>): string {
  const s = scale.value;
  let d = `M0,${H}`;
  for (let i = 0; i < bins.length; i++) {
    const y = H - Math.min(bins[i] / s, 1) * H;
    d += ` L${i},${y.toFixed(2)}`;
  }
  return `${d} L${bins.length - 1},${H} Z`;
}

const paths = computed(() => {
  const h = props.histogram;
  if (!h) return [];
  return channels.value.map((c) => ({ channel: c, d: pathFor(h[c]) }));
});

function percent(count: number, total: number): string {
  if (count === 0 || total === 0) return '0%';
  const p = (count / total) * 100;
  return p < 0.1 ? '<0.1%' : `${p.toFixed(1)}%`;
}

const shadowText = computed(() =>
  props.histogram ? percent(props.histogram.shadowClipped, props.histogram.pixels) : '—',
);
const highlightText = computed(() =>
  props.histogram ? percent(props.histogram.highlightClipped, props.histogram.pixels) : '—',
);

const label = computed(
  () => `Histogram: ${MODES.find((m) => m.key === props.mode)?.name ?? props.mode}`,
);
</script>

<template>
  <div class="histogram" data-testid="histogram">
    <svg
      class="histogram__plot"
      :class="{ 'is-rgb': mode === 'rgb' }"
      viewBox="0 0 255 100"
      preserveAspectRatio="none"
      role="img"
      :aria-label="label"
      data-testid="histogram-plot"
    >
      <path
        v-for="p in paths"
        :key="p.channel"
        class="histogram__channel"
        :class="`histogram__channel--${p.channel}`"
        :d="p.d"
        :data-channel="p.channel"
      />
    </svg>
    <p v-if="!histogram" class="histogram__empty">No frame yet</p>

    <div class="histogram__modes" role="group" aria-label="Histogram channel">
      <button
        v-for="m in MODES"
        :key="m.key"
        type="button"
        class="histogram__mode"
        :class="{ active: mode === m.key }"
        :aria-pressed="mode === m.key"
        :title="m.name"
        :data-testid="`histogram-mode-${m.key}`"
        @click="emit('update:mode', m.key)"
      >
        {{ m.label }}
      </button>
    </div>

    <p class="histogram__clipping" data-testid="histogram-clipping">
      <span>Shadows clipped <strong data-testid="histogram-shadow-clipped">{{ shadowText }}</strong></span>
      <span>Highlights clipped <strong data-testid="histogram-highlight-clipped">{{ highlightText }}</strong></span>
    </p>
  </div>
</template>

<style scoped>
.histogram {
  position: relative;
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.histogram__plot {
  display: block;
  width: 100%;
  height: 96px;
  background: var(--void);
  border: var(--border-hair);
}

.histogram__channel {
  stroke: none;
}

.histogram__channel--luminance {
  fill: var(--text);
  fill-opacity: 0.75;
}

.histogram__channel--red {
  fill: var(--channel-red);
}

.histogram__channel--green {
  fill: var(--channel-green);
}

.histogram__channel--blue {
  fill: var(--channel-blue);
}

/* Overlapping channels screen-blend, so all three at once read as white. */
.histogram__plot.is-rgb .histogram__channel {
  fill-opacity: 0.85;
  mix-blend-mode: screen;
}

.histogram__plot:not(.is-rgb) .histogram__channel:not(.histogram__channel--luminance) {
  fill-opacity: 0.8;
}

.histogram__empty {
  position: absolute;
  top: var(--space-2);
  left: var(--space-2);
  right: var(--space-2);
  height: 96px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
  pointer-events: none;
}

.histogram__modes {
  display: grid;
  grid-template-columns: 1.4fr 1.4fr 1fr 1fr 1fr;
  gap: var(--space-1);
}

.histogram__mode {
  min-height: 40px;
  padding: 0 var(--space-1);
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
  background: var(--bg-elevated);
  border: var(--border-hair);
  border-radius: var(--radius-none);
}

.histogram__mode.active {
  color: var(--accent);
  border: var(--border-active);
}

.histogram__mode:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}

.histogram__clipping {
  display: flex;
  justify-content: space-between;
  gap: var(--space-2);
  margin: 0;
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
}

.histogram__clipping strong {
  color: var(--text);
  font-weight: normal;
}
</style>
