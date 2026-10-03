<script setup lang="ts">
/**
 * Handles for shaping gradient masks over the photograph (ED-20).
 *
 * Transport-free: masks and the frame's map arrive as props; edits leave as events.
 *
 * - Masks live in the stored frame's coordinates (core's `masks` module). The overlay
 *   works in the frame's own coordinates and goes through `toStored`, the map core sends
 *   with every frame, and its inverse. It knows nothing of crops, rotations or
 *   orientation, so a geometry rule cannot be re-derived here and drift (the ED-13 lesson).
 * - It is sized and transformed exactly like the canvas, and pointer positions are read
 *   back through the browser's own transform (`getScreenCTM`), so a rotated canvas needs
 *   no special case.
 * - Shapes are measured in the stored frame's pixel proportions (`aspect`), as core
 *   measures them, so a circle drawn here is a circle in the photograph.
 * - Every handle is a 40 px target whatever the zoom, and moves with the arrow keys.
 * - With `paint` set, a press-and-drag anywhere paints a stroke (ED-21). A point is kept
 *   only once the pointer has moved a quarter of the brush radius, so a slow drag does not
 *   fill the sidecar with points that change nothing; a ring shows the brush's size.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

export type MaskShape =
  | { type: 'linear'; start: [number, number]; end: [number, number] }
  | {
      type: 'radial';
      center: [number, number];
      radius_x: number;
      radius_y: number;
      angle: number;
      feather: number;
    }
  | { type: 'brush' }
  /** Made by a model and stored as pixels: nothing to drag, only to paint (ED-23). */
  | { type: 'auto' };

export interface OverlayMask {
  id: string;
  name: string;
  kind: MaskShape;
  strokes?: { points: [number, number][] }[];
}

type Affine = [number, number, number, number, number, number];

const props = defineProps<{
  masks: OverlayMask[];
  selectedId: string | null;
  /** Frame-normalised → stored-normalised. */
  toStored: Affine;
  /** Stored width and height over the stored long edge. */
  aspect: { ax: number; ay: number };
  frameWidth: number;
  frameHeight: number;
  /** Brush radius as a fraction of the stored long edge, when painting; null otherwise. */
  paint?: number | null;
}>();

const emit = defineEmits<{
  select: [id: string];
  /** While a handle moves: drag frames. */
  update: [id: string, kind: MaskShape];
  /** When it is released: a settle frame and a save. */
  commit: [id: string];
  /** Painting: the first point of a stroke, each further point, and the release. */
  'stroke-start': [point: [number, number]];
  'stroke-point': [point: [number, number]];
  'stroke-end': [];
}>();

const svgRef = ref<SVGSVGElement | null>(null);
/** SVG units per screen pixel, so handles keep a fixed on-screen size. */
const unitsPerPx = ref(1);

type Pt = [number, number];

// --- the map and its inverse ------------------------------------------------------

const inverse = computed<Affine>(() => {
  const [a, b, c, d, e, f] = props.toStored;
  const det = a * e - b * d || 1e-12;
  return [e / det, -b / det, (b * f - c * e) / det, -d / det, a / det, (c * d - a * f) / det];
});

/** Stored-normalised → SVG units (frame pixels). */
function toView([us, vs]: Pt): Pt {
  const [a, b, c, d, e, f] = inverse.value;
  return [(a * us + b * vs + c) * props.frameWidth, (d * us + e * vs + f) * props.frameHeight];
}

/** SVG units → stored-normalised. */
function toStoredPt([x, y]: Pt): Pt {
  const [a, b, c, d, e, f] = props.toStored;
  const u = x / props.frameWidth;
  const v = y / props.frameHeight;
  return [a * u + b * v + c, d * u + e * v + f];
}

/** Stored-normalised ↔ the aspect-scaled space shapes are measured in. */
const toAspect = ([u, v]: Pt): Pt => [u * props.aspect.ax, v * props.aspect.ay];
const fromAspect = ([x, y]: Pt): Pt => [x / props.aspect.ax, y / props.aspect.ay];

