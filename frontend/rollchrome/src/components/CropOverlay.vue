<script setup lang="ts">
/**
 * Interactive crop overlay component (ED-13).
 *
 * Citations & Principles:
 * - Transport-free: props in, events out, never `@host/api`.
 * - Shows interactive crop box with darkened surround (token `var(--bg)`),
 *   rule-of-thirds grid, and 8 hit targets >= 40px.
 * - Keyboard accessible: Enter applies, Esc cancels, Arrow keys nudge (Shift for 5x).
 * - Tokens only, no fallbacks, no radius above 2px.
 */
import { computed, onMounted, ref } from 'vue';
import type { NormalizedCrop } from '@phototools/shared';

const props = withDefaults(
  defineProps<{
    modelValue?: NormalizedCrop | null;
    containerWidth: number;
    containerHeight: number;
    aspectRatio?: number | null;
  }>(),
  {
    modelValue: () => ({ x: 0, y: 0, width: 1, height: 1 }),
    aspectRatio: null,
  },
);

const emit = defineEmits<{
  'update:modelValue': [val: NormalizedCrop];
  apply: [val: NormalizedCrop];
  cancel: [];
}>();

const rootRef = ref<HTMLDivElement | null>(null);

onMounted(() => {
  rootRef.value?.focus();
});

const crop = computed<NormalizedCrop>(() => {
  const c = props.modelValue;
  if (!c) return { x: 0, y: 0, width: 1, height: 1 };
  return {
    x: Math.max(0, Math.min(1, c.x)),
    y: Math.max(0, Math.min(1, c.y)),
    width: Math.max(0.01, Math.min(1, c.width)),
    height: Math.max(0.01, Math.min(1, c.height)),
  };
});

// Pixel coordinates relative to container
const boxPx = computed(() => {
  const cw = props.containerWidth;
  const ch = props.containerHeight;
  const x = Math.round(crop.value.x * cw);
  const y = Math.round(crop.value.y * ch);
  const w = Math.max(16, Math.round(crop.value.width * cw));
  const h = Math.max(16, Math.round(crop.value.height * ch));
  return { x, y, w, h };
});

// SVG path for darkened mask with evenodd fill rule
const maskPath = computed(() => {
  const cw = props.containerWidth;
  const ch = props.containerHeight;
  const { x, y, w, h } = boxPx.value;
  return `M 0 0 H ${cw} V ${ch} H 0 Z M ${x} ${y} H ${x + w} V ${y + h} H ${x} Z`;
});

// Rule-of-thirds grid line coordinates
const gridLines = computed(() => {
  const { x, y, w, h } = boxPx.value;
  return {
    v1: x + Math.round(w / 3),
    v2: x + Math.round((2 * w) / 3),
    h1: y + Math.round(h / 3),
    h2: y + Math.round((2 * h) / 3),
  };
});

// 8 handle positions in pixels
type HandleType = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w' | 'move';

const handles = computed(() => {
  const { x, y, w, h } = boxPx.value;
  return [
    { type: 'nw' as HandleType, x, y, cursor: 'nwse-resize' },
    { type: 'n' as HandleType, x: x + w / 2, y, cursor: 'ns-resize' },
    { type: 'ne' as HandleType, x: x + w, y, cursor: 'nesw-resize' },
    { type: 'e' as HandleType, x: x + w, y: y + h / 2, cursor: 'ew-resize' },
    { type: 'se' as HandleType, x: x + w, y: y + h, cursor: 'nwse-resize' },
    { type: 's' as HandleType, x: x + w / 2, y: y + h, cursor: 'ns-resize' },
    { type: 'sw' as HandleType, x, y: y + h, cursor: 'nesw-resize' },
    { type: 'w' as HandleType, x, y: y + h / 2, cursor: 'ew-resize' },
  ];
});

// Pointer drag tracking
const activeDrag = ref<{
  type: HandleType;
  startX: number;
  startY: number;
  initialCrop: NormalizedCrop;
} | null>(null);

function startDrag(e: PointerEvent, type: HandleType) {
  e.preventDefault();
  e.stopPropagation();
  (e.currentTarget as Element)?.setPointerCapture?.(e.pointerId);

  activeDrag.value = {
    type,
    startX: e.clientX,
    startY: e.clientY,
    initialCrop: { ...crop.value },
  };
}

