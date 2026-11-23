<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';

const {
  selectedMask,
  currentVersionName,
  editingCurrentVersion,
  renameCurrentInput,
  submitRenameCurrent,
  loading,
  sessionId,
  startRenameCurrent,
  newVersionInput,
  createVersion,
  versionMessage,
  recipe,
  editingVersionName,
  renameOtherInput,
  submitRenameOther,
  switchVersion,
  startRenameOther,
  deleteVersion,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Versions"
    test-id="section-versions"
    v-show="!selectedMask"
    :default-open="false"
  >
    <div class="versions-panel" data-testid="versions-panel">
      <div class="current-version-card" data-testid="current-version-card">
        <div class="current-version-header">
          <span class="current-version-badge">Current</span>
          <span class="current-version-name" data-testid="current-version-name">{{ currentVersionName }}</span>
        </div>
        <div v-if="editingCurrentVersion" class="version-rename-form">
          <input
            v-model="renameCurrentInput"
            type="text"
            class="version-input"
            data-testid="rename-current-input"
            placeholder="New name"
            @keydown.enter="submitRenameCurrent"
            @keydown.esc="editingCurrentVersion = false"
          />
          <button
            type="button"
            class="version-btn version-btn--save"
            data-testid="rename-current-save-btn"
            @click="submitRenameCurrent"
          >
            Save
          </button>
          <button
            type="button"
            class="version-btn"
            data-testid="rename-current-cancel-btn"
            @click="editingCurrentVersion = false"
          >
            Cancel
          </button>
        </div>
        <div v-else class="current-version-actions">
          <button
            type="button"
            class="version-btn"
            data-testid="rename-current-btn"
            :disabled="loading || !sessionId"
            @click="startRenameCurrent"
          >
            Rename
          </button>
        </div>
      </div>

      <div class="versions-new-form">
        <input
          v-model="newVersionInput"
          type="text"
          placeholder="Version name"
          class="version-input"
          data-testid="version-name-input"
          :disabled="loading || !sessionId"
          @keydown.enter="createVersion"
        />
        <button
          type="button"
          class="version-btn version-btn--primary"
          data-testid="version-create-btn"
          :disabled="loading || !sessionId"
          @click="createVersion"
        >
          New version
        </button>
      </div>

      <div
        v-if="versionMessage"
        class="version-message"
        data-testid="version-message"
      >
        {{ versionMessage }}
      </div>

      <div v-if="!recipe.versions || recipe.versions.length === 0" class="versions-empty" data-testid="versions-empty">
        No other versions
      </div>
      <ul v-else class="versions-list" data-testid="versions-list">
        <li
          v-for="v in recipe.versions"
          :key="v.name"
          class="version-item"
          :data-testid="`version-item-${v.name}`"
        >
          <div v-if="editingVersionName === v.name" class="version-rename-form">
            <input
              v-model="renameOtherInput"
              type="text"
              class="version-input"
              :data-testid="`rename-input-${v.name}`"
              placeholder="New name"
              @keydown.enter="submitRenameOther(v.name)"
              @keydown.esc="editingVersionName = null"
            />
            <button
              type="button"
              class="version-btn version-btn--save"
              :data-testid="`rename-save-${v.name}`"
              @click="submitRenameOther(v.name)"
            >
              Save
            </button>
            <button
              type="button"
              class="version-btn"
              :data-testid="`rename-cancel-${v.name}`"
              @click="editingVersionName = null"
            >
              Cancel
            </button>
          </div>
          <div v-else class="version-item__row">
            <span class="version-item__name" :data-testid="`version-name-${v.name}`">{{ v.name }}</span>
            <div class="version-item__actions">
              <button
                type="button"
                class="version-btn"
                :data-testid="`version-switch-${v.name}`"
                :disabled="loading || !sessionId"
                @click="switchVersion(v.name)"
              >
                Switch
              </button>
              <button
                type="button"
                class="version-btn"
                :data-testid="`version-rename-${v.name}`"
                :disabled="loading || !sessionId"
                @click="startRenameOther(v.name)"
              >
                Rename
              </button>
              <button
                type="button"
                class="version-btn version-btn--delete"
                :data-testid="`version-delete-${v.name}`"
                :disabled="loading || !sessionId"
                @click="deleteVersion(v.name)"
              >
                Delete
              </button>
            </div>
          </div>
        </li>
      </ul>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* --- Versions Section (RP-6) -------------------------------------------- */
.versions-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.current-version-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2);
  border: 1px solid var(--border);
  background: var(--bg-panel);
}

.current-version-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 24px;
}

.current-version-badge {
  font-family: var(--font-label);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--accent);
  font-weight: 600;
}

.current-version-name {
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  color: var(--text-heading);
}

.current-version-actions {
  display: flex;
  gap: var(--space-2);
}

.versions-new-form {
  display: flex;
  gap: var(--space-2);
}

.version-input {
  flex: 1;
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 14px;
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  box-sizing: border-box;
}

.version-input:focus {
  outline: none;
  border-color: var(--accent);
}

.version-btn {
  min-height: 40px;
  min-width: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-heading);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  white-space: nowrap;
  box-sizing: border-box;
  transition: border-color var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
}

.version-btn:hover:not(:disabled) {
  border-color: var(--accent);
}

.version-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.version-btn--primary {
  background: var(--bg-panel);
  border-color: var(--border);
}

.version-btn--save {
  background: var(--bg-panel);
}

.version-btn--delete:hover:not(:disabled) {
  color: var(--accent);
  border-color: var(--accent);
}

.version-message {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--accent);
}

.versions-empty {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  padding: var(--space-1) 0;
}

.versions-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.version-item {
  display: flex;
  flex-direction: column;
  min-height: 40px;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  box-sizing: border-box;
}

.version-item__row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 40px;
  padding: var(--space-1) var(--space-2);
  gap: var(--space-2);
}

.version-item__name {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.version-item__actions {
  display: flex;
  gap: var(--space-1);
}

.version-rename-form {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-1);
}
</style>
