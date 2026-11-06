<script setup lang="ts">
/**
 * 3-way colour grading wheel component (ED-12).
 *
 * Citations:
 * - American Society of Cinematographers (ASC) Color Decision List (CDL).
 * - Perceptual OkLCh color space for hue ring knots matching 8-band HSL swatches.
 *
 * Transport-free: props in, events out, never `@host/api`.
 *
 * Requirements:
 * - Content circles and hue ring drawn with SVG (content, OkLCh colors), not CSS border-radius.
 * - Draggable reticle with pointer capture, keyboard accessible (Arrow keys, Shift for 5x step).
 * - Numeric readouts for angle and intensity/saturation.
 * - Double-click resets hue/saturation to neutral identity.
 * - Luminance slider (-100% to +100%) underneath wheel with double-click reset.
 * - Hit targets >= 40px; controls 16px text. No drop shadows.
 */
import { computed, ref } from 'vue';
import type { ColorWheel } from '@phototools/shared';
import AdjustmentSlider from './AdjustmentSlider.vue';

const props = withDefaults(
  defineProps<{
    modelValue?: ColorWheel | null;
    label?: string;
    disabled?: boolean;
    testId?: string;
  }>(),
  {
    modelValue: () => ({ hue: 0, saturation: 0, luminance: 0 }),
    label: '',
    disabled: false,
    testId: 'color-wheel',
  },
);

const emit = defineEmits<{
  'update:modelValue': [val: ColorWheel];
  change: [val: ColorWheel];
}>();

const svgRef = ref<SVGSVGElement | null>(null);
const isDragging = ref(false);

const currentWheel = computed<ColorWheel>(() => ({
  hue: props.modelValue?.hue ?? 0,
  saturation: props.modelValue?.saturation ?? 0,
  luminance: props.modelValue?.luminance ?? 0,
}));

// SVG wheel radius and center
const CX = 80;
const CY = 80;
const RADIUS = 60;
const INNER_RADIUS = 50;

// OkLCh reference hue knots matching 8-band HSL swatches
// Generate conical gradient stops for the hue ring
const conicGradient = computed(() => {
  return [
    'oklch(0.7 0.2 29.23deg) 29.23deg',
    'oklch(0.7 0.2 52.78deg) 52.78deg',
    'oklch(0.7 0.2 109.77deg) 109.77deg',
    'oklch(0.7 0.2 142.50deg) 142.50deg',
    'oklch(0.7 0.2 194.77deg) 194.77deg',
    'oklch(0.7 0.2 264.05deg) 264.05deg',
    'oklch(0.7 0.2 293.77deg) 293.77deg',
    'oklch(0.7 0.2 328.36deg) 328.36deg',
    'oklch(0.7 0.2 29.23deg) 389.23deg',
  ].join(', ');
});

// Reticle position in SVG coordinate space
const reticlePos = computed(() => {
  const sat = Math.max(0, Math.min(1, currentWheel.value.saturation));
  const rad = (currentWheel.value.hue * Math.PI) / 180;
  const dist = sat * INNER_RADIUS;
  return {
    x: CX + dist * Math.cos(rad),
    y: CY + dist * Math.sin(rad),
  };
});

function updateFromPointer(e: PointerEvent, isFinal: boolean) {
  if (props.disabled || !svgRef.value) return;
  const rect = svgRef.value.getBoundingClientRect();
  const scale = 160 / rect.width;
  const clientX = (e.clientX - rect.left) * scale;
  const clientY = (e.clientY - rect.top) * scale;

  const dx = clientX - CX;
  const dy = clientY - CY;
  const dist = Math.sqrt(dx * dx + dy * dy);

  let hueDeg = (Math.atan2(dy, dx) * 180) / Math.PI;
  if (hueDeg < 0) hueDeg += 360;

  const sat = Math.max(0, Math.min(1, dist / INNER_RADIUS));

  const updated: ColorWheel = {
    ...currentWheel.value,
    hue: Math.round(hueDeg * 10) / 10,
    saturation: Math.round(sat * 1000) / 1000,
  };

  emit('update:modelValue', updated);
  if (isFinal) {
    emit('change', updated);
  }
}

function onPointerDown(e: PointerEvent) {
  if (props.disabled || !svgRef.value) return;
  e.preventDefault();
  isDragging.value = true;
  svgRef.value.setPointerCapture(e.pointerId);
  updateFromPointer(e, false);
}

