<script setup lang="ts">
/**
 * Collapsible section container for photographic tool panels (ED-9).
 *
 * Transport-free: props in, events out, never `@host/api`.
 *
 * Accessibility & Design:
 * - Keyboard-accessible toggle button with aria-expanded.
 * - Minimum 40px touch targets for header toggle and reset buttons.
 * - Brutalist design adhering strictly to tokens and <= 2px radius.
 * - Individual section reset button emitting `@reset`.
 */
import { ref } from 'vue';

const props = withDefaults(
  defineProps<{
    title: string;
    defaultOpen?: boolean;
    showReset?: boolean;
    resetLabel?: string;
    disabled?: boolean;
    testId?: string;
  }>(),
  {
    defaultOpen: true,
    showReset: true,
    resetLabel: 'Reset',
    disabled: false,
    testId: undefined,
  },
);

const emit = defineEmits<{
  reset: [];
  'update:open': [open: boolean];
}>();

const isOpen = ref(props.defaultOpen);

function toggle() {
  if (props.disabled) return;
  isOpen.value = !isOpen.value;
  emit('update:open', isOpen.value);
}

function handleReset() {
  if (props.disabled) return;
  emit('reset');
}
</script>

<template>
  <div
    class="collapsible-section"
    :class="{ 'is-open': isOpen, 'is-disabled': disabled }"
    :data-testid="testId"
  >
    <div class="collapsible-section__header">
      <button
        type="button"
        class="collapsible-section__toggle"
        :data-testid="testId ? `${testId}-toggle` : undefined"
        :aria-expanded="isOpen"
        :disabled="disabled"
        @click="toggle"
      >
        <span class="collapsible-section__chevron" :class="{ 'is-expanded': isOpen }">
          <svg
            width="12"
            height="12"
            viewBox="0 0 12 12"
            fill="none"
            aria-hidden="true"
          >
            <path
              d="M4 2L8 6L4 10"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="square"
            />
          </svg>
        </span>
        <span class="collapsible-section__title">{{ title }}</span>
      </button>

      <div class="collapsible-section__actions">
        <slot name="actions" />
        <button
          v-if="showReset"
          type="button"
          class="ghost collapsible-section__reset-btn"
          :data-testid="testId ? `${testId}-reset` : undefined"
          :disabled="disabled"
          @click.stop="handleReset"
        >
          {{ resetLabel }}
        </button>
      </div>
    </div>

    <div
      v-show="isOpen"
      class="collapsible-section__content"
      :data-testid="testId ? `${testId}-content` : undefined"
    >
      <slot />
    </div>
  </div>
</template>

<style scoped>
.collapsible-section {
  border-bottom: 1px solid var(--border);
  background: var(--bg-panel);
}

.collapsible-section__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 40px;
  padding: var(--space-1) var(--space-2);
}

.collapsible-section__toggle {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  padding: 0 var(--space-2);
  background: transparent;
  border: none;
  color: var(--text-heading);
  font-family: var(--font-label);
  font-size: 16px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  cursor: pointer;
  border-radius: var(--radius-pill);
  flex: 1;
  text-align: left;
}

.collapsible-section__toggle:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.04);
}

.collapsible-section__toggle:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.collapsible-section__chevron {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  transition: transform var(--dur-fast) var(--ease);
  color: var(--text-muted);
}

.collapsible-section__chevron.is-expanded {
  transform: rotate(90deg);
  color: var(--accent);
}

.collapsible-section__title {
  flex: 1;
}

.collapsible-section__actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.collapsible-section__reset-btn {
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-body);
  font-size: 13px;
  color: var(--text-muted);
  border-radius: var(--radius-pill);
  cursor: pointer;
}

.collapsible-section__reset-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.collapsible-section__content {
  padding: var(--space-2) var(--space-3) var(--space-3);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.is-disabled {
  opacity: 0.5;
  pointer-events: none;
}
</style>
