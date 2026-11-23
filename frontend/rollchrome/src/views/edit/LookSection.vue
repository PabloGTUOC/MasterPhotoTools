<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import LutPicker from '../../components/LutPicker.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetLook,
  recipe,
  lutList,
  roots,
  rootsError,
  listRoots,
  setLut,
  setLutIntensity,
  onLutIntensityChange,
  handleImportLut,
  setToneMapper,
  loading,
  isReadOnly,
  onLooksSliderInput,
  onLooksSliderChange,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Look"
    test-id="section-look"
    v-show="!selectedMask"
    :default-open="true"
    @reset="resetLook"
  >
    <div class="look-panel" data-testid="look-panel">
      <LutPicker
        :model-value="recipe.lut ?? null"
        :intensity="recipe.lut_intensity ?? 1.0"
        :luts="lutList.luts"
        :errors="lutList.errors"
        :roots="roots"
        :roots-error="rootsError"
        :list="listRoots"
        @update:model-value="setLut"
        @update:intensity="setLutIntensity"
        @change="onLutIntensityChange"
        @import="handleImportLut"
      />

      <!-- Tone Mapper -->
      <div class="look-group">
        <div class="look-group-title">Tone Mapper</div>
        <div class="tone-mapper-toggle" data-testid="tone-mapper-toggle">
          <button
            type="button"
            class="secondary tone-btn"
            :class="{ active: !recipe.looks?.tone_mapper || recipe.looks?.tone_mapper === 'none' }"
            data-testid="tone-mapper-plain"
            :disabled="loading || isReadOnly"
            @click="setToneMapper('none')"
          >
            Plain
          </button>
          <button
            type="button"
            class="secondary tone-btn"
            :class="{ active: recipe.looks?.tone_mapper === 'aces' }"
            data-testid="tone-mapper-filmic"
            :disabled="loading || isReadOnly"
            @click="setToneMapper('aces')"
          >
            Filmic
          </button>
        </div>
        <div
          v-if="recipe.looks?.tone_mapper === 'aces'"
          class="tone-mapper-banner"
          data-testid="tone-mapper-banner"
        >
          Filmic tone curve on: highlights roll off and mid-tones shift. Turn it off to return to the plain curve.
        </div>
      </div>

      <!-- Glow -->
      <div class="look-group">
        <div class="look-group-title">Glow</div>
        <AdjustmentSlider
          label="Amount"
          :model-value="recipe.looks?.glow_amount ?? 0"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="glow-amount"
          @update:model-value="(v) => onLooksSliderInput('glow_amount', v)"
          @change="(v) => onLooksSliderChange('glow_amount', v)"
        />
        <AdjustmentSlider
          label="Threshold"
          :model-value="recipe.looks?.glow_threshold ?? 70"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="70"
          :disabled="loading || isReadOnly"
          test-id="glow-threshold"
          @update:model-value="(v) => onLooksSliderInput('glow_threshold', v)"
          @change="(v) => onLooksSliderChange('glow_threshold', v)"
        />
        <AdjustmentSlider
          label="Radius"
          :model-value="recipe.looks?.glow_radius ?? 30"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="30"
          :disabled="loading || isReadOnly"
          test-id="glow-radius"
          @update:model-value="(v) => onLooksSliderInput('glow_radius', v)"
          @change="(v) => onLooksSliderChange('glow_radius', v)"
        />
      </div>

      <!-- Halation -->
      <div class="look-group">
        <div class="look-group-title">Halation</div>
        <AdjustmentSlider
          label="Amount"
          :model-value="recipe.looks?.halation_amount ?? 0"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="0"
          :disabled="loading || isReadOnly"
          test-id="halation-amount"
          @update:model-value="(v) => onLooksSliderInput('halation_amount', v)"
          @change="(v) => onLooksSliderChange('halation_amount', v)"
        />
        <AdjustmentSlider
          label="Threshold"
          :model-value="recipe.looks?.halation_threshold ?? 80"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="80"
          :disabled="loading || isReadOnly"
          test-id="halation-threshold"
          @update:model-value="(v) => onLooksSliderInput('halation_threshold', v)"
          @change="(v) => onLooksSliderChange('halation_threshold', v)"
        />
        <AdjustmentSlider
          label="Radius"
          :model-value="recipe.looks?.halation_radius ?? 20"
          :min="0"
          :max="100"
          :step="1"
          unit="%"
          :default-value="20"
          :disabled="loading || isReadOnly"
          test-id="halation-radius"
          @update:model-value="(v) => onLooksSliderInput('halation_radius', v)"
          @change="(v) => onLooksSliderChange('halation_radius', v)"
        />
      </div>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* --- Look Section (ED-16) ----------------------------------------------- */

.look-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.look-group {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.look-group-title {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.tone-mapper-toggle {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2);
}

.tone-btn {
  min-height: 40px;
  padding: var(--space-2);
  font-size: 13px;
  border-radius: var(--radius-pill);
}

.tone-btn.active {
  background: var(--accent);
  color: var(--void);
}

.tone-mapper-banner {
  padding: var(--space-2) var(--space-3);
  background: var(--bg-elevated);
  border-left: 2px solid var(--accent-warm);
  color: var(--text);
  font-family: var(--font-body);
  font-size: 11px;
  line-height: 1.4;
}
</style>
