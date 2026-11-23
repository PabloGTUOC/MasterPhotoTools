<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetHsl,
  isBwActive,
  toggleBlackAndWhite,
  isBwMixDefault,
  loading,
  isReadOnly,
  resetBwMix,
  BW_BANDS,
  getBwBand,
  onBwSliderInput,
  onBwSliderChange,
  HSL_BANDS,
  selectedHslBand,
  isBandModified,
  selectHslBand,
  currentBandDef,
  resetCurrentHslBand,
  recipe,
  onHslSliderInput,
  onHslSliderChange,
  eyedropperTarget,
  togglePointColourEyedropper,
  selectedPointIndex,
  selectPoint,
  selectedPoint,
  onPointColourSliderInput,
  onPointColourSliderChange,
  deleteSelectedPoint,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Colour / HSL"
    test-id="section-hsl"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetHsl"
  >
    <div class="colour-mode-bar">
      <button
        type="button"
        class="ghost bw-toggle-btn"
        data-testid="bw-toggle-btn"
        :class="{ active: isBwActive }"
        title="Toggle Black & White (V)"
        @click="toggleBlackAndWhite"
      >
        Black & White
      </button>
    </div>

    <!-- B&W Mix Panel (RP-4) -->
    <div v-if="isBwActive" class="bw-mix-panel" data-testid="bw-mix-panel">
      <div class="bw-mix-header">
        <span class="bw-mix-title">B&W Mix</span>
        <button
          type="button"
          class="ghost bw-mix-reset-btn"
          data-testid="bw-mix-reset-btn"
          :disabled="isBwMixDefault || loading || isReadOnly"
          @click="resetBwMix"
        >
          Reset Mix
        </button>
      </div>

      <div class="bw-mix-sliders">
        <AdjustmentSlider
          v-for="band in BW_BANDS"
          :key="band.key"
          :label="band.label"
          :model-value="getBwBand(band.key)"
          :min="-100"
          :max="100"
          :step="1"
          :disabled="loading || isReadOnly"
          :test-id="`bw-slider-${band.key}`"
          @update:model-value="(v) => onBwSliderInput(band.key, v)"
          @change="(v) => onBwSliderChange(band.key, v)"
        />
      </div>
    </div>

    <!-- HSL Panel -->
    <div v-else class="hsl-panel" data-testid="hsl-panel">
      <div class="hsl-swatches" role="tablist" aria-label="HSL Colour Bands">
        <button
          v-for="band in HSL_BANDS"
          :key="band.key"
          type="button"
          class="hsl-swatch"
          :class="{ active: selectedHslBand === band.key, modified: isBandModified(band.key) }"
          :style="{ backgroundColor: `oklch(0.7 0.2 ${band.hueDeg}deg)` }"
          :title="band.label"
          :aria-label="band.label"
          :aria-selected="selectedHslBand === band.key"
          role="tab"
          tabindex="0"
          :data-testid="`hsl-swatch-${band.key}`"
          @click="selectHslBand(band.key)"
          @keydown.enter="selectHslBand(band.key)"
          @keydown.space.prevent="selectHslBand(band.key)"
        >
          <span
            v-if="isBandModified(band.key)"
            class="hsl-swatch-dot"
            :data-testid="`hsl-dot-${band.key}`"
            aria-hidden="true"
          />
        </button>
      </div>

      <div class="hsl-controls">
        <div class="hsl-band-header">
          <span class="hsl-band-title">{{ currentBandDef.label }}</span>
          <button
            type="button"
            class="hsl-reset-band-btn"
            :disabled="!isBandModified(selectedHslBand) || loading || isReadOnly"
            data-testid="hsl-reset-band-btn"
            @click="resetCurrentHslBand"
          >
            Reset {{ currentBandDef.label }}
          </button>
        </div>

        <AdjustmentSlider
          label="Hue"
          :model-value="recipe.hsl?.[selectedHslBand]?.hue ?? 0"
          :min="-180"
          :max="180"
          :step="1"
          unit="°"
          :disabled="loading || isReadOnly"
          test-id="hsl-slider-hue"
          @update:model-value="(v) => onHslSliderInput('hue', v)"
          @change="(v) => onHslSliderChange('hue', v)"
        />

        <AdjustmentSlider
          label="Saturation"
          :model-value="recipe.hsl?.[selectedHslBand]?.saturation ?? 0"
          :min="-100"
          :max="100"
          :step="1"
          unit="%"
          :disabled="loading || isReadOnly"
          test-id="hsl-slider-saturation"
          @update:model-value="(v) => onHslSliderInput('saturation', v)"
          @change="(v) => onHslSliderChange('saturation', v)"
        />

        <AdjustmentSlider
          label="Luminance"
          :model-value="recipe.hsl?.[selectedHslBand]?.luminance ?? 0"
          :min="-100"
          :max="100"
          :step="1"
          unit="%"
          :disabled="loading || isReadOnly"
          test-id="hsl-slider-luminance"
          @update:model-value="(v) => onHslSliderInput('luminance', v)"
          @change="(v) => onHslSliderChange('luminance', v)"
        />
      </div>
    </div>

    <!-- Point Colour Group (RP-10) -->
    <div v-if="!isBwActive" class="point-colour-panel" data-testid="point-colour-panel">
      <div class="point-colour-header">
        <span class="point-colour-title">Point colour</span>
        <button
          type="button"
          class="ghost point-colour-pick-btn"
          :class="{ active: eyedropperTarget === 'point-colour' }"
          :disabled="loading || isReadOnly || (recipe.point_colours?.length ?? 0) >= 8"
          data-testid="point-colour-pick-btn"
          :title="(recipe.point_colours?.length ?? 0) >= 8 ? 'Maximum 8 points reached' : 'Pick a colour'"
          @click="togglePointColourEyedropper"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M14 2a2 2 0 0 0-2.83 0L9.41 3.76l2.83 2.83L14 4.83A2 2 0 0 0 14 2z"/>
            <path d="M8 5.17L2.34 10.83a2 2 0 0 0-.54.97L1 15l3.2-.8a2 2 0 0 0 .97-.54L10.83 8"/>
          </svg>
          Pick a colour
        </button>
      </div>

      <div
        v-if="(recipe.point_colours?.length ?? 0) >= 8"
        class="point-colour-limit-msg"
        data-testid="point-colour-limit-msg"
      >
        Maximum 8 points reached
      </div>

      <!-- Swatches list -->
      <div
        v-if="(recipe.point_colours?.length ?? 0) > 0"
        class="point-colour-swatches"
        role="tablist"
        aria-label="Point Colours"
      >
        <button
          v-for="(pt, idx) in recipe.point_colours"
          :key="idx"
          type="button"
          class="point-colour-swatch"
          :class="{ active: selectedPointIndex === idx }"
          :style="{ backgroundColor: `oklch(${pt.lightness} ${pt.chroma} ${pt.hue}deg)` }"
          :title="`Point colour ${idx + 1}`"
          :aria-label="`Point colour ${idx + 1}`"
          :aria-selected="selectedPointIndex === idx"
          role="tab"
          tabindex="0"
          :data-testid="`point-colour-swatch-${idx}`"
          @click="selectPoint(idx)"
        />
      </div>

      <!-- Selected point controls -->
      <div v-if="selectedPoint" class="point-colour-controls" data-testid="point-colour-controls">
        <AdjustmentSlider
          label="Hue"
          :model-value="selectedPoint.hue_shift"
          :min="-100"
          :max="100"
          :step="1"
          unit="°"
          :disabled="loading || isReadOnly"
          test-id="point-colour-hue"
          @update:model-value="(v) => onPointColourSliderInput('hue_shift', v)"
          @change="(v) => onPointColourSliderChange('hue_shift', v)"
        />

        <AdjustmentSlider
          label="Saturation"
          :model-value="selectedPoint.saturation"
          :min="-100"
          :max="100"
          :step="1"
          unit="%"
          :disabled="loading || isReadOnly"
          test-id="point-colour-sat"
          @update:model-value="(v) => onPointColourSliderInput('saturation', v)"
          @change="(v) => onPointColourSliderChange('saturation', v)"
        />

        <AdjustmentSlider
          label="Luminance"
          :model-value="selectedPoint.luminance"
          :min="-100"
          :max="100"
          :step="1"
          unit="%"
          :disabled="loading || isReadOnly"
          test-id="point-colour-lum"
          @update:model-value="(v) => onPointColourSliderInput('luminance', v)"
          @change="(v) => onPointColourSliderChange('luminance', v)"
        />

        <AdjustmentSlider
          label="Range"
          :model-value="selectedPoint.range"
          :min="0"
          :max="100"
          :step="1"
          :disabled="loading || isReadOnly"
          test-id="point-colour-range"
          @update:model-value="(v) => onPointColourSliderInput('range', v)"
          @change="(v) => onPointColourSliderChange('range', v)"
        />

        <div class="point-colour-actions">
          <button
            type="button"
            class="ghost point-colour-delete-btn"
            data-testid="point-colour-delete-btn"
            :disabled="loading || isReadOnly"
            @click="deleteSelectedPoint"
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
.hsl-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.hsl-swatches {
  display: grid;
  grid-template-columns: repeat(8, 1fr);
  gap: var(--space-1);
}