function onPointerMove(e: PointerEvent) {
  if (!activeDrag.value) return;
  e.preventDefault();

  const { type, startX, startY, initialCrop } = activeDrag.value;
  const cw = props.containerWidth;
  const ch = props.containerHeight;
  if (cw <= 0 || ch <= 0) return;

  const dxNorm = (e.clientX - startX) / cw;
  const dyNorm = (e.clientY - startY) / ch;

  const minWNorm = 16 / cw;
  const minHNorm = 16 / ch;

  let newX = initialCrop.x;
  let newY = initialCrop.y;
  let newW = initialCrop.width;
  let newH = initialCrop.height;

  if (type === 'move') {
    newX = Math.max(0, Math.min(1 - newW, initialCrop.x + dxNorm));
    newY = Math.max(0, Math.min(1 - newH, initialCrop.y + dyNorm));
  } else {
    // Handle resizing
    let left = initialCrop.x;
    let top = initialCrop.y;
    let right = initialCrop.x + initialCrop.width;
    let bottom = initialCrop.y + initialCrop.height;

    if (type.includes('w')) {
      left = Math.min(right - minWNorm, Math.max(0, left + dxNorm));
    }
    if (type.includes('e')) {
      right = Math.max(left + minWNorm, Math.min(1, right + dxNorm));
    }
    if (type.includes('n')) {
      top = Math.min(bottom - minHNorm, Math.max(0, top + dyNorm));
    }
    if (type.includes('s')) {
      bottom = Math.max(top + minHNorm, Math.min(1, bottom + dyNorm));
    }

    // Aspect ratio locking if specified
    if (props.aspectRatio && props.aspectRatio > 0) {
      const targetRatio = props.aspectRatio; // target = (w_px) / (h_px) = (w_norm * cw) / (h_norm * ch)
      let wPx = (right - left) * cw;
      let hPx = (bottom - top) * ch;

      if (type === 'w' || type === 'e') {
        hPx = wPx / targetRatio;
        const cy = (top + bottom) * 0.5 * ch;
        top = Math.max(0, (cy - hPx * 0.5) / ch);
        bottom = Math.min(1, (cy + hPx * 0.5) / ch);
      } else if (type === 'n' || type === 's') {
        wPx = hPx * targetRatio;
        const cx = (left + right) * 0.5 * cw;
        left = Math.max(0, (cx - wPx * 0.5) / cw);
        right = Math.min(1, (cx + wPx * 0.5) / cw);
      } else {
        // Corner handles: fit to aspect ratio
        const cornerW = wPx;
        const cornerH = cornerW / targetRatio;
        if (type.includes('n')) {
          top = Math.max(0, bottom - cornerH / ch);
        } else {
          bottom = Math.min(1, top + cornerH / ch);
        }
      }
    }

    newX = left;
    newY = top;
    newW = Math.max(minWNorm, right - left);
    newH = Math.max(minHNorm, bottom - top);
  }

  const updated: NormalizedCrop = {
    x: Number(newX.toFixed(4)),
    y: Number(newY.toFixed(4)),
    width: Number(newW.toFixed(4)),
    height: Number(newH.toFixed(4)),
  };

  emit('update:modelValue', updated);
}

function onPointerUp(e: PointerEvent) {
  if (activeDrag.value) {
    (e.currentTarget as Element)?.releasePointerCapture?.(e.pointerId);
    activeDrag.value = null;
  }
}

function onKeyDown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault();
    emit('apply', crop.value);
    return;
  }
  if (e.key === 'Escape') {
    e.preventDefault();
    emit('cancel');
    return;
  }

  const cw = props.containerWidth;
  const ch = props.containerHeight;
  if (cw <= 0 || ch <= 0) return;

  const stepPx = e.shiftKey ? 10 : 2;
  const dxNorm = stepPx / cw;
  const dyNorm = stepPx / ch;

  let moved = false;
  let newX = crop.value.x;
  let newY = crop.value.y;

  if (e.key === 'ArrowLeft') {
    e.preventDefault();
    newX = Math.max(0, newX - dxNorm);
    moved = true;
  } else if (e.key === 'ArrowRight') {
    e.preventDefault();
    newX = Math.min(1 - crop.value.width, newX + dxNorm);
    moved = true;
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    newY = Math.max(0, newY - dyNorm);
    moved = true;
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    newY = Math.min(1 - crop.value.height, newY + dyNorm);
    moved = true;
  }

  if (moved) {
    emit('update:modelValue', {
      x: Number(newX.toFixed(4)),
      y: Number(newY.toFixed(4)),
      width: crop.value.width,
      height: crop.value.height,
    });
  }
}
</script>

