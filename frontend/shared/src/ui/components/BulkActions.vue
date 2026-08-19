<script setup lang="ts">
/**
 * The bulk action bar (Phase 13, tasks 2 and 3).
 *
 * **A 10 MP ceiling means a 24–45 MP camera fails the resolution check on
 * virtually every frame.** Resizing is the normal path, not an exception, so
 * this is built for one approval covering four hundred shots rather than four
 * hundred prompts — and the resize action arrives already chosen.
 *
 * Transport-free: groups in, a request out.
 */
import { computed, ref, watch } from 'vue';
import type { FailureGroup } from '@phototools/shared';

const props = defineProps<{
  groups: FailureGroup[];
  /** Which failure class the grid is filtered to, if any. */
  filter: string;
  busy?: boolean;
}>();

const emit = defineEmits<{
  (e: 'filter', failure: string): void;
  (e: 'apply', request: { failure: string; action: string; date: string | null }): void;
}>();

/** The action chosen for each failure class, keyed by class. */
const chosen = ref<Record<string, string>>({});
/** A manual date, for the classes whose action needs one. */
const dates = ref<Record<string, string>>({});

/**
 * Seed every group with its default action.
 *
 * F13's default for `too_many_pixels` is `resize`, which is why "auto-resize on
 * by default" needs no special case here: the default *is* the resize, and
 * taking it is one press.
 */
watch(
  () => props.groups,
  (groups) => {
    for (const group of groups) {
      if (!chosen.value[group.failure]) {
        chosen.value[group.failure] =
          group.default_action ?? group.actions[0] ?? 'skip';
      }
    }
  },
  { immediate: true, deep: true },
);

/**
 * Failed **checks**, not failing shots.
 *
 * One frame from a 24 MP camera fails resolution *and* size, so it appears in
 * two groups. Adding the counts and calling the result "shots" would report 736
 * of them on a 400-frame card, which is the kind of number that makes somebody
 * stop trusting the screen.
 */
const failedChecks = computed(() =>
  props.groups.reduce((sum, group) => sum + group.count, 0),
);

/**
 * The actions that need a date typed in — the two manual ones.
 *
 * `bulk_shift` is deliberately not among them: it takes its delta from the
 * clock offset F12 already worked out across the card, which is the whole
 * reason a drifted camera is one decision rather than four hundred.
 */
const MANUAL_DATE_ACTIONS = new Set(['enter_date_manually', 'redate_manually']);

function needsDate(failure: string): boolean {
  return MANUAL_DATE_ACTIONS.has(chosen.value[failure]);
}

function ready(group: FailureGroup): boolean {
  if (!needsDate(group.failure)) return true;
  return Boolean(dates.value[group.failure]?.trim());
}

function apply(group: FailureGroup) {
  emit('apply', {
    failure: group.failure,
    action: chosen.value[group.failure],
    date: needsDate(group.failure) ? dates.value[group.failure] : null,
  });
}

/** `too_many_pixels` → `too many pixels`. */
function label(failure: string): string {
  return failure.replace(/_/g, ' ');
}
</script>

<template>
  <section v-if="groups.length" class="bulk" aria-label="Bulk actions">
    <header class="bulk-head">
      <h3>
        {{ groups.length }} decision{{ groups.length === 1 ? '' : 's' }} to make
      </h3>
      <button
        v-if="filter"
        type="button"
        class="ghost"
        @click="emit('filter', '')"
      >
        Clear filter
      </button>
    </header>

    <p class="muted small">
      One decision per failure class, applied to every shot sharing it —
      {{ failedChecks }} failed check{{ failedChecks === 1 ? '' : 's' }} in all.
      Resizing is preselected: a 10&nbsp;megapixel ceiling fails almost every
      frame from a modern camera, so it is the normal path rather than an
      exception.
    </p>

    <ul class="groups">
      <li
        v-for="group in groups"
        :key="group.failure"
        class="group"
        :data-testid="`group-${group.failure}`"
      >
        <button
          type="button"
          class="group-name"
          :aria-pressed="filter === group.failure"
          :data-active="filter === group.failure"
          @click="emit('filter', filter === group.failure ? '' : group.failure)"
        >
          <span class="count" :data-testid="`count-${group.failure}`">{{ group.count }}</span>
          {{ label(group.failure) }}
        </button>

        <label class="field action">
          <span class="sr-only">Action for {{ label(group.failure) }}</span>
          <select
            v-model="chosen[group.failure]"
            :data-testid="`action-${group.failure}`"
            :disabled="busy"
          >
            <option v-for="action in group.actions" :key="action" :value="action">
              {{ label(action) }}
            </option>
          </select>
        </label>

        <input
          v-if="needsDate(group.failure)"
          v-model="dates[group.failure]"
          type="text"
          class="date"
          placeholder="2024-05-01T12:00:00"
          :disabled="busy"
        />

        <button
          type="button"
          class="primary"
          :data-testid="`apply-${group.failure}`"
          :disabled="busy || !ready(group)"
          @click="apply(group)"
        >
          Apply to {{ group.count }}
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.bulk {
  border: var(--border-hair);
  border-radius: var(--radius-none);
  padding: var(--space-4);
  background: var(--bg-panel);
  display: grid;
  gap: var(--space-3);
}

.bulk-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}
/* label-lg: this is a subhead within a screen, not a page title, so it stays on
   the label face rather than the display one. */
.bulk-head h3 {
  font-family: var(--font-label);
  font-size: 18px;
  letter-spacing: 0.1em;
}

.groups {
  list-style: none;
  display: grid;
  gap: var(--space-3);
}

.group {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
}

/* A failure class reads as a selectable terminal row: pick it, and the border
   lights. */
.group-name {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  background: var(--bg-elevated);
  border: var(--border-hair);
  color: var(--text);
  padding: var(--space-2) var(--space-3);
  flex: 1 1 180px;
  justify-content: flex-start;
  text-align: left;
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.08em;
}
.group-name[data-active='true'] {
  border-color: var(--accent);
  color: var(--accent);
  box-shadow: var(--glow-phosphor);
}

.count {
  margin-left: auto;
  font-family: var(--font-body);
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  min-width: 2.5ch;
  text-align: right;
  color: var(--text-muted);
}

.action {
  flex: 0 1 214px;
}
.action select {
  min-height: 44px;
}
.date {
  flex: 1 1 200px;
}
.small {
  font-size: 12px;
  color: var(--text-muted);
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
  border: 0;
}
</style>