function onPointerMove(e: PointerEvent) {
  if (!isDragging.value) return;
  updateFromPointer(e, false);
}

function onPointerUp(e: PointerEvent) {
  if (!isDragging.value) return;
  isDragging.value = false;
  if (svgRef.value && svgRef.value.hasPointerCapture(e.pointerId)) {
    svgRef.value.releasePointerCapture(e.pointerId);
  }
  updateFromPointer(e, true);
}

function onDoubleClick() {
  if (props.disabled) return;
  // Reset hue and saturation to identity 0, preserving luminance
  const reset: ColorWheel = {
    ...currentWheel.value,
    hue: 0,
    saturation: 0,
  };
  emit('update:modelValue', reset);
  emit('change', reset);
}

function onKeyDown(e: KeyboardEvent) {
  if (props.disabled) return;
  const stepHue = e.shiftKey ? 5 : 1;
  const stepSat = e.shiftKey ? 0.05 : 0.01;
  let newHue = currentWheel.value.hue;
  let newSat = currentWheel.value.saturation;
  let handled = false;

  switch (e.key) {
    case 'ArrowLeft':
      newHue = (newHue - stepHue + 360) % 360;
      handled = true;
      break;
    case 'ArrowRight':
      newHue = (newHue + stepHue) % 360;
      handled = true;
      break;
    case 'ArrowUp':
      newSat = Math.min(1, newSat + stepSat);
      handled = true;
      break;
    case 'ArrowDown':
      newSat = Math.max(0, newSat - stepSat);
      handled = true;
      break;
    case 'Home':
    case 'Delete':
    case 'Backspace':
      newHue = 0;
      newSat = 0;
      handled = true;
      break;
  }

  if (handled) {
    e.preventDefault();
    const updated: ColorWheel = {
      ...currentWheel.value,
      hue: Math.round(newHue * 10) / 10,
      saturation: Math.round(newSat * 1000) / 1000,
    };
    emit('update:modelValue', updated);
    emit('change', updated);
  }
}

function onLuminanceInput(v: number) {
  const updated: ColorWheel = {
    ...currentWheel.value,
    luminance: Math.round((v / 100) * 1000) / 1000,
  };
  emit('update:modelValue', updated);
}

function onLuminanceChange(v: number) {
  const updated: ColorWheel = {
    ...currentWheel.value,
    luminance: Math.round((v / 100) * 1000) / 1000,
  };
  emit('change', updated);
}
</script>

