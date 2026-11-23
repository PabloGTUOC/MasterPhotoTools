<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';
import JobProgress from '@ui/components/JobProgress.vue';

const {
  resetMasks,
  sessionId,
  isReadOnly,
  masks,
  MASK_LIMIT,
  addMask,
  addLuminanceMask,
  startColourRangeEyedropper,
  autoBusy,
  addAutoMask,
  addObjectMask,
  addBackgroundMask,
  eyedropperTarget,
  exitEyedropper,
  autoDownload,
  AUTO_LABEL,
  startModelDownload,
  cancelModelDownload,
  onModelDownloaded,
  stopAutoMask,
  selectedMaskId,
  toggleMask,
  deleteMask,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Masks"
    test-id="section-masks"
    :default-open="false"
    @reset="resetMasks"
  >
    <div class="effects-panel" data-testid="masks-panel">
      <div class="mask-add">
        <button
          type="button"
          class="secondary"
          data-testid="add-linear-mask"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
          @click="addMask('linear')"
        >
          + Linear
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-radial-mask"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
          @click="addMask('radial')"
        >
          + Radial
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-brush-mask"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
          @click="addMask('brush')"
        >
          + Brush
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-luminance-mask"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
          @click="addLuminanceMask"
        >
          + Luminance range
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-colour-mask"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
          @click="startColourRangeEyedropper"
        >
          + Colour range
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-subject-mask"
          title="Find the main subject (a model, downloaded once)"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
          @click="addAutoMask('subject')"
        >
          + Subject
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-people-mask"
          title="Find people (a model, downloaded once)"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
          @click="addAutoMask('people')"
        >
          + People
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-object-mask"
          title="Select an object by clicking on it"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
          @click="addObjectMask"
        >
          + Object
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-sky-mask"
          title="Find the sky (a model, downloaded once)"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
          @click="addAutoMask('sky')"
        >
          + Sky
        </button>
        <button
          type="button"
          class="secondary"
          data-testid="add-background-mask"
          title="Find the background (subject model inverted)"
          :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
          @click="addBackgroundMask"
        >
          + Background
        </button>
      </div>

      <div v-if="eyedropperTarget === 'object'" class="effects-group" data-testid="object-hint-group">
        <p class="mask-note" data-testid="object-hint">
          Click the object. Click again to add, Alt-click to take away.
        </p>
        <div class="brush-tool__row">
          <button
            type="button"
            class="primary"
            data-testid="object-done"
            @click="exitEyedropper"
          >
            Done
          </button>
        </div>
      </div>

      <div v-if="autoDownload" class="effects-group" data-testid="auto-download">
        <p class="mask-note">
          Finding the {{ AUTO_LABEL[autoDownload.target].toLowerCase() }} needs a model,
          downloaded once and kept for every photograph: {{ autoDownload.name }}.
        </p>
        <div v-if="!autoDownload.jobId" class="brush-tool__row">
          <button
            type="button"
            class="primary"
            data-testid="auto-download-start"
            @click="startModelDownload"
          >
            Download
          </button>
          <button
            type="button"
            class="ghost"
            data-testid="auto-download-cancel"
            @click="autoDownload = null"
          >
            Not now
          </button>
        </div>
        <JobProgress
          v-else
          :job-id="autoDownload.jobId"
          :can-cancel="true"
          @cancel="cancelModelDownload"
          @done="onModelDownloaded"
        />
      </div>

      <div v-if="autoBusy" class="auto-busy" role="status" data-testid="auto-busy">
        <span>
          Finding the {{ AUTO_LABEL[autoBusy].toLowerCase() }}<span class="edit-screen__cursor">_</span>
          a few seconds
        </span>
        <button type="button" class="ghost" data-testid="auto-stop" @click="stopAutoMask">
          Stop
        </button>
      </div>
      <p v-if="masks.length >= MASK_LIMIT" class="mask-note" data-testid="mask-limit">
        Up to {{ MASK_LIMIT }} masks per photograph: each one costs every preview frame.
      </p>
      <p v-else-if="!masks.length" class="mask-note">
        A mask applies its own adjustments to part of the photograph. Masks stay with
        this photograph: presets, paste and Batch Grade never carry them.
      </p>

      <ul v-if="masks.length" class="mask-list" data-testid="mask-list">
        <li
          v-for="m in masks"
          :key="m.id"
          class="mask-row"
          :class="{ 'is-selected': m.id === selectedMaskId }"
          :data-testid="`mask-row-${m.id}`"
        >
          <button
            type="button"
            class="ghost mask-row__name"
            :aria-pressed="m.id === selectedMaskId"
            :data-testid="`mask-select-${m.id}`"
            @click="selectedMaskId = m.id === selectedMaskId ? null : m.id"
          >
            {{ m.name }}
          </button>
          <button
            type="button"
            class="ghost mask-row__toggle"
            :class="{ active: m.enabled }"
            :aria-pressed="m.enabled"
            :title="m.enabled ? 'Turn this mask off' : 'Turn this mask on'"
            :data-testid="`mask-enable-${m.id}`"
            @click="toggleMask(m.id, 'enabled')"
          >
            {{ m.enabled ? 'On' : 'Off' }}
          </button>
          <button
            type="button"
            class="ghost mask-row__toggle"
            :class="{ active: m.invert }"
            :aria-pressed="m.invert"
            title="Apply outside the shape instead"
            :data-testid="`mask-invert-${m.id}`"
            @click="toggleMask(m.id, 'invert')"
          >
            Invert
          </button>
          <button
            type="button"
            class="ghost mask-row__delete"
            :aria-label="`Delete mask ${m.name}`"
            :data-testid="`mask-delete-${m.id}`"
            @click="deleteMask(m.id)"
          >
            ×
          </button>
        </li>
      </ul>

    </div>
  </CollapsibleSection>
</template>

<style scoped src="./shared.css"></style>

<style scoped>
.mask-add {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: var(--space-2);
}

.mask-add button {
  min-height: 40px;
  padding: 0 var(--space-1);
  font-size: 12px;
  letter-spacing: 0.04em;
  white-space: nowrap;
}

.mask-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: var(--space-1);
}

.mask-row {
  display: grid;
  grid-template-columns: 1fr auto auto 40px;
  gap: var(--space-1);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.mask-row.is-selected {
  border: var(--border-active);
}

.mask-row button {
  min-height: 40px;
  padding: 0 var(--space-2);
  font-size: 12px;
}

.mask-row__name {
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mask-row__toggle.active {
  color: var(--accent);
}
</style>