<template>
  <div
    ref="rootRef"
    class="crop-overlay"
    tabindex="0"
    :style="{
      width: `${containerWidth}px`,
      height: `${containerHeight}px`,
    }"
    data-test="crop-overlay"
    @keydown="onKeyDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
  >
    <svg
      class="crop-overlay__svg"
      :width="containerWidth"
      :height="containerHeight"
      :viewBox="`0 0 ${containerWidth} ${containerHeight}`"
    >
      <!-- Darkened mask outside crop box (token var(--bg)) -->
      <path
        class="crop-overlay__mask"
        :d="maskPath"
        fill="var(--bg)"
        fill-opacity="0.65"
        fill-rule="evenodd"
      />

      <!-- Rule-of-thirds grid lines -->
      <g class="crop-overlay__grid" stroke="var(--text)" stroke-opacity="0.35" stroke-dasharray="3 3">
        <!-- Verticals -->
        <line
          :x1="gridLines.v1"
          :y1="boxPx.y"
          :x2="gridLines.v1"
          :y2="boxPx.y + boxPx.h"
        />
        <line
          :x1="gridLines.v2"
          :y1="boxPx.y"
          :x2="gridLines.v2"
          :y2="boxPx.y + boxPx.h"
        />
        <!-- Horizontals -->
        <line
          :x1="boxPx.x"
          :y1="gridLines.h1"
          :x2="boxPx.x + boxPx.w"
          :y2="gridLines.h1"
        />
        <line
          :x1="boxPx.x"
          :y1="gridLines.h2"
          :x2="boxPx.x + boxPx.w"
          :y2="gridLines.h2"
        />
      </g>

      <!-- Crop box border -->
      <rect
        class="crop-overlay__box"
        :x="boxPx.x"
        :y="boxPx.y"
        :width="boxPx.w"
        :height="boxPx.h"
        fill="none"
        stroke="var(--text-heading)"
        stroke-width="1.5"
      />

      <!-- Draggable interior -->
      <rect
        class="crop-overlay__interior"
        :x="boxPx.x"
        :y="boxPx.y"
        :width="boxPx.w"
        :height="boxPx.h"
        fill="transparent"
        style="cursor: move"
        data-test="crop-interior"
        @pointerdown="startDrag($event, 'move')"
      />

      <!-- 8 Handles (visual markers + >= 40px hit areas) -->
      <g v-for="h in handles" :key="h.type" class="crop-overlay__handle-group">
        <!-- Visual marker: <= 2px radius -->
        <rect
          class="crop-overlay__handle-visual"
          :x="h.x - 4"
          :y="h.y - 4"
          width="8"
          height="8"
          rx="1"
          fill="var(--text-heading)"
          stroke="var(--bg)"
          stroke-width="1"
        />
        <!-- Invisible hit target >= 40px -->
        <rect
          class="crop-overlay__handle-hit"
          :x="h.x - 20"
          :y="h.y - 20"
          width="40"
          height="40"
          fill="transparent"
          :style="{ cursor: h.cursor }"
          :data-test="`crop-handle-${h.type}`"
          @pointerdown="startDrag($event, h.type)"
        />
      </g>
    </svg>
  </div>
</template>

<style scoped>
.crop-overlay {
  position: absolute;
  top: 0;
  left: 0;
  outline: none;
  user-select: none;
  touch-action: none;
  z-index: calc(var(--z-canvas) + 1);
}

.crop-overlay__svg {
  display: block;
  width: 100%;
  height: 100%;
  pointer-events: auto;
}

.crop-overlay__mask {
  pointer-events: none;
}

.crop-overlay__grid {
  pointer-events: none;
}

.crop-overlay__box {
  pointer-events: none;
}

.crop-overlay__handle-visual {
  pointer-events: none;
}

.crop-overlay__handle-hit {
  pointer-events: auto;
}
</style>
