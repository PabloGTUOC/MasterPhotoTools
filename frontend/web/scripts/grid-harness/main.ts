/**
 * A harness for measuring {@link ShotGrid} against a full card.
 *
 * Not part of either application: `check-grid.mjs` serves this directory on its
 * own and drives it with a browser. It exists because the acceptance criterion
 * — *"a 400-shot session renders and stays responsive"* — is a measurement, and
 * a measurement needs something to measure.
 *
 * The grid is transport-free, which is what makes this possible at all: no
 * `@host/api`, no Tauri, no server.
 */
import { createApp, h, ref } from 'vue';
import type { ShotRow, ShotVerdict } from '@phototools/shared';
import ShotGrid from '@ui/components/ShotGrid.vue';
import '@ui/style.css';

/** How many frames a full card carries. The build plan says four hundred. */
const SHOTS = Number(new URLSearchParams(location.search).get('shots') ?? 400);

/**
 * A realistic card: a 24 MP camera against a 10 MP ceiling, so nearly every
 * frame fails the resolution check. That is the case the UI is built for, and
 * the expensive one — four hundred rows each carrying three chips.
 */
function card(count: number): { shots: ShotRow[]; verdicts: Record<string, ShotVerdict> } {
  const shots: ShotRow[] = [];
  const verdicts: Record<string, ShotVerdict> = {};

  for (let i = 0; i < count; i += 1) {
    const stem = `IMG_${String(i).padStart(4, '0')}`;
    const raw = i % 3 === 0;
    const oversized = i % 10 !== 7;

    shots.push({
      stem,
      candidate_kind: raw ? 'raw' : 'jpeg',
      candidate_path: `/Volumes/EOS_DIGITAL/DCIM/100CANON/${stem}.JPG`,
      bytes: oversized ? 12_400_000 : 3_100_000,
      width: oversized ? 6000 : 3000,
      height: oversized ? 4000 : 2000,
      megapixels: oversized ? 24 : 6,
      capture: `2024-05-01T${String(8 + (i % 10)).padStart(2, '0')}:${String(i % 60).padStart(2, '0')}:00`,
      camera: 'CANON EOS R6',
      asset_count: raw ? 2 : 1,
      needs_derivation: raw && i % 6 === 0,
    });

    verdicts[stem] = {
      stem,
      status: oversized ? 'fail' : 'pass',
      checks: [
        {
          rule: 'CaptureDate',
          status: i % 25 === 0 ? 'fail' : 'pass',
          failure: i % 25 === 0 ? 'no_date' : null,
          detail: i % 25 === 0 ? 'No capture date in metadata' : 'Capture date present',
        },
        {
          rule: 'Resolution',
          status: oversized ? 'fail' : 'pass',
          failure: oversized ? 'too_many_pixels' : null,
          detail: oversized ? '24.0 MP is over the 10 MP ceiling' : '6.0 MP',
        },
        {
          rule: 'FileSize',
          status: oversized ? 'fail' : 'pass',
          failure: oversized ? 'too_large' : null,
          detail: oversized ? '11.8 MB is over the 10 MB ceiling' : '3.0 MB',
        },
      ],
    };
  }

  return { shots, verdicts };
}

const { shots, verdicts } = card(SHOTS);
const filter = ref('');

createApp({
  setup() {
    // The buttons are what the measurement drives; the grid is what it measures.
    return () =>
      h('div', { style: 'padding:12px;display:grid;gap:12px' }, [
        h('div', { style: 'display:flex;gap:8px;flex-wrap:wrap' }, [
          h(
            'button',
            {
              'data-testid': 'filter-pixels',
              onClick: () => (filter.value = 'too_many_pixels'),
            },
            'too many pixels',
          ),
          h(
            'button',
            { 'data-testid': 'filter-none', onClick: () => (filter.value = '') },
            'all',
          ),
        ]),
        h(ShotGrid, { shots, verdicts, filter: filter.value }),
      ]);
  },
}).mount('#app');
