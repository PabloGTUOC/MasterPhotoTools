<script setup lang="ts">
/**
 * Monotone tone curves editor with cubic Hermite spline interpolation (ED-10).
 *
 * Citations:
 * - F. N. Fritsch and R. E. Carlson (1980), "Monotone Piecewise Cubic Interpolation",
 *   SIAM Journal on Numerical Analysis, 17(2), 238–246.
 *
 * Transport-free: props in, events out, never `@host/api`.
 *
 * Controls:
 * - Channel selector pills: Luma (master), Red, Green, Blue with >= 40px hit targets.
 * - Endpoints (0, 0) and (1, 1) are strictly fixed identity boundaries.
 * - Click graph to insert new control point (ordered by x).
 * - Drag control points with mouse (scrubbing emits update:modelValue, release emits change).
 * - Keyboard accessible: arrow keys move selected point by 0.01 (shift+arrow by 0.05).
 * - Double-click or Delete/Backspace key removes an interior control point.
 * - Reset button restores current channel to identity line [(0,0), (1,1)].
 */
import { computed, ref, onUnmounted } from 'vue';
import type { CurvePoint, ToneCurves } from '@phototools/shared';

const props = withDefaults(
  defineProps<{
    modelValue?: ToneCurves | null;
    disabled?: boolean;
    testId?: string;
  }>(),
  {
    modelValue: null,
    disabled: false,
    testId: undefined,
  },
);

const emit = defineEmits<{
  'update:modelValue': [curves: ToneCurves];
  change: [curves: ToneCurves];
}>();

type ChannelId = 'luma' | 'red' | 'green' | 'blue';

interface ChannelInfo {
  id: ChannelId;
  label: string;
}

const CHANNELS: ChannelInfo[] = [
  { id: 'luma', label: 'Luma' },
  { id: 'red', label: 'Red' },
  { id: 'green', label: 'Green' },
  { id: 'blue', label: 'Blue' },
];

const activeChannel = ref<ChannelId>('luma');
const selectedPointIdx = ref<number | null>(null);
const svgRef = ref<SVGSVGElement | null>(null);

// Default identity curve has endpoints (0, 0) and (1, 1)
function defaultPoints(): CurvePoint[] {
  return [
    { x: 0, y: 0 },
    { x: 1, y: 1 },
  ];
}

const currentCurves = computed<ToneCurves>(() => {
  return {
    luma: props.modelValue?.luma ? [...props.modelValue.luma] : defaultPoints(),
    red: props.modelValue?.red ? [...props.modelValue.red] : defaultPoints(),
    green: props.modelValue?.green ? [...props.modelValue.green] : defaultPoints(),
    blue: props.modelValue?.blue ? [...props.modelValue.blue] : defaultPoints(),
  };
});

const currentPoints = computed<CurvePoint[]>(() => {
  const ch = activeChannel.value;
  const pts = currentCurves.value[ch];
  if (!pts || pts.length < 2) {
    return defaultPoints();
  }
  return [...pts].sort((a, b) => a.x - b.x);
});

const selectedPoint = computed<CurvePoint | null>(() => {
  if (selectedPointIdx.value === null) return null;
  return currentPoints.value[selectedPointIdx.value] ?? null;
});

const isCurrentChannelIdentity = computed(() => {
  const pts = currentPoints.value;
  if (pts.length !== 2) return false;
  return (
    Math.abs(pts[0].x) < 1e-4 &&
    Math.abs(pts[0].y) < 1e-4 &&
    Math.abs(pts[1].x - 1) < 1e-4 &&
    Math.abs(pts[1].y - 1) < 1e-4
  );
});

const activeChannelLabel = computed(() => {
  const ch = CHANNELS.find((c) => c.id === activeChannel.value);
  return ch ? ch.label : 'Curve';
});

// SVG coordinate conversions (viewBox 0 0 256 256)
function toSvgX(x: number): number {
  return x * 256;
}

function toSvgY(y: number): number {
  return (1 - y) * 256;
}

function fromSvgX(svgX: number): number {
  return Math.max(0, Math.min(1, svgX / 256));
}

function fromSvgY(svgY: number): number {
  return Math.max(0, Math.min(1, 1 - svgY / 256));
}

/**
 * Fritsch & Carlson (1980) monotone cubic Hermite spline path generation.
 * Converted to exact SVG cubic Bézier control points (C cp1x cp1y, cp2x cp2y, x2 y2).
 */
