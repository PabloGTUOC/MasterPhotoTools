<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetPresence,
  recipe,
  loading,
  isReadOnly,
  onPresenceSliderInput,
  onPresenceSliderChange,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Presence"
    test-id="section-presence"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetPresence"
  >
    <div class="presence-panel" data-testid="presence-panel">
      <AdjustmentSlider
        label="Clarity"
        :model-value="recipe.detail?.clarity ?? 0"
        :min="-100"
        :max="100"
        :step="1"
        :default-value="0"
        :disabled="loading || isReadOnly"
        test-id="presence-clarity"
        @update:model-value="(v) => onPresenceSliderInput('clarity', v)"
        @change="(v) => onPresenceSliderChange('clarity', v)"
      />
      <AdjustmentSlider
        label="Texture"
        :model-value="recipe.detail?.texture ?? 0"
        :min="-100"
        :max="100"
        :step="1"
        :default-value="0"
        :disabled="loading || isReadOnly"
        test-id="presence-texture"
        @update:model-value="(v) => onPresenceSliderInput('texture', v)"
        @change="(v) => onPresenceSliderChange('texture', v)"
      />
      <AdjustmentSlider
        label="Dehaze"
        :model-value="recipe.detail?.dehaze ?? 0"
        :min="-100"
        :max="100"
        :step="1"
        :default-value="0"
        :disabled="loading || isReadOnly"
        test-id="presence-dehaze"
        @update:model-value="(v) => onPresenceSliderInput('dehaze', v)"
        @change="(v) => onPresenceSliderChange('dehaze', v)"
      />
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* Presence Section (RP-8) */
.presence-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) 0;
}
</style>