// --- geometry of the drawn shapes ---------------------------------------------------

/** Endpoints of the line through `p`, perpendicular to `start → end` in aspect space. */
function perpendicular(p: Pt, start: Pt, end: Pt): [Pt, Pt] {
  const [sx, sy] = toAspect(start);
  const [ex, ey] = toAspect(end);
  let [dx, dy] = [-(ey - sy), ex - sx];
  const len = Math.hypot(dx, dy) || 1;
  [dx, dy] = [(dx / len) * 4, (dy / len) * 4]; // far beyond any frame; the SVG clips it
  const [px, py] = toAspect(p);
  return [toView(fromAspect([px - dx, py - dy])), toView(fromAspect([px + dx, py + dy]))];
}

function ellipsePoints(
  kind: Extract<MaskShape, { type: 'radial' }>,
  scale: number,
): string {
  const [cx, cy] = toAspect(kind.center);
  const t = (kind.angle * Math.PI) / 180;
  const [cos, sin] = [Math.cos(t), Math.sin(t)];
  const pts: string[] = [];
  for (let i = 0; i < 72; i++) {
    const th = (i / 72) * Math.PI * 2;
    const ex = kind.radius_x * scale * Math.cos(th);
    const ey = kind.radius_y * scale * Math.sin(th);
    const [vx, vy] = toView(fromAspect([cx + ex * cos - ey * sin, cy + ex * sin + ey * cos]));
    pts.push(`${vx.toFixed(2)},${vy.toFixed(2)}`);
  }
  return pts.join(' ');
}

/** The point on a radial's own axis at `along` × radius_x (0°) or radius_y (90°). */
function radialAxisPoint(kind: Extract<MaskShape, { type: 'radial' }>, axis: 'x' | 'y'): Pt {
  const [cx, cy] = toAspect(kind.center);
  const t = (kind.angle * Math.PI) / 180;
  const [cos, sin] = [Math.cos(t), Math.sin(t)];
  const [ex, ey] = axis === 'x' ? [kind.radius_x, 0] : [0, kind.radius_y];
  return fromAspect([cx + ex * cos - ey * sin, cy + ex * sin + ey * cos]);
}

const selected = computed(() => props.masks.find((m) => m.id === props.selectedId) ?? null);

const linearEdges = computed(() => {
  const k = selected.value?.kind;
  if (!k || k.type !== 'linear') return [];
  return [perpendicular(k.start, k.start, k.end), perpendicular(k.end, k.start, k.end)];
});

const radialRings = computed(() => {
  const k = selected.value?.kind;
  if (!k || k.type !== 'radial') return [];
  return [ellipsePoints(k, 1), ellipsePoints(k, 1 - k.feather)];
});

/** Where an unselected mask's pin sits, to select it by clicking; none for an unpainted brush. */
function anchorOf(m: OverlayMask): Pt | null {
  if (m.kind.type === 'radial') return toView(m.kind.center);
  if (m.kind.type === 'auto') return null;
  if (m.kind.type === 'brush') {
    const pts = m.strokes?.[0]?.points;
    return pts?.length ? toView(pts[Math.floor(pts.length / 2)]) : null;
  }
  const [s, e] = [m.kind.start, m.kind.end];
  return toView([(s[0] + e[0]) / 2, (s[1] + e[1]) / 2]);
}

// --- dragging -------------------------------------------------------------------------

type Handle = 'start' | 'end' | 'move' | 'center' | 'rx' | 'ry';
let drag: { id: string; handle: Handle; from: Pt; kind: MaskShape } | null = null;

function svgPoint(e: PointerEvent): Pt | null {
  const svg = svgRef.value;
  const ctm = svg?.getScreenCTM();
  if (!svg || !ctm) return null;
  const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
  return [p.x, p.y];
}

