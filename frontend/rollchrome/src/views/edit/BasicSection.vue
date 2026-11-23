<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetBasic,
  sessionId,
  loading,
  triggerAutoTone,
  recipe,
  onRecipeValueInput,
  onRecipeValueChange,
  eyedropperTarget,
  isCropMode,
  isBefore,
  compareMode,
  toggleEyedropper,
  isBwActive,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Basic"
    test-id="section-basic"
    v-show="!selectedMask"
    :default-open="true"
    @reset="resetBasic"
  >
    <template #actions>
      <button
        type="button"
        class="ghost auto-tone-btn"
        data-testid="auto-tone-btn"
        :disabled="!sessionId || loading"
        title="Auto Tone: Exposure, Whites, Blacks"
        @click.stop="triggerAutoTone"
      >
        Auto
      </button>
    </template>
    <AdjustmentSlider
      v-model="recipe.exposure"
      label="Exposure"
      :min="-5.0"
      :max="5.0"
      :step="0.1"
      unit="EV"
      test-id="exposure"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.contrast"
      label="Contrast"
      :min="-100"
      :max="100"
      :step="1"
      test-id="contrast"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.highlights"
      label="Highlights"
      :min="-100"
      :max="100"
      :step="1"
      test-id="highlights"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.shadows"
      label="Shadows"
      :min="-100"
      :max="100"
      :step="1"
      test-id="shadows"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.whites"
      label="Whites"
      :min="-100"
      :max="100"
      :step="1"
      test-id="whites"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.blacks"
      label="Blacks"
      :min="-100"
      :max="100"
      :step="1"
      test-id="blacks"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.brightness"
      label="Brightness"
      :min="-100"
      :max="100"
      :step="1"
      test-id="brightness"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <div class="wb-slider-row">
      <AdjustmentSlider
        v-model="recipe.temperature"
        label="Temperature"
        :min="-100"
        :max="100"
        :step="1"
        test-id="temperature"
        @update:model-value="onRecipeValueInput"
        @change="onRecipeValueChange"
      />
      <button
        type="button"
        class="ghost eyedropper-btn"
        :class="{ active: eyedropperTarget === 'white-balance' }"
        :disabled="isCropMode || isBefore || compareMode !== 'off' || !sessionId"
        title="White Balance Eyedropper (W)"
        data-testid="eyedropper-btn"
        @click="toggleEyedropper"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M14 2a2 2 0 0 0-2.83 0L9.41 3.76l2.83 2.83L14 4.83A2 2 0 0 0 14 2z"/>
          <path d="M8 5.17L2.34 10.83a2 2 0 0 0-.54.97L1 15l3.2-.8a2 2 0 0 0 .97-.54L10.83 8"/>
        </svg>
      </button>
    </div>

    <AdjustmentSlider
      v-model="recipe.tint"
      label="Tint"
      :min="-100"
      :max="100"
      :step="1"
      test-id="tint"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.saturation"
      label="Saturation"
      :min="-100"
      :max="100"
      :step="1"
      :disabled="isBwActive"
      test-id="saturation"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.vibrance"
      label="Vibrance"
      :min="-100"
      :max="100"
      :step="1"
      :disabled="isBwActive"
      test-id="vibrance"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />

    <AdjustmentSlider
      v-model="recipe.hue"
      label="Hue"
      :min="-180"
      :max="180"
      :step="1"
      unit="°"
      test-id="hue"
      @update:model-value="onRecipeValueInput"
      @change="onRecipeValueChange"
    />
  </CollapsibleSection>
</template>

<style scoped>
/* Eyedropper and WB row (RP-4) */
.wb-slider-row {
  display: flex;
  align-items: flex-end;
  gap: var(--space-2);
}

.wb-slider-row :deep(.adjustment-slider) {
  flex: 1;
}

.eyedropper-btn {
  min-height: 44px;
  min-width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  color: var(--text-muted);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.eyedropper-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.eyedropper-btn.active {
  background: var(--accent);
  color: #000;
  border-color: var(--accent);
}

.eyedropper-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* Auto Tone button (RP-4) */
.auto-tone-btn {
  min-height: 40px;
  min-width: 44px;
  padding: 0 var(--space-2);
  font-family: var(--font-label);
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.auto-tone-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.auto-tone-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>
