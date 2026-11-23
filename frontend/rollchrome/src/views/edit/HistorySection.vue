<script setup lang="ts">
import { inject } from 'vue';
import { EDIT_CONTEXT } from './context';
import CollapsibleSection from '../../components/CollapsibleSection.vue';

const {
  selectedMask,
  historyEntries,
  reversedHistory,
  historyCursor,
  jumpToHistory,
} = inject(EDIT_CONTEXT)!;
</script>

<template>
  <CollapsibleSection
    title="History"
    test-id="section-history"
    v-show="!selectedMask"
    :default-open="false"
  >
    <div class="history-panel" data-testid="history-panel">
      <div v-if="historyEntries.length === 0" class="history-empty" data-testid="history-empty">
        No history
      </div>
      <ul v-else class="history-list" data-testid="history-list">
        <li
          v-for="item in reversedHistory"
          :key="item.index"
          class="history-item"
          :class="{ 'history-item--current': item.index === historyCursor }"
          :data-testid="`history-item-${item.index}`"
          :data-current="item.index === historyCursor ? 'true' : undefined"
          @click="jumpToHistory(item.index)"
        >
          <span class="history-item__label">{{ item.label }}</span>
          <span v-if="item.index === historyCursor" class="history-item__current-badge">Current</span>
        </li>
      </ul>
    </div>
  </CollapsibleSection>
</template>

<style scoped>
/* --- History Section (RP-5) --------------------------------------------- */
.history-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.history-empty {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  padding: var(--space-2) 0;
}

.history-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.history-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  user-select: none;
  transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.history-item:hover {
  border-color: var(--text-muted);
}

.history-item--current {
  border-color: var(--accent);
  background: var(--bg-hover);
  font-weight: 600;
}

.history-item__current-badge {
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--accent);
}
</style>