function onHandleDown(e: PointerEvent, handle: Handle) {
  const m = selected.value;
  const p = svgPoint(e);
  if (!m || !p) return;
  e.preventDefault();
  e.stopPropagation();
  svgRef.value?.setPointerCapture(e.pointerId);
  drag = { id: m.id, handle, from: toStoredPt(p), kind: JSON.parse(JSON.stringify(m.kind)) };
}

/** The shape `kind` with `handle` moved to stored point `to`, from a drag that began at `from`. */
function moved(kind: MaskShape, handle: Handle, from: Pt, to: Pt): MaskShape {
  const d: Pt = [to[0] - from[0], to[1] - from[1]];
  const shift = (p: Pt): [number, number] => [p[0] + d[0], p[1] + d[1]];
  if (kind.type === 'linear') {
    if (handle === 'start') return { ...kind, start: shift(kind.start) };
    if (handle === 'end') return { ...kind, end: shift(kind.end) };
    return { ...kind, start: shift(kind.start), end: shift(kind.end) };
  }
  if (kind.type === 'brush' || kind.type === 'auto') return kind; // painted, not dragged
  if (handle === 'center') return { ...kind, center: shift(kind.center) };
  const [cx, cy] = toAspect(kind.center);
  const [px, py] = toAspect(to);
  const [vx, vy] = [px - cx, py - cy];
  const r = Math.max(Math.hypot(vx, vy), 0.005);
  if (handle === 'rx') {
    // The width handle also turns the ellipse, as in every darkroom application.
    return { ...kind, radius_x: r, angle: (Math.atan2(vy, vx) * 180) / Math.PI };
  }
  // The height handle sits at 90° to the width axis: measure along that axis only.
  const t = ((kind.angle + 90) * Math.PI) / 180;
  return { ...kind, radius_y: Math.max(Math.abs(vx * Math.cos(t) + vy * Math.sin(t)), 0.005) };
}

// --- painting -----------------------------------------------------------------------

const cursor = ref<Pt | null>(null);
let painting = false;
let lastPainted: Pt | null = null;

function onSurfaceDown(e: PointerEvent) {
  if (!props.paint || e.button !== 0) return;
  const p = svgPoint(e);
  if (!p) return;
  e.preventDefault();
  svgRef.value?.setPointerCapture(e.pointerId);
  painting = true;
  lastPainted = toStoredPt(p);
  emit('stroke-start', lastPainted);
}

function onPointerMove(e: PointerEvent) {
  const p = svgPoint(e);
  if (!p) return;
  if (props.paint) cursor.value = toStoredPt(p);
  if (painting && props.paint && lastPainted) {
    const next = toStoredPt(p);
    const [ax, ay] = toAspect(next);
    const [lx, ly] = toAspect(lastPainted);
    if (Math.hypot(ax - lx, ay - ly) >= props.paint * 0.25) {
      lastPainted = next;
      emit('stroke-point', next);
    }
    return;
  }
  if (!drag) return;
  emit('update', drag.id, moved(drag.kind, drag.handle, drag.from, toStoredPt(p)));
}

function onPointerUp(e: PointerEvent) {
  if (painting) {
    painting = false;
    lastPainted = null;
    svgRef.value?.releasePointerCapture?.(e.pointerId);
    emit('stroke-end');
    return;
  }
  if (!drag) return;
  svgRef.value?.releasePointerCapture?.(e.pointerId);
  const id = drag.id;
  drag = null;
  emit('commit', id);
}