<template>
  <div
    class="color-wheel-wrapper"
    :class="{ 'color-wheel--disabled': props.disabled }"
    :data-testid="props.testId"
  >
    <div class="color-wheel__header">
      <span v-if="props.label" class="color-wheel__title">{{ props.label }}</span>
      <div class="color-wheel__readouts">
        <span class="color-wheel__readout-val" :data-testid="`${props.testId}-readout-hue`">
          {{ Math.round(currentWheel.hue) }}°
        </span>
        <span class="color-wheel__readout-sep">/</span>
        <span class="color-wheel__readout-val" :data-testid="`${props.testId}-readout-sat`">
          {{ Math.round(currentWheel.saturation * 100) }}%
        </span>
      </div>
    </div>

    <div class="color-wheel__disc-container">
      <svg
        ref="svgRef"
        class="color-wheel__svg"
        viewBox="0 0 160 160"
        tabindex="0"
        role="slider"
        :aria-label="`${props.label || 'Color'} grading wheel`"
        :aria-valuenow="Math.round(currentWheel.hue)"
        aria-valuemin="0"
        aria-valuemax="360"
        :data-testid="`${props.testId}-svg`"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @pointercancel="onPointerUp"
        @dblclick="onDoubleClick"
        @keydown="onKeyDown"
      >
        <defs>
          <!-- Radial gradient: neutral center to perimeter saturation -->
          <radialGradient id="center-fade" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stop-color="var(--canvas-surround)" stop-opacity="1" />
            <stop offset="60%" stop-color="var(--canvas-surround)" stop-opacity="0.6" />
            <stop offset="100%" stop-color="var(--canvas-surround)" stop-opacity="0" />
          </radialGradient>
          <clipPath :id="`${props.testId}-clip`">
            <circle :cx="CX" :cy="CY" :r="RADIUS" />
          </clipPath>
        </defs>

        <!-- Outer border ring -->
        <circle
          :cx="CX"
          :cy="CY"
          :r="RADIUS"
          fill="none"
          stroke="var(--border)"
          stroke-width="1"
        />

        <!-- Outer Hue ring rendered with foreignObject conic gradient for smooth spectral transitions -->
        <foreignObject
          :x="CX - RADIUS"
          :y="CY - RADIUS"
          :width="RADIUS * 2"
          :height="RADIUS * 2"
          class="color-wheel__foreign"
          :clip-path="`url(#${props.testId}-clip)`"
        >
          <div
            class="color-wheel__gradient-ring"
            :style="{ background: `conic-gradient(from 0deg at 50% 50%, ${conicGradient})` }"
          />
        </foreignObject>

        <!-- Inner neutral core covering center -->
        <circle
          :cx="CX"
          :cy="CY"
          :r="INNER_RADIUS"
          fill="var(--bg-panel)"
          stroke="none"
        />

        <!-- Soft radial falloff from center -->
        <circle
          :cx="CX"
          :cy="CY"
          :r="INNER_RADIUS"
          fill="url(#center-fade)"
          stroke="none"
        />

        <!-- Inner boundary circle -->
        <circle
          :cx="CX"
          :cy="CY"
          :r="INNER_RADIUS"
          fill="none"
          stroke="var(--border)"
          stroke-width="1"
        />

        <!-- Crosshair lines -->
        <line
          :x1="CX - INNER_RADIUS"
          :y1="CY"
          :x2="CX + INNER_RADIUS"
          :y2="CY"
          stroke="var(--border)"
          stroke-width="1"
          stroke-opacity="0.3"
        />
        <line
          :x1="CX"
          :y1="CY - INNER_RADIUS"
          :x2="CX"
          :y2="CY + INNER_RADIUS"
          stroke="var(--border)"
          stroke-width="1"
          stroke-opacity="0.3"
        />

        <!-- Center identity dot -->
        <circle
          :cx="CX"
          :cy="CY"
          r="2.5"
          fill="var(--text-muted)"
        />

        <!-- Draggable reticle: circle content with contrast border, no drop shadows -->
        <g
          class="color-wheel__reticle-group"
          :transform="`translate(${reticlePos.x}, ${reticlePos.y})`"
          :data-testid="`${props.testId}-reticle`"
        >
          <!-- Outer contrast halo -->
          <circle
            cx="0"
            cy="0"
            r="7"
            fill="none"
            stroke="var(--bg)"
            stroke-width="2.5"
          />
          <!-- Inner reticle ring -->
          <circle
            cx="0"
            cy="0"
            r="6"
            fill="none"
            stroke="var(--text)"
            stroke-width="1.5"
          />
          <!-- Center indicator dot -->
          <circle
            v-if="currentWheel.saturation > 0.05"
            cx="0"
            cy="0"
            r="2"
            fill="var(--text)"
          />
        </g>
      </svg>
    </div>

    <!-- Integrated Luminance slider -->
    <div class="color-wheel__slider">
      <AdjustmentSlider
        label="Luminance"
        :model-value="Math.round(currentWheel.luminance * 100)"
        :min="-100"
        :max="100"
        :step="1"
        unit="%"
        :disabled="props.disabled"
        :test-id="`${props.testId}-slider-lum`"
        @update:model-value="onLuminanceInput"
        @change="onLuminanceChange"
      />
    </div>
  </div>
</template>

<style scoped>
.color-wheel-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
}

.color-wheel--disabled {
  opacity: 0.5;
  pointer-events: none;
}

.color-wheel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  min-height: 28px;
}

.color-wheel__title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.color-wheel__readouts {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}

.color-wheel__readout-val {
  color: var(--text);
  min-width: 28px;
  text-align: right;
}

.color-wheel__readout-sep {
  opacity: 0.4;
}

.color-wheel__disc-container {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 160px;
  height: 160px;
  touch-action: none;
  cursor: crosshair;
}

.color-wheel__svg {
  width: 160px;
  height: 160px;
  overflow: visible;
  outline: none;
}

.color-wheel__svg:focus-visible circle[stroke='var(--border)'] {
  stroke: var(--border-active);
  stroke-width: 2;
}

.color-wheel__foreign {
  overflow: hidden;
  pointer-events: none;
}

.color-wheel__gradient-ring {
  width: 100%;
  height: 100%;
}

.color-wheel__reticle-group {
  cursor: grab;
  transition: transform var(--dur-fast) var(--ease);
}

.color-wheel-wrapper:active .color-wheel__reticle-group {
  cursor: grabbing;
}

.color-wheel__slider {
  width: 100%;
}
</style>