const curvePathD = computed(() => {
  const pts = currentPoints.value;
  const n = pts.length;
  if (n < 2) {
    return 'M 0 256 L 256 0';
  }
  if (n === 2) {
    return `M ${toSvgX(pts[0].x).toFixed(2)} ${toSvgY(pts[0].y).toFixed(2)} L ${toSvgX(pts[1].x).toFixed(2)} ${toSvgY(pts[1].y).toFixed(2)}`;
  }

  // 1. Secant slopes
  const h: number[] = new Array(n - 1);
  const delta: number[] = new Array(n - 1);
  for (let i = 0; i < n - 1; i++) {
    h[i] = pts[i + 1].x - pts[i].x;
    delta[i] = h[i] > 1e-7 ? (pts[i + 1].y - pts[i].y) / h[i] : 0;
  }

  // 2. Initial tangents
  const d: number[] = new Array(n);
  d[0] = delta[0];
  d[n - 1] = delta[n - 2];
  for (let i = 1; i < n - 1; i++) {
    if (delta[i - 1] * delta[i] <= 0) {
      d[i] = 0;
    } else {
      d[i] = (delta[i - 1] + delta[i]) / 2;
    }
  }

  // 3. Fritsch & Carlson limiter
  for (let i = 0; i < n - 1; i++) {
    if (Math.abs(delta[i]) < 1e-7) {
      d[i] = 0;
      d[i + 1] = 0;
    } else {
      const alpha = d[i] / delta[i];
      const beta = d[i + 1] / delta[i];
      const sumSq = alpha * alpha + beta * beta;
      if (sumSq > 9) {
        const tau = 3 / Math.sqrt(sumSq);
        d[i] = tau * alpha * delta[i];
        d[i + 1] = tau * beta * delta[i];
      }
    }
  }

  // 4. Build exact SVG Bézier path
  let path = `M ${toSvgX(pts[0].x).toFixed(2)} ${toSvgY(pts[0].y).toFixed(2)}`;
  for (let i = 0; i < n - 1; i++) {
    const x0 = pts[i].x;
    const y0 = pts[i].y;
    const x1 = pts[i + 1].x;
    const y1 = pts[i + 1].y;
    const hi = h[i];

    const cp1x = x0 + hi / 3;
    const cp1y = y0 + d[i] * hi / 3;
    const cp2x = x1 - hi / 3;
    const cp2y = y1 - d[i + 1] * hi / 3;

    path += ` C ${toSvgX(cp1x).toFixed(2)} ${toSvgY(cp1y).toFixed(2)}, ${toSvgX(cp2x).toFixed(2)} ${toSvgY(cp2y).toFixed(2)}, ${toSvgX(x1).toFixed(2)} ${toSvgY(y1).toFixed(2)}`;
  }
  return path;
});

function emitUpdate(newPoints: CurvePoint[], isFinalChange = false) {
  const updatedCurves: ToneCurves = {
    ...currentCurves.value,
    [activeChannel.value]: newPoints,
  };
  emit('update:modelValue', updatedCurves);
  if (isFinalChange) {
    emit('change', updatedCurves);
  }
}

function selectChannel(id: ChannelId) {
  if (props.disabled) return;
  activeChannel.value = id;
  selectedPointIdx.value = null;
}

function resetActiveChannel() {
  if (props.disabled) return;
  selectedPointIdx.value = null;
  emitUpdate(defaultPoints(), true);
}

// Drag state tracking
let isDragging = false;
let draggedIdx: number | null = null;

function getSvgCoords(event: MouseEvent): { x: number; y: number } | null {
  if (!svgRef.value) return null;
  const rect = svgRef.value.getBoundingClientRect();
  const svgX = ((event.clientX - rect.left) / rect.width) * 256;
  const svgY = ((event.clientY - rect.top) / rect.height) * 256;
  return {
    x: fromSvgX(svgX),
    y: fromSvgY(svgY),
  };
}

function onPointMouseDown(idx: number, _event: MouseEvent) {
  if (props.disabled) return;
  selectedPointIdx.value = idx;

  // Endpoints (0, 0) and (1, 1) are strictly fixed
  if (idx === 0 || idx === currentPoints.value.length - 1) {
    return;
  }

  isDragging = true;
  draggedIdx = idx;

  window.addEventListener('mousemove', onWindowMouseMove);
  window.addEventListener('mouseup', onWindowMouseUp);
}