/** Arrow keys move a handle by 1% of the frame; Shift by 5%. */
function onHandleKey(e: KeyboardEvent, handle: Handle) {
  const m = selected.value;
  const steps: Record<string, Pt> = {
    ArrowLeft: [-1, 0],
    ArrowRight: [1, 0],
    ArrowUp: [0, -1],
    ArrowDown: [0, 1],
  };
  const dir = steps[e.key];
  if (!m || !dir) return;
  e.preventDefault();
  const step = e.shiftKey ? 0.05 : 0.01;
  const anchor = handleAnchor(m.kind, handle);
  const fromView = toView(anchor);
  const to = toStoredPt([
    fromView[0] + dir[0] * step * props.frameWidth,
    fromView[1] + dir[1] * step * props.frameHeight,
  ]);
  emit('update', m.id, moved(m.kind, handle, anchor, to));
  emit('commit', m.id);
}

function handleAnchor(kind: MaskShape, handle: Handle): Pt {
  if (kind.type === 'linear') {
    if (handle === 'start') return kind.start;
    if (handle === 'end') return kind.end;
    return [(kind.start[0] + kind.end[0]) / 2, (kind.start[1] + kind.end[1]) / 2];
  }
  if (kind.type === 'brush' || kind.type === 'auto') return [0.5, 0.5];
  if (handle === 'rx') return radialAxisPoint(kind, 'x');
  if (handle === 'ry') return radialAxisPoint(kind, 'y');
  return kind.center;
}

// --- handle size --------------------------------------------------------------------

function measure() {
  const ctm = svgRef.value?.getScreenCTM();
  if (ctm) unitsPerPx.value = 1 / (Math.hypot(ctm.a, ctm.b) || 1);
}
let resizeObserver: ResizeObserver | null = null;
onMounted(() => {
  measure();
  if (svgRef.value) {
    resizeObserver = new ResizeObserver(measure);
    resizeObserver.observe(svgRef.value);
  }
});
onBeforeUnmount(() => resizeObserver?.disconnect());
watch(() => [props.frameWidth, props.frameHeight], () => requestAnimationFrame(measure));

/** 20 px radius: a 40 px target (CLAUDE.md design rules). */
const hitR = computed(() => 20 * unitsPerPx.value);
const dotR = computed(() => 7 * unitsPerPx.value);

/** The brush's footprint under the pointer, drawn as the photograph will see it. */
const brushRing = computed(() => {
  if (!props.paint || !cursor.value) return '';
  return ellipsePoints(
    { type: 'radial', center: cursor.value, radius_x: props.paint, radius_y: props.paint, angle: 0, feather: 0 },
    1,
  );
});

const handles = computed(() => {
  const m = selected.value;
  if (!m || props.paint || m.kind.type === 'brush' || m.kind.type === 'auto') return [];
  const kind = m.kind;
  const list: { handle: Handle; at: Pt; label: string }[] =
    kind.type === 'linear'
      ? [
          { handle: 'start', at: toView(kind.start), label: 'Gradient start' },
          { handle: 'end', at: toView(kind.end), label: 'Gradient end' },
          { handle: 'move', at: toView(handleAnchor(kind, 'move')), label: 'Move gradient' },
        ]
      : [
          { handle: 'center', at: toView(kind.center), label: 'Move radial mask' },
          { handle: 'rx', at: toView(radialAxisPoint(kind, 'x')), label: 'Width and angle' },
          { handle: 'ry', at: toView(radialAxisPoint(kind, 'y')), label: 'Height' },
        ];
  return list;
});
</script>

