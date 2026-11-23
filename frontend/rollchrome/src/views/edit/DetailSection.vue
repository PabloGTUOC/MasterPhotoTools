<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetDetail,
  zoom,
  recipe,
  loading,
  isReadOnly,
  onDetailSliderInput,
  onDetailSliderChange,
  toggleHotPixels,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <!-- Detail (RP-7) -->
  <CollapsibleSection
    title="Detail"
    test-id="section-detail"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetDetail"
  >
    <div class="detail-panel" data-testid="detail-panel">
      <div
        v-if="zoom === 'fit'"
        class="detail-hint"
        data-testid="detail-hint"
      >
        Judge detail at 100% (Z)
      </div>

      <div class="detail-group">
        <div class="detail-group-title">Sharpening</div>
        <AdjustmentSlider
          label="Amount"
          :model-value="recipe.detail?.sharpen_amount ?? 0"
          :min="0"
          :max="150"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="detail-sharpen-amount"
          @update:model-value="(v) => onDetailSliderInput('sharpen_amount', v)"
          @change="(v) => onDetailSliderChange('sharpen_amount', v)"
        />
        <AdjustmentSlider
          label="Radius"
          :model-value="recipe.detail?.sharpen_radius ?? 1.0"
          :min="0.5"
          :max="3.0"
          :step="0.1"
          :default-value="1.0"
          :disabled="loading || isReadOnly"
          test-id="detail-sharpen-radius"
          @update:model-value="(v) => onDetailSliderInput('sharpen_radius', v)"
          @change="(v) => onDetailSliderChange('sharpen_radius', v)"
        />
        <AdjustmentSlider
          label="Masking"
          :model-value="recipe.detail?.sharpen_masking ?? 0"
          :min="0"
          :max="100"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="detail-sharpen-masking"
          @update:model-value="(v) => onDetailSliderInput('sharpen_masking', v)"
          @change="(v) => onDetailSliderChange('sharpen_masking', v)"
        />
      </div>

      <div class="detail-group">
        <div class="detail-group-title">Noise reduction</div>
        <AdjustmentSlider
          label="Luminance"
          :model-value="recipe.detail?.noise_luminance ?? 0"
          :min="0"
          :max="100"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="detail-noise-luminance"
          @update:model-value="(v) => onDetailSliderInput('noise_luminance', v)"
          @change="(v) => onDetailSliderChange('noise_luminance', v)"
        />
        <AdjustmentSlider
          label="Colour"
          :model-value="recipe.detail?.noise_colour ?? 0"
          :min="0"
          :max="100"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="detail-noise-colour"
          @update:model-value="(v) => onDetailSliderInput('noise_colour', v)"
          @change="(v) => onDetailSliderChange('noise_colour', v)"
        />
      </div>

      <div class="detail-group detail-group--toggle">
        <button
          type="button"
          class="ghost hot-pixels-toggle-btn"
          data-testid="detail-hot-pixels-toggle"
          :class="{ active: recipe.detail?.hot_pixels ?? false }"
          :disabled="loading || isReadOnly"
          title="Remove hot pixels"
          @click="toggleHotPixels"
        >
          Remove hot pixels
        </button>
      </div>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* Detail Section (RP-7) */
.detail-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}

.detail-hint {
  font-family: var(--font-label);
  font-size: 11px;
  font-style: italic;
  color: var(--text-muted);
  padding: var(--space-1) var(--space-2);
  line-height: 1.4;
}

.detail-group {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.detail-group-title {
  font-family: var(--font-label);
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  padding: 0 var(--space-2);
}

.detail-group--toggle {
  margin-top: var(--space-1);
}

.hot-pixels-toggle-btn {
  width: 100%;
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.hot-pixels-toggle-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.hot-pixels-toggle-btn.active {
  background: var(--accent);
  color: #000;
  border-color: var(--accent);
}
</style>