function onWindowMouseMove(event: MouseEvent) {
  if (!isDragging || draggedIdx === null) return;
  const coords = getSvgCoords(event);
  if (!coords) return;

  const pts = [...currentPoints.value];
  const idx = draggedIdx;
  if (idx <= 0 || idx >= pts.length - 1) return;

  // Clamp x strictly between left neighbor and right neighbor
  const minX = pts[idx - 1].x + 0.01;
  const maxX = pts[idx + 1].x - 0.01;
  const clampedX = Math.max(minX, Math.min(maxX, coords.x));
  const clampedY = Math.max(0, Math.min(1, coords.y));

  pts[idx] = { x: clampedX, y: clampedY };
  emitUpdate(pts, false);
}

function onWindowMouseUp() {
  if (isDragging) {
    isDragging = false;
    draggedIdx = null;
    emitUpdate(currentPoints.value, true);
  }
  window.removeEventListener('mousemove', onWindowMouseMove);
  window.removeEventListener('mouseup', onWindowMouseUp);
}

onUnmounted(() => {
  window.removeEventListener('mousemove', onWindowMouseMove);
  window.removeEventListener('mouseup', onWindowMouseUp);
});

function onGraphMouseDown(event: MouseEvent) {
  if (props.disabled) return;
  const coords = getSvgCoords(event);
  if (!coords) return;

  // If clicked near an existing point, select it instead of creating a new one
  const pts = currentPoints.value;
  for (let i = 0; i < pts.length; i++) {
    const dx = Math.abs(pts[i].x - coords.x);
    const dy = Math.abs(pts[i].y - coords.y);
    if (dx < 0.04 && dy < 0.04) {
      selectedPointIdx.value = i;
      return;
    }
  }

  // Insert a new point if within valid interior bounds
  if (coords.x > 0.02 && coords.x < 0.98) {
    const newPt: CurvePoint = {
      x: coords.x,
      y: Math.max(0, Math.min(1, coords.y)),
    };
    const newPts = [...pts, newPt].sort((a, b) => a.x - b.x);
    const newIdx = newPts.findIndex((p) => Math.abs(p.x - newPt.x) < 1e-4);
    selectedPointIdx.value = newIdx >= 0 ? newIdx : null;
    emitUpdate(newPts, true);
  }
}

function onPointDblClick(idx: number) {
  if (props.disabled) return;
  // Endpoints cannot be deleted
  if (idx === 0 || idx === currentPoints.value.length - 1) return;

  const pts = currentPoints.value.filter((_, i) => i !== idx);
  selectedPointIdx.value = null;
  emitUpdate(pts, true);
}

function onPointKeyDown(idx: number, event: KeyboardEvent) {
  if (props.disabled) return;

  // Delete key removes selected interior point
  if (event.key === 'Delete' || event.key === 'Backspace') {
    if (idx > 0 && idx < currentPoints.value.length - 1) {
      event.preventDefault();
      onPointDblClick(idx);
    }
    return;
  }

  // Arrow keys adjust point position
  const step = event.shiftKey ? 0.05 : 0.01;
  const pts = [...currentPoints.value];

  // Endpoints are strictly fixed at (0,0) and (1,1)
  if (idx === 0 || idx === pts.length - 1) return;

  let changed = false;
  if (event.key === 'ArrowUp') {
    event.preventDefault();
    pts[idx].y = Math.min(1.0, pts[idx].y + step);
    changed = true;
  } else if (event.key === 'ArrowDown') {
    event.preventDefault();
    pts[idx].y = Math.max(0.0, pts[idx].y - step);
    changed = true;
  } else if (event.key === 'ArrowLeft') {
    event.preventDefault();
    const minX = pts[idx - 1].x + 0.01;
    pts[idx].x = Math.max(minX, pts[idx].x - step);
    changed = true;
  } else if (event.key === 'ArrowRight') {
    event.preventDefault();
    const maxX = pts[idx + 1].x - 0.01;
    pts[idx].x = Math.min(maxX, pts[idx].x + step);
    changed = true;
  }

  if (changed) {
    emitUpdate(pts, true);
  }
}

function onGraphKeyDown(event: KeyboardEvent) {
  if (selectedPointIdx.value !== null) {
    onPointKeyDown(selectedPointIdx.value, event);
  }
}
</script>

