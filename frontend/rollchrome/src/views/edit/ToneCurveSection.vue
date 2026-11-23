<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import CurveEditor from '../../components/CurveEditor.vue';

const {
  selectedMask,
  resetToneCurve,
  recipe,
  loading,
  isReadOnly,
  onCurvesInput,
  onCurvesChange,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Tone Curve"
    test-id="section-tone-curve"
    v-show="!selectedMask"
    :default-open="false"
    @reset="resetToneCurve"
  >
    <CurveEditor
      :model-value="recipe.curves"
      test-id="tone-curve-editor"
      :disabled="loading || isReadOnly"
      @update:model-value="onCurvesInput"
      @change="onCurvesChange"
    />
  </CollapsibleSection>
</template>
