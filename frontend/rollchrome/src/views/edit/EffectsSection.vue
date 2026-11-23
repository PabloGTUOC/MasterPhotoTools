<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetEffects,
  recipe,
  loading,
  isReadOnly,
  onVignetteSliderInput,
  onVignetteSliderChange,
  onGrainSliderInput,
  onGrainSliderChange,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Effects"
    test-id="section-effects"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetEffects"
  >
    <div class="effects-panel" data-testid="effects-panel">
      <div class="effects-group">
        <div class="effects-group-title">Vignette</div>
        <AdjustmentSlider
          label="Amount"
          :model-value="recipe.vignette?.amount ?? 0"
          :min="-100"
          :max="100"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="vignette-amount"
          @update:model-value="(v) => onVignetteSliderInput('amount', v)"
          @change="(v) => onVignetteSliderChange('amount', v)"
        />
        <AdjustmentSlider
          label="Midpoint"
          :model-value="recipe.vignette?.midpoint ?? 50"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="50"
          :disabled="loading || isReadOnly"
          test-id="vignette-midpoint"
          @update:model-value="(v) => onVignetteSliderInput('midpoint', v)"
          @change="(v) => onVignetteSliderChange('midpoint', v)"
        />
        <AdjustmentSlider
          label="Roundness"
          :model-value="recipe.vignette?.roundness ?? 0"
          :min="-100"
          :max="100"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="vignette-roundness"
          @update:model-value="(v) => onVignetteSliderInput('roundness', v)"
          @change="(v) => onVignetteSliderChange('roundness', v)"
        />
        <AdjustmentSlider
          label="Feather"
          :model-value="recipe.vignette?.feather ?? 50"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="50"
          :disabled="loading || isReadOnly"
          test-id="vignette-feather"
          @update:model-value="(v) => onVignetteSliderInput('feather', v)"
          @change="(v) => onVignetteSliderChange('feather', v)"
        />
      </div>

      <div class="effects-group">
        <div class="effects-group-title">Film Grain</div>
        <AdjustmentSlider
          label="Amount"
          :model-value="recipe.grain?.amount ?? 0"
          :min="0"
          :max="100"
          :step="1"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="grain-amount"
          @update:model-value="(v) => onGrainSliderInput('amount', v)"
          @change="(v) => onGrainSliderChange('amount', v)"
        />
        <AdjustmentSlider
          label="Size"
          :model-value="recipe.grain?.size ?? 25"
          :min="0"
          :max="100"
          :step="1"
          :default-value="25"
          :disabled="loading || isReadOnly"
          test-id="grain-size"
          @update:model-value="(v) => onGrainSliderInput('size', v)"
          @change="(v) => onGrainSliderChange('size', v)"
        />
        <AdjustmentSlider
          label="Roughness"
          :model-value="recipe.grain?.roughness ?? 50"
          :min="0"
          :max="100"
          :step="1"
          :default-value="50"
          :disabled="loading || isReadOnly"
          test-id="grain-roughness"
          @update:model-value="(v) => onGrainSliderInput('roughness', v)"
          @change="(v) => onGrainSliderChange('roughness', v)"
        />
      </div>
    </div>
  </CollapsibleSection>
</template>

<style scoped src="./shared.css"></style>