<template>
  <div
    class="curve-editor"
    :class="{ 'curve-editor--disabled': props.disabled }"
    :data-testid="props.testId ?? 'curve-editor'"
  >
    <!-- Channel selector pills with >= 40px hit targets -->
    <div class="curve-editor__channels" role="tablist" aria-label="Tone curve channels">
      <button
        v-for="ch in CHANNELS"
        :key="ch.id"
        type="button"
        role="tab"
        :aria-selected="activeChannel === ch.id"
        class="curve-editor__channel-pill"
        :class="{
          'curve-editor__channel-pill--active': activeChannel === ch.id,
          [`curve-editor__channel-pill--${ch.id}`]: true,
        }"
        :data-testid="`curve-channel-${ch.id}`"
        :disabled="props.disabled"
        @click="selectChannel(ch.id)"
      >
        {{ ch.label }}
      </button>
    </div>

    <!-- SVG Curve Graph (256x256) -->
    <div class="curve-editor__graph-container">
      <svg
        ref="svgRef"
        class="curve-editor__graph"
        viewBox="0 0 256 256"
        tabindex="0"
        role="application"
        aria-label="Tone curve graph"
        data-testid="curve-graph"
        @mousedown="onGraphMouseDown"
        @keydown="onGraphKeyDown"
      >
        <!-- Background grid lines -->
        <g class="curve-editor__grid">
          <line x1="64" y1="0" x2="64" y2="256" />
          <line x1="128" y1="0" x2="128" y2="256" />
          <line x1="192" y1="0" x2="192" y2="256" />
          <line x1="0" y1="64" x2="256" y2="64" />
          <line x1="0" y1="128" x2="256" y2="128" />
          <line x1="0" y1="192" x2="256" y2="192" />
        </g>

        <!-- Diagonal identity reference line (dashed) -->
        <line x1="0" y1="256" x2="256" y2="0" class="curve-editor__identity-line" />

        <!-- Monotone cubic spline curve -->
        <path
          :d="curvePathD"
          class="curve-editor__curve"
          :class="`curve-editor__curve--${activeChannel}`"
          fill="none"
          data-testid="curve-path"
        />

        <!-- Control points -->
        <g
          v-for="(pt, idx) in currentPoints"
          :key="idx"
          class="curve-editor__point-group"
          :class="{
            'curve-editor__point-group--selected': selectedPointIdx === idx,
            'curve-editor__point-group--endpoint': idx === 0 || idx === currentPoints.length - 1,
          }"
          :data-testid="`curve-point-${idx}`"
          tabindex="0"
          role="slider"
          :aria-valuenow="Math.round(pt.y * 100)"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-label="`Point ${idx}: in ${Math.round(pt.x * 255)}, out ${Math.round(pt.y * 255)}`"
          @mousedown.stop="onPointMouseDown(idx, $event)"
          @dblclick.stop="onPointDblClick(idx)"
          @keydown="onPointKeyDown(idx, $event)"
        >
          <!-- Invisible 40px touch hit area (r=20) -->
          <circle
            :cx="toSvgX(pt.x)"
            :cy="toSvgY(pt.y)"
            r="20"
            class="curve-editor__point-hit"
          />
          <!-- Selection halo ring -->
          <circle
            v-if="selectedPointIdx === idx"
            :cx="toSvgX(pt.x)"
            :cy="toSvgY(pt.y)"
            r="8"
            class="curve-editor__point-ring"
          />
          <!-- Visible point circle -->
          <circle
            :cx="toSvgX(pt.x)"
            :cy="toSvgY(pt.y)"
            r="4.5"
            class="curve-editor__point"
            :class="`curve-editor__point--${activeChannel}`"
          />
        </g>
      </svg>
    </div>

    <!-- Readout coordinates & channel reset -->
    <div class="curve-editor__footer">
      <div class="curve-editor__readout">
        <template v-if="selectedPoint">
          <span class="curve-editor__coord">In: {{ Math.round(selectedPoint.x * 255) }}</span>
          <span class="curve-editor__coord">Out: {{ Math.round(selectedPoint.y * 255) }}</span>
        </template>
        <span v-else class="curve-editor__coord-hint">Click graph to add point</span>
      </div>
      <button
        type="button"
        class="curve-editor__reset-btn"
        :disabled="props.disabled || isCurrentChannelIdentity"
        data-testid="curve-reset-channel"
        @click="resetActiveChannel"
      >
        Reset {{ activeChannelLabel }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.curve-editor {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm, 8px);
  width: 100%;
  user-select: none;
}

.curve-editor--disabled {
  opacity: 0.5;
  pointer-events: none;
}

/* Channel selector pills (>= 40px hit target) */
.curve-editor__channels {
  display: flex;
  gap: var(--space-xs, 4px);
  width: 100%;
}

