<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import AdjustmentSlider from '../../components/AdjustmentSlider.vue';

const {
  selectedMask,
  resetGeometry,
  isCropMode,
  toggleCropMode,
  selectedAspect,
  onAspectChange,
  straightenAngle,
  onStraightenInput,
  onStraightenChange,
  rotateCCW,
  rotateCW,
  isFlipH,
  toggleFlipH,
  isFlipV,
  toggleFlipV,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Geometry"
    test-id="section-geometry"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetGeometry"
  >
    <div class="geometry-panel" data-testid="geometry-panel">
      <!-- Crop mode toggle button -->
      <div class="geometry-toolbar">
        <button
          type="button"
          class="secondary geometry-btn geometry-crop-btn"
          :class="{ active: isCropMode }"
          data-testid="crop-mode-toggle-btn"
          @click="toggleCropMode"
        >
          {{ isCropMode ? 'Done Cropping' : 'Crop' }}
        </button>
      </div>

      <!-- Aspect ratio preset dropdown -->
      <div class="geometry-row">
        <label class="geometry-label" for="geometry-aspect-select">Aspect</label>
        <select
          id="geometry-aspect-select"
          v-model="selectedAspect"
          class="geometry-select"
          data-testid="geometry-aspect-select"
          @change="onAspectChange"
        >
          <option value="original">Original</option>
          <option value="free">Free</option>
          <option value="1:1">1:1 (Square)</option>
          <option value="3:2">3:2 (35mm)</option>
          <option value="2:3">2:3 (Portrait)</option>
          <option value="4:3">4:3</option>
          <option value="3:4">3:4</option>
          <option value="16:9">16:9 (Widescreen)</option>
          <option value="9:16">9:16 (Vertical)</option>
          <option value="5:4">5:4</option>
          <option value="4:5">4:5</option>
        </select>
      </div>

      <!-- Straighten slider -->
      <AdjustmentSlider
        v-model="straightenAngle"
        label="Straighten"
        :min="-45.0"
        :max="45.0"
        :step="0.1"
        unit="°"
        test-id="geometry-straighten"
        @update:model-value="onStraightenInput"
        @change="onStraightenChange"
      />

      <!-- Transform buttons: Rotate CCW, Rotate CW, Flip H, Flip V -->
      <div class="geometry-actions">
        <button
          type="button"
          class="secondary geometry-btn"
          data-testid="rotate-ccw-btn"
          title="Rotate 90° CCW"
          @click="rotateCCW"
        >
          ↺ -90°
        </button>
        <button
          type="button"
          class="secondary geometry-btn"
          data-testid="rotate-cw-btn"
          title="Rotate 90° CW"
          @click="rotateCW"
        >
          ↻ +90°
        </button>
        <button
          type="button"
          class="secondary geometry-btn"
          :class="{ active: isFlipH }"
          data-testid="flip-h-btn"
          title="Flip Horizontal"
          @click="toggleFlipH"
        >
          ⇄ Flip H
        </button>
        <button
          type="button"
          class="secondary geometry-btn"
          :class="{ active: isFlipV }"
          data-testid="flip-v-btn"
          title="Flip Vertical"
          @click="toggleFlipV"
        >
          ⇅ Flip V
        </button>
      </div>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* --- Geometry Section (ED-13) ------------------------------------------- */

.geometry-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}

.geometry-toolbar {
  display: flex;
  gap: var(--space-2);
}

.geometry-crop-btn {
  flex: 1;
  min-height: 40px;
  font-size: 13px;
  font-weight: 600;
  border-radius: 2px;
}

.geometry-crop-btn.active {
  background: var(--accent);
  color: var(--void);
}

.geometry-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.geometry-label {
  font-size: 13px;
  color: var(--text-muted);
}

.geometry-select {
  flex: 1;
  min-height: 40px;
  background: var(--bg-panel);
  border: var(--border-hair);
  color: var(--text-heading);
  padding: 0 var(--space-2);
  border-radius: 2px;
  font-size: 13px;
  outline: none;
}

.geometry-select:focus {
  border-color: var(--focus-ring);
}

.geometry-actions {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2);
}

.geometry-btn {
  min-height: 40px;
  padding: var(--space-2);
  font-size: 13px;
  border-radius: 2px;
}

.geometry-btn.active {
  background: var(--accent);
  color: var(--void);
}
</style>
