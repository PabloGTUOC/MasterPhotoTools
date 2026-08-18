/**
 * A harness for {@link BulkActions}.
 *
 * The acceptance criterion is *"bulk-approving all resizes is one action"*,
 * which is a claim about a press: what it starts with selected, and how many
 * shots it covers. `check-ingest.mjs` drives this and reads `window.__applied`.
 */
import { createApp, h, ref } from 'vue';
import type { FailureGroup } from '@phototools/shared';
import BulkActions from '@ui/components/BulkActions.vue';
import '@ui/style.css';

const SHOTS = Number(new URLSearchParams(location.search).get('shots') ?? 400);

/**
 * What a real card produces: a 24 MP camera against a 10 MP ceiling fails
 * almost every frame on resolution and size, with a handful of missing dates.
 */
const groups: FailureGroup[] = [
  {
    failure: 'too_many_pixels',
    count: Math.round(SHOTS * 0.9),
    actions: ['resize', 'publish_anyway', 'skip'],
    default_action: 'resize',
  },
  {
    failure: 'too_large',
    count: Math.round(SHOTS * 0.9),
    actions: ['reencode_lower', 'resize', 'skip'],
    default_action: 'reencode_lower',
  },
  {
    failure: 'no_date',
    count: 16,
    actions: ['enter_date_manually', 'use_file_modification_time', 'skip'],
    default_action: null,
  },
];

declare global {
  interface Window {
    __applied: unknown[];
  }
}
window.__applied = [];

const filter = ref('');

createApp({
  setup() {
    return () =>
      h('div', { style: 'padding:12px' }, [
        h(BulkActions, {
          groups,
          filter: filter.value,
          onFilter: (next: string) => (filter.value = next),
          onApply: (request: unknown) => window.__applied.push(request),
        }),
      ]);
  },
}).mount('#app');