.curve-editor__channel-pill {
  flex: 1;
  min-height: 40px;
  padding: 8px 12px;
  font-family: inherit;
  font-size: 13px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-muted, #888888);
  background: var(--color-surface, #1e1e1e);
  border: 1px solid var(--color-border, #333333);
  border-radius: 2px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease;
}

.curve-editor__channel-pill:hover:not(:disabled) {
  color: var(--color-text, #ffffff);
  border-color: var(--color-text-muted, #888888);
}

.curve-editor__channel-pill:focus-visible {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: 1px;
}

.curve-editor__channel-pill--active {
  color: var(--color-text, #ffffff);
  background: var(--color-bg, #121212);
  border-color: var(--color-accent, #3b82f6);
  font-weight: 600;
}

.curve-editor__channel-pill--active.curve-editor__channel-pill--red {
  border-color: #ef4444;
  color: #ef4444;
}

.curve-editor__channel-pill--active.curve-editor__channel-pill--green {
  border-color: #22c55e;
  color: #22c55e;
}

.curve-editor__channel-pill--active.curve-editor__channel-pill--blue {
  border-color: #3b82f6;
  color: #3b82f6;
}

/* SVG Graph area */
.curve-editor__graph-container {
  width: 100%;
  aspect-ratio: 1 / 1;
  background: var(--color-surface, #1e1e1e);
  border: 1px solid var(--color-border, #333333);
  border-radius: 2px;
  overflow: hidden;
  position: relative;
}

.curve-editor__graph {
  width: 100%;
  height: 100%;
  display: block;
  cursor: crosshair;
}

.curve-editor__graph:focus-visible {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: -2px;
}

/* Grid & Reference lines (strictly no glow) */
.curve-editor__grid line {
  stroke: var(--color-border, #333333);
  stroke-width: 1;
  stroke-dasharray: 2 4;
}

.curve-editor__identity-line {
  stroke: var(--color-border, #333333);
  stroke-width: 1;
  stroke-dasharray: 4 4;
}

/* Curve lines */
.curve-editor__curve {
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.curve-editor__curve--luma {
  stroke: var(--color-accent, #3b82f6);
}

.curve-editor__curve--red {
  stroke: #ef4444;
}

.curve-editor__curve--green {
  stroke: #22c55e;
}

.curve-editor__curve--blue {
  stroke: #3b82f6;
}

/* Control points */
.curve-editor__point-hit {
  fill: transparent;
  cursor: grab;
}

.curve-editor__point-group:active .curve-editor__point-hit {
  cursor: grabbing;
}

.curve-editor__point-group--endpoint .curve-editor__point-hit {
  cursor: default;
}

.curve-editor__point-ring {
  fill: none;
  stroke: var(--color-text, #ffffff);
  stroke-width: 1.5;
}

.curve-editor__point {
  fill: var(--color-bg, #121212);
  stroke-width: 2;
  transition: transform 0.1s ease;
}

.curve-editor__point--luma {
  stroke: var(--color-accent, #3b82f6);
}

.curve-editor__point--red {
  stroke: #ef4444;
}

.curve-editor__point--green {
  stroke: #22c55e;
}

.curve-editor__point--blue {
  stroke: #3b82f6;
}

.curve-editor__point-group:hover .curve-editor__point {
  fill: var(--color-text, #ffffff);
}

.curve-editor__point-group:focus-visible {
  outline: none;
}

.curve-editor__point-group:focus-visible .curve-editor__point {
  fill: var(--color-text, #ffffff);
  stroke: var(--color-accent, #3b82f6);
}

/* Footer readout & reset button */
.curve-editor__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 40px;
  gap: var(--space-sm, 8px);
}

.curve-editor__readout {
  display: flex;
  align-items: center;
  gap: var(--space-sm, 8px);
  font-size: 13px;
  font-family: var(--font-mono, monospace);
  color: var(--color-text-muted, #888888);
}

.curve-editor__coord {
  padding: 2px 6px;
  background: var(--color-surface, #1e1e1e);
  border: 1px solid var(--color-border, #333333);
  border-radius: 2px;
}

.curve-editor__coord-hint {
  font-size: 12px;
  color: var(--color-text-muted, #888888);
}

.curve-editor__reset-btn {
  min-height: 40px;
  padding: 8px 12px;
  font-family: inherit;
  font-size: 13px;
  color: var(--color-text-muted, #888888);
  background: transparent;
  border: 1px solid var(--color-border, #333333);
  border-radius: 2px;
  cursor: pointer;
  transition: color 0.15s ease, border-color 0.15s ease;
}

.curve-editor__reset-btn:hover:not(:disabled) {
  color: var(--color-text, #ffffff);
  border-color: var(--color-text-muted, #888888);
}

.curve-editor__reset-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.curve-editor__reset-btn:focus-visible {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: 1px;
}
</style>