.hsl-swatch {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 40px;
  min-height: 40px;
  height: 40px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  transition: transform var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.hsl-swatch:hover {
  transform: translateY(-1px);
  border-color: var(--border-strong);
}

.hsl-swatch.active {
  border: var(--border-active);
}

.hsl-swatch-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-none);
  background-color: var(--text);
  border: 1px solid var(--bg);
}

.hsl-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-1);
}

.hsl-band-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 32px;
}

.hsl-band-title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.hsl-reset-band-btn {
  font-family: var(--font-label);
  font-size: 12px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: var(--space-1) var(--space-2);
  min-height: 32px;
  border-radius: var(--radius-none);
  transition: color var(--dur-fast) var(--ease);
}

.hsl-reset-band-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.hsl-reset-band-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.point-colour-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: var(--border-hair);
}

.point-colour-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 40px;
}

.point-colour-title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.point-colour-pick-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  cursor: pointer;
}

.point-colour-pick-btn.active {
  color: var(--accent);
  border-color: var(--accent);
}

.point-colour-pick-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.point-colour-limit-msg {
  font-family: var(--font-body);
  font-size: 12px;
  color: var(--text-muted);
  font-style: italic;
  padding: var(--space-1) 0;
}

.point-colour-swatches {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding: var(--space-1) 0;
}

