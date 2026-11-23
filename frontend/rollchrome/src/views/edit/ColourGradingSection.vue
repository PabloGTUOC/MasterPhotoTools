<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import ColorWheel from '../../components/ColorWheel.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetGrading,
  GRADING_WHEELS,
  selectedGradingWheel,
  isGradingWheelModified,
  selectGradingWheel,
  currentGradingWheelDef,
  loading,
  isReadOnly,
  resetCurrentGradingWheel,
  recipe,
  onGradingWheelInput,
  onGradingWheelChange,
  onGradingParamInput,
  onGradingParamChange,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Colour Grading"
    test-id="section-grading"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetGrading"
  >
    <div class="grading-panel" data-testid="grading-panel">
      <!-- Wheel selector tabs -->
      <div class="grading-tabs" role="tablist" aria-label="Grading Wheels">
        <button
          v-for="wheel in GRADING_WHEELS"
          :key="wheel.key"
          type="button"
          class="grading-tab"
          :class="{ active: selectedGradingWheel === wheel.key, modified: isGradingWheelModified(wheel.key) }"
          :aria-label="wheel.label"
          :aria-selected="selectedGradingWheel === wheel.key"
          role="tab"
          tabindex="0"
          :data-testid="`grading-tab-${wheel.key}`"
          @click="selectGradingWheel(wheel.key)"
          @keydown.enter="selectGradingWheel(wheel.key)"
          @keydown.space.prevent="selectGradingWheel(wheel.key)"
        >
          <span class="grading-tab-label">{{ wheel.label }}</span>
          <span
            v-if="isGradingWheelModified(wheel.key)"
            class="grading-tab-dot"
            :data-testid="`grading-dot-${wheel.key}`"
            aria-hidden="true"
          />
        </button>
      </div>

      <!-- Active Wheel View -->
      <div class="grading-wheel-container">
        <div class="grading-wheel-header">
          <span class="grading-wheel-title">{{ currentGradingWheelDef.label }} Wheel</span>
          <button
            type="button"
            class="grading-reset-wheel-btn"
            :disabled="!isGradingWheelModified(selectedGradingWheel) || loading || isReadOnly"
            data-testid="grading-reset-wheel-btn"
            @click="resetCurrentGradingWheel"
          >
            Reset {{ currentGradingWheelDef.label }}
          </button>
        </div>

        <ColorWheel
          :model-value="recipe.grading?.[selectedGradingWheel]"
          :disabled="loading || isReadOnly"
          :test-id="`grading-wheel-${selectedGradingWheel}`"
          @update:model-value="onGradingWheelInput"
          @change="onGradingWheelChange"
        />
      </div>

      <!-- Tonal Range Sliders: Blending and Balance -->
      <div class="grading-range-controls">
        <AdjustmentSlider
          label="Blending"
          :model-value="recipe.grading?.blending ?? 50"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :disabled="loading || isReadOnly"
          test-id="grading-slider-blending"
          @update:model-value="(v) => onGradingParamInput('blending', v)"
          @change="(v) => onGradingParamChange('blending', v)"
        />

        <AdjustmentSlider
          label="Balance"
          :model-value="recipe.grading?.balance ?? 0"
          :min="-100"
          :max="100"
          :step="1"
          :disabled="loading || isReadOnly"
          test-id="grading-slider-balance"
          @update:model-value="(v) => onGradingParamInput('balance', v)"
          @change="(v) => onGradingParamChange('balance', v)"
        />
      </div>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
.grading-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.grading-tabs {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-1);
}

.grading-tab {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  min-height: 40px;
  height: 40px;
  padding: 0 var(--space-2);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.grading-tab:hover {
  color: var(--text-heading);
  border-color: var(--border-strong);
}

.grading-tab.active {
  color: var(--text-heading);
  border: var(--border-active);
  font-weight: 600;
}

.grading-tab-label {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.grading-tab-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-none);
  background-color: var(--text);
  border: 1px solid var(--bg);
  flex-shrink: 0;
}

.grading-wheel-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) 0;
  width: 100%;
}

.grading-wheel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  min-height: 40px;
  height: 40px;
}

.grading-wheel-title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.grading-reset-wheel-btn {
  font-family: var(--font-label);
  font-size: 12px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: var(--space-1) var(--space-2);
  min-height: 40px;
  height: 40px;
  border-radius: var(--radius-none);
  transition: color var(--dur-fast) var(--ease);
}

.grading-reset-wheel-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.grading-reset-wheel-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.grading-range-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-2);
  border-top: 1px solid var(--border);
  width: 100%;
}
</style>
