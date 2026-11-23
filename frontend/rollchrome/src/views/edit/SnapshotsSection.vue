<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';

const {
  selectedMask,
  newSnapshotName,
  loading,
  sessionId,
  saveSnapshot,
  snapshotMessage,
  recipe,
  restoreSnapshot,
  deleteSnapshot,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="Snapshots"
    test-id="section-snapshots"
    v-show="!selectedMask"
    :default-open="false"
  >
    <div class="snapshots-panel" data-testid="snapshots-panel">
      <div class="snapshots-form">
        <input
          v-model="newSnapshotName"
          type="text"
          placeholder="Snapshot name"
          class="snapshots-input"
          data-testid="snapshot-name-input"
          :disabled="loading || !sessionId"
          @keydown.enter="saveSnapshot"
        />
        <button
          type="button"
          class="snapshots-save-btn"
          data-testid="snapshot-save-btn"
          :disabled="loading || !sessionId"
          @click="saveSnapshot"
        >
          Save snapshot
        </button>
      </div>
      <div
        v-if="snapshotMessage"
        class="snapshots-message"
        data-testid="snapshot-message"
      >
        {{ snapshotMessage }}
      </div>
      <div v-if="!recipe.snapshots || recipe.snapshots.length === 0" class="snapshots-empty" data-testid="snapshots-empty">
        No snapshots
      </div>
      <ul v-else class="snapshots-list" data-testid="snapshots-list">
        <li
          v-for="snap in recipe.snapshots"
          :key="snap.name"
          class="snapshot-item"
          :data-testid="`snapshot-item-${snap.name}`"
        >
          <span class="snapshot-item__name">{{ snap.name }}</span>
          <div class="snapshot-item__actions">
            <button
              type="button"
              class="snapshot-action-btn"
              :data-testid="`snapshot-restore-${snap.name}`"
              :disabled="loading || !sessionId"
              @click="restoreSnapshot(snap)"
            >
              Restore
            </button>
            <button
              type="button"
              class="snapshot-action-btn snapshot-action-btn--delete"
              :data-testid="`snapshot-delete-${snap.name}`"
              :disabled="loading || !sessionId"
              @click="deleteSnapshot(snap.name)"
            >
              Delete
            </button>
          </div>
        </li>
      </ul>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* --- Snapshots Section (RP-5) ------------------------------------------- */
.snapshots-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.snapshots-form {
  display: flex;
  gap: var(--space-2);
}

.snapshots-input {
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

.snapshots-input:focus {
  outline: none;
  border-color: var(--accent);
}

.snapshots-save-btn {
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
  transition: border-color var(--dur-fast) var(--ease);
}

.snapshots-save-btn:hover:not(:disabled) {
  border-color: var(--accent);
}

.snapshots-save-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.snapshots-message {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--accent);
}

.snapshots-empty {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  padding: var(--space-1) 0;
}

.snapshots-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.snapshot-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 40px;
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  gap: var(--space-2);
}

.snapshot-item__name {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.snapshot-item__actions {
  display: flex;
  gap: var(--space-1);
}

.snapshot-action-btn {
  min-height: 40px;
  min-width: 40px;
  padding: var(--space-1) var(--space-3);
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
  background: transparent;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.snapshot-action-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--text-muted);
}

.snapshot-action-btn--delete:hover:not(:disabled) {
  color: var(--accent);
  border-color: var(--accent);
}

.snapshot-action-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