.point-colour-swatch {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  min-width: 40px;
  min-height: 40px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  transition: transform var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.point-colour-swatch:hover {
  transform: translateY(-1px);
  border-color: var(--border-strong);
}

.point-colour-swatch.active {
  border: var(--border-active);
  box-shadow: 0 0 0 1px var(--accent);
}

.point-colour-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-1);
}

.point-colour-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: var(--space-1);
}

.point-colour-delete-btn {
  min-height: 40px;
  min-width: 80px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  cursor: pointer;
}

.point-colour-delete-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.point-colour-delete-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* Colour mode bar and Black & White mix (RP-4) */
.colour-mode-bar {
  display: flex;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.bw-toggle-btn {
  flex: 1;
  min-height: 40px;
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
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.bw-toggle-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.bw-toggle-btn.active {
  background: var(--accent);
  color: #000;
  border-color: var(--accent);
}

.bw-mix-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.bw-mix-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: var(--space-1);
  border-bottom: 1px solid var(--border);
}

.bw-mix-title {
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-heading);
}

.bw-mix-reset-btn {
  font-family: var(--font-label);
  font-size: 12px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: var(--space-1) var(--space-2);
  min-height: 40px;
  min-width: 40px;
  border-radius: var(--radius-none);
  transition: color var(--dur-fast) var(--ease);
}

.bw-mix-reset-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.bw-mix-reset-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.bw-mix-sliders {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
</style>