<template>
  <svg
    ref="svgRef"
    class="mask-overlay"
    data-testid="mask-overlay"
    :viewBox="`0 0 ${frameWidth} ${frameHeight}`"
    preserveAspectRatio="none"
    :class="{ 'is-painting': !!paint }"
    @pointerdown="onSurfaceDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
    @pointerleave="cursor = null"
  >
    <!-- Pins for the masks not being shaped: click to shape one. -->
    <g v-for="m in masks" :key="`pin-${m.id}`">
      <circle
        v-if="m.id !== selectedId && anchorOf(m) && !paint"
        class="mask-overlay__pin"
        :cx="anchorOf(m)![0]"
        :cy="anchorOf(m)![1]"
        :r="hitR"
        role="button"
        tabindex="0"
        :aria-label="`Select mask ${m.name}`"
        :data-testid="`mask-pin-${m.id}`"
        @pointerdown.stop.prevent="emit('select', m.id)"
        @keydown.enter.prevent="emit('select', m.id)"
      />
    </g>

    <polygon
      v-for="layer in brushRing ? ['under', 'over'] : []"
      :key="`brush-${layer}`"
      class="mask-overlay__line"
      :class="`mask-overlay__line--${layer}`"
      :points="brushRing"
      :data-testid="layer === 'over' ? 'brush-cursor' : undefined"
    />

    <template v-if="selected && !paint">
      <!-- Each line is drawn twice, dark under light, so it shows on any photograph
           without a shadow (the design rules allow none). -->
      <template v-if="selected.kind.type === 'linear'">
        <template v-for="(line, i) in linearEdges" :key="`edge-${i}`">
          <line
            v-for="layer in ['under', 'over']"
            :key="layer"
            class="mask-overlay__line"
            :class="[`mask-overlay__line--${layer}`, { 'mask-overlay__line--end': i === 1 }]"
            :x1="line[0][0]"
            :y1="line[0][1]"
            :x2="line[1][0]"
            :y2="line[1][1]"
            :data-testid="layer === 'over' ? (i === 0 ? 'mask-start-line' : 'mask-end-line') : undefined"
          />
        </template>
      </template>
      <template v-else>
        <template v-for="(ring, i) in radialRings" :key="`ring-${i}`">
          <polygon
            v-for="layer in ['under', 'over']"
            :key="layer"
            class="mask-overlay__line"
            :class="[`mask-overlay__line--${layer}`, { 'mask-overlay__line--end': i === 1 }]"
            :points="ring"
            :data-testid="layer === 'over' ? (i === 0 ? 'mask-ellipse' : 'mask-feather-ellipse') : undefined"
          />
        </template>
      </template>

      <g
        v-for="h in handles"
        :key="h.handle"
        class="mask-overlay__handle"
        role="slider"
        tabindex="0"
        :aria-label="h.label"
        :data-testid="`mask-handle-${h.handle}`"
        @pointerdown="onHandleDown($event, h.handle)"
        @keydown="onHandleKey($event, h.handle)"
      >
        <circle class="mask-overlay__hit" :cx="h.at[0]" :cy="h.at[1]" :r="hitR" />
        <circle class="mask-overlay__dot" :cx="h.at[0]" :cy="h.at[1]" :r="dotR" />
      </g>
    </template>
  </svg>
</template>

<style scoped>
.mask-overlay {
  position: absolute;
  top: 0;
  left: 0;
  overflow: hidden;
  touch-action: none;
}

.mask-overlay.is-painting {
  cursor: crosshair;
}

.mask-overlay__line {
  fill: none;
  vector-effect: non-scaling-stroke;
  pointer-events: none;
}

.mask-overlay__line--under {
  stroke: var(--void);
  stroke-width: 3.5;
}

.mask-overlay__line--over {
  stroke: var(--white);
  stroke-width: 1.5;
}

.mask-overlay__line--end {
  stroke-dasharray: 6 4;
}

.mask-overlay__handle {
  cursor: grab;
  outline: none;
}

.mask-overlay__hit {
  fill: transparent;
}

.mask-overlay__dot {
  fill: var(--void);
  stroke: var(--white);
  stroke-width: 2;
  vector-effect: non-scaling-stroke;
}

.mask-overlay__handle:focus-visible .mask-overlay__dot,
.mask-overlay__handle:hover .mask-overlay__dot {
  stroke: var(--accent);
}

.mask-overlay__pin {
  fill: transparent;
  stroke: var(--white);
  stroke-width: 1.5;
  stroke-dasharray: 3 3;
  vector-effect: non-scaling-stroke;
  cursor: pointer;
}
</style>
