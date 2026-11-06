<script setup lang="ts">
/**
 * Photographic adjustment slider with double-click reset and numeric entry (ED-7).
 *
 * Transport-free: props in, events out, never `@host/api`.
 *
 * Controls:
 * - Double-click the label or the slider itself resets value to defaultValue (exact 0.0
 *   identity by default). The slider is where the hand already is after a drag; making
 *   the person travel to a small label to undo it was the slower of the two gestures.
 *   The first click of the pair may move the value to where it landed; the reset that
 *   follows makes that harmless.
 * - Range slider emits `update:modelValue` while scrubbing (drag proxy) and `change` on release (settle proxy).
 * - Number field provides exact keyboard entry with 16px font to prevent mobile zoom and >= 40px touch target.
 */
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    modelValue?: number;
    label: string;
    min: number;
    max: number;
    step?: number;
    unit?: string;
    defaultValue?: number;
    disabled?: boolean;
    testId?: string;
  }>(),
  {
    modelValue: 0,
    step: 1,
    unit: '',
    defaultValue: 0,
    disabled: false,
    testId: undefined,
  },
);

const emit = defineEmits<{
  'update:modelValue': [value: number];
  change: [value: number];
}>();

const formattedValue = computed(() => {
  const decimals = props.step < 1 ? 1 : 0;
  return Number(props.modelValue).toFixed(decimals);
});

function onSliderInput(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value);
  if (!Number.isNaN(val)) {
    emit('update:modelValue', val);
  }
}

function onSliderChange(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value);
  if (!Number.isNaN(val)) {
    emit('change', val);
  }
}

function onNumberInput(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value);
  if (!Number.isNaN(val)) {
    emit('update:modelValue', val);
  }
}

function onNumberChange(e: Event) {
  const val = parseFloat((e.target as HTMLInputElement).value);
  if (!Number.isNaN(val)) {
    emit('change', val);
  }
}

function resetToDefault() {
  if (props.disabled) return;
  emit('update:modelValue', props.defaultValue);
  emit('change', props.defaultValue);
}
</script>

<template>
  <div class="adjustment-slider" :class="{ 'adjustment-slider--disabled': props.disabled }">
    <div class="adjustment-slider__header">
      <span
        class="adjustment-slider__label"
        role="button"
        tabindex="0"
        :title="`Double-click to reset to ${props.defaultValue}`"
        :data-testid="props.testId ? `${props.testId}-label` : undefined"
        @dblclick="resetToDefault"
        @keydown.enter="resetToDefault"
      >
        {{ props.label }}
      </span>
      <span v-if="props.unit" class="adjustment-slider__unit">{{ props.unit }}</span>
    </div>

    <div class="adjustment-slider__controls">
      <input
        type="range"
        class="adjustment-slider__range"
        :value="props.modelValue"
        :min="props.min"
        :max="props.max"
        :step="props.step"
        :disabled="props.disabled"
        :data-testid="props.testId ? `${props.testId}-slider` : undefined"
        :title="`Double-click to reset to ${props.defaultValue}`"
        @dblclick="resetToDefault"
        @input="onSliderInput"
        @change="onSliderChange"
      />

      <input
        type="number"
        class="adjustment-slider__number"
        :value="formattedValue"
        :min="props.min"
        :max="props.max"
        :step="props.step"
        :disabled="props.disabled"
        :data-testid="props.testId ? `${props.testId}-number` : undefined"
        @input="onNumberInput"
        @change="onNumberChange"
      />
    </div>
  </div>
</template>

<style scoped>
.adjustment-slider {
  display: grid;
  gap: var(--space-1);
}

.adjustment-slider--disabled {
  opacity: 0.5;
  pointer-events: none;
}

.adjustment-slider__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.adjustment-slider__label {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
  cursor: pointer;
  user-select: none;
  transition: color var(--dur-fast) var(--ease);
}

.adjustment-slider__label:hover {
  color: var(--accent);
}

.adjustment-slider__unit {
  font-family: var(--font-body);
  font-size: 11px;
  color: var(--text-disabled);
}

.adjustment-slider__controls {
  display: grid;
  grid-template-columns: 1fr 76px;
  gap: var(--space-2);
  align-items: center;
}

.adjustment-slider__range {
  width: 100%;
  height: 44px;
  min-height: 44px;
  accent-color: var(--accent);
  background: transparent;
  cursor: pointer;
  margin: 0;
  padding: 0;
}

.adjustment-slider__number {
  width: 100%;
  min-height: 44px;
  height: 44px;
  font-family: var(--font-body);
  font-size: 16px;
  padding: var(--space-2) var(--space-2);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  color: var(--text);
  text-align: right;
  -moz-appearance: textfield;
}
</style>
