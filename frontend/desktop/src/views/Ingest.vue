<script setup lang="ts">
/**
 * The card review experience (Phase 13).
 *
 * A desktop view, and deliberately not a shared one. Specification §2.3 puts
 * scanning, validating, converting and resizing on the Mac, because that is
 * where the card reader is — the server has no card to read, so a shared view
 * would be a view one of its two builds could not honestly render.
 *
 * **This screen reports; the tools act.** The shape of the work is: look →
 * scan → read the verdicts → copy to a folder. What is wrong with a frame is
 * said here and fixed in the tab that fixes it — a card screen that both
 * diagnosed and repaired duplicated the Dates tab, and now the Geotag tab as
 * well (`docs/workflow-plan.md`).
 */
import { computed, ref } from 'vue';
import type {
  CardScan,
  CardSummary,
  CardValidation,
  ShotVerdict,
  ThresholdOverrides,
} from '@phototools/shared';
import JobProgress from '@ui/components/JobProgress.vue';
import PathField from '@ui/components/PathField.vue';
import ShotGrid from '@ui/components/ShotGrid.vue';
import { useRoots } from '@ui/useRoots';
import { desktop } from '../api';

/** Where the desktop is in the card's life. */
type Stage = 'idle' | 'reviewing';

const cardPath = ref('');

/**
 * Where the frames that passed should be copied to.
 *
 * The other road off the card (`docs/publish-folder-plan.md`): instead of
 * handing a session to the server, put the photographs somewhere the tools can
 * work on them. Local when you intend to edit them, the NAS when you do not.
 */
const workingDir = ref('');

/** What the last copy reported. */
const copied = ref<string | null>(null);

// The folders the pickers may offer, and the lister they walk with. A card
// mounted under /Volumes only appears once /Volumes is a configured root —
// G6 refuses it otherwise, and scan_card would refuse it too.
const { roots, failure: rootsError } = useRoots();

/**
 * F12's two ceilings, for this card.
 *
 * Seeded with what the installation is configured with, and sent with every
 * request so one card can differ from the default without editing config.json.
 *
 * A resolution ceiling of 0 means none at all — publishing is limited by file
 * size, and a 40 MP frame inside the byte cap is worth keeping whole rather
 * than resizing to a limit nothing is enforcing.
 */
const maxMegapixels = ref(0);
const maxOutputMb = ref(10);

function thresholds(): ThresholdOverrides {
  return {
    max_megapixels: maxMegapixels.value,
    max_output_bytes: Math.round(maxOutputMb.value * 1024 * 1024),
  };
}
const list = (path: string) => desktop.list(path);

const summary = ref<CardSummary | null>(null);
const scan = ref<CardScan | null>(null);
const validation = ref<CardValidation | null>(null);

const filter = ref('');
const busy = ref(false);
const failure = ref<string | null>(null);
const jobId = ref<string | null>(null);
const stage = ref<Stage>('idle');

const verdicts = computed<Record<string, ShotVerdict>>(() => {
  const out: Record<string, ShotVerdict> = {};
  for (const verdict of validation.value?.shots ?? []) out[verdict.stem] = verdict;
  return out;
});

const shots = computed(() => scan.value?.shots ?? []);

async function guard<T>(work: () => Promise<T>): Promise<T | undefined> {
  busy.value = true;
  failure.value = null;
  try {
    return await work();
  } catch (e) {
    failure.value = e instanceof Error ? e.message : String(e);
    return undefined;
  } finally {
    busy.value = false;
  }
}

/** F10's cheap look: entries and the ledger, no photographs. */
async function look() {
  const path = cardPath.value.trim();
  if (!path) {
    failure.value = 'Point at a card or a folder first.';
    return;
  }
  const result = await guard(() => desktop.summariseCard(path));
  if (result) summary.value = result;
}

/** Scan, then read the rows and validate them (F11, F12). */
async function review() {
  const path = cardPath.value.trim();
  if (!path) {
    failure.value = 'Point at a card or a folder first.';
    return;
  }

  await guard(async () => {
    const id = await desktop.scanCard(path);
    jobId.value = id;
    // The scan is a job; the rows and verdicts are read once it has recorded
    // what it found.
    await desktop.watchJob(id, () => {});

    scan.value = await desktop.readCard(path);
    validation.value = await desktop.validateCard({ path, thresholds: thresholds() });
    stage.value = 'reviewing';
  });
}

/**
 * Copy the frames that passed to a folder.
 *
 * Where the card's contents go, and where the desktop's work ends. The tools
 * run on what lands here.
 *
 * **The card is never written to (G5).** `deliver_card` refuses a destination
 * inside the folder being read before a byte is copied — this is the one
 * operation whose destination somebody types, while looking at the card.
 *
 * Copies the shots that did not *fail*. A warning — the odd frame with no
 * coordinates — is something to see in the table and decide about, not a
 * reason to leave a photograph behind.
 */
async function copyToFolder() {
  const destination = workingDir.value.trim();
  if (!destination) {
    failure.value = 'Choose a folder to copy the photographs to.';
    return;
  }

  await guard(async () => {
    copied.value = null;
    const id = await desktop.deliverCard(cardPath.value.trim(), destination);
    jobId.value = id;
    await desktop.watchJob(id, (event) => {
      if (event.message) copied.value = event.message;
    });
  });
}

const awaitingDerivation = computed(() => scan.value?.awaiting_derivation ?? 0);

/** One thing that needs doing, how many frames need it, and where it is done. */
interface Needed {
  count: number;
  what: string;
  where: string;
}

/**
 * What the card needs, in the order somebody would act on it.
 *
 * The counts alone say a card is imperfect; this says what to do about it. Each
 * line names the tab that does the job, because this screen reports and the
 * tools act — and a person reading "12 have no location" should not have to
 * work out which tab that means.
 *
 * Lines with no frames behind them are dropped: a list of zeroes reads as work
 * outstanding.
 */
const needed = computed<Needed[]>(() => {
  const rows = Object.values(verdicts.value);
  const failed = (verdict: ShotVerdict, ...classes: string[]) =>
    verdict.checks.some((c) => c.failure !== null && classes.includes(c.failure));
  const warns = (verdict: ShotVerdict, rule: string) =>
    verdict.checks.some((c) => c.rule === rule && c.status === 'warn');
  const count = (fn: (v: ShotVerdict) => boolean) => rows.filter(fn).length;

  return [
    {
      count: count((v) => failed(v, 'no_date')),
      what: 'have no capture date',
      where: 'Dates tab',
    },
    {
      count: count((v) => failed(v, 'date_out_of_range', 'date_out_of_range_batch')),
      what: 'have a capture date outside the window',
      where: 'Dates tab',
    },
    {
      count: count((v) => warns(v, 'location')),
      what: 'have no location, where others on this card do',
      where: 'Geotag tab',
    },
    {
      count: count((v) => failed(v, 'too_many_pixels')),
      what: 'are above the resolution ceiling',
      where: 'Transform tab',
    },
    {
      count: count((v) => failed(v, 'too_large')),
      what: 'are larger than the size limit',
      where: 'Transform tab',
    },
    {
      count: awaitingDerivation.value,
      what: 'are RAW with no JPEG beside them',
      where: 'RAW tab',
    },
  ].filter((line) => line.count > 0);
});

/** Frames with nothing *failing*. A warning is worth seeing, not a blockage. */
const ready = computed(
  () => (validation.value?.shots.length ?? 0) - (validation.value?.failing ?? 0),
);

</script>

<template>
  <section class="page">
    <header class="head">
      <h1>Ingest</h1>
      <p class="muted">
        Review a card, decide in bulk, and hand the results to the server.
      </p>
    </header>

    <div class="form">
      <PathField
        v-model="cardPath"
        label="Card or folder"
        placeholder="/Volumes/EOS_DIGITAL"
        hint="A card is only offered here if its mount point is one of the configured folders."
        :roots="roots"
        :roots-error="rootsError"
        :list="list"
      />


      <PathField
        v-model="workingDir"
        label="Copy the photographs to"
        placeholder="~/Pictures/2026/berlin"
        hint="Where the frames that passed should land. Local if you mean to work on them, the NAS if you do not. The card itself is never written to."
        :roots="roots"
        :roots-error="rootsError"
        :list="list"
      />


      <fieldset class="field limits">
        <legend>Limits for this card</legend>
        <p class="muted">
          What a photograph has to satisfy to be publishable. Set per card, so one roll can differ
          from the default without changing the configuration.
        </p>
        <div class="limits__grid">
          <label class="field">
            <span>File size ceiling (MB)</span>
            <input v-model.number="maxOutputMb" type="number" min="1" step="0.5" />
            <small class="muted">A frame over this is resized until it fits.</small>
          </label>
          <label class="field">
            <span>Resolution ceiling (MP)</span>
            <input v-model.number="maxMegapixels" type="number" min="0" />
            <small class="muted">0 means no limit — the frame is kept whole.</small>
          </label>
        </div>
      </fieldset>

      <div class="row">
        <button type="button" class="secondary" :disabled="busy" @click="look">Look</button>
        <button type="button" class="primary" :disabled="busy" @click="review">Scan and review</button>
      </div>
    </div>

    <p v-if="failure" class="error">{{ failure }}</p>

    <section v-if="summary" class="summary" aria-live="polite">
      <strong>{{ summary.label ?? 'Unlabelled' }}</strong>
      <span class="muted">
        {{ summary.shots }} shot{{ summary.shots === 1 ? '' : 's' }},
        {{ summary.new_shots }} new{{ summary.seen_before ? ' — seen before' : '' }}
      </span>
      <span v-if="!summary.looks_like_a_card" class="muted small">
        No DCIM folder; treating it as a plain directory.
      </span>
    </section>

    <JobProgress :job-id="jobId" />

    <template v-if="stage === 'reviewing'">
      <section v-if="validation" class="status" aria-live="polite">
        <h2 class="status__head">
          // {{ validation.shots.length }} FRAME{{ validation.shots.length === 1 ? '' : 'S' }}
          // {{ ready }} READY //
        </h2>

        <!-- What to do, not just what is wrong. Each line names the tab. -->
        <ul v-if="needed.length" class="needs">
          <li v-for="line in needed" :key="line.what">
            <strong>{{ line.count }}</strong> {{ line.what }}
            <span class="needs__where">→ {{ line.where }}</span>
          </li>
        </ul>
        <p v-else class="muted small">
          Nothing needs attention. Copy them to a folder and carry on.
        </p>
      </section>

      <p v-if="validation?.clock_offset" class="notice">
        Every date on this card sits about {{ validation.clock_offset.median_age_days }} days
        from now, within a {{ validation.clock_offset.spread_days }}-day spread — a camera
        clock that was never set, not {{ validation.clock_offset.affected }} separate mistakes.
        A shift of <code>{{ validation.clock_offset.shift }}</code> in the Dates tab corrects all
        of them at once.
      </p>

      <ShotGrid :shots="shots" :verdicts="verdicts" :filter="filter" />

      <div class="row">
        <button type="button" class="primary" :disabled="busy" @click="copyToFolder">
          Copy to a folder
        </button>
      </div>

      <p v-if="copied" class="copied" role="status">{{ copied }}</p>

      <ul v-if="scan?.problems.length" class="problems">
        <li v-for="problem in scan.problems" :key="problem.rel_path" class="muted small">
          {{ problem.rel_path }} — {{ problem.detail }}
        </li>
      </ul>
    </template>

  </section>
</template>

<style scoped>
.limits__grid {
  display: grid;
  gap: var(--space-3);
  grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
  /* Each field is label, input, hint. Without this the shorter of two fields
     stretches its input to fill the taller row and the two stop lining up. */
  align-items: start;
}
.limits__grid .field > small {
  /* Reserve the line whether or not the text wraps, so the inputs above stay
     on the same baseline at any width. */
  min-height: 2.4em;
}
.limits > .muted {
  margin-bottom: var(--space-2);
}

.status {
  display: grid;
  gap: var(--space-2);
}
.status__head {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.needs {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  font-size: 13px;
}
.needs strong {
  /* The number is what the eye lands on; the sentence explains it. */
  display: inline-block;
  min-width: 2.5em;
  text-align: right;
  color: var(--text-heading);
  font-variant-numeric: tabular-nums;
}
.needs__where {
  color: var(--accent-warm);
}

.copied {
  font-size: 13px;
  color: var(--accent);
}

.page { display: grid; gap: 16px; padding: 16px; max-width: 1100px; }
.head h1 {
  font-size: 40px;
}
.form { display: grid; gap: 12px; }
.row { display: flex; gap: 10px; flex-wrap: wrap; }
.summary {
  display: flex;
  gap: 10px;
  align-items: baseline;
  flex-wrap: wrap;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
}
.counts { display: flex; gap: 8px; flex-wrap: wrap; }
.pill {
  font-size: 13px;
  padding: 4px 10px;
  border-radius: var(--radius-none);
  border: 1px solid var(--border);
  color: var(--text-muted);
}
.pill[data-tone='ok'] { color: var(--accent); border-color: var(--accent); }
.pill[data-tone='bad'] { color: var(--danger); border-color: var(--danger); }
.notice {
  padding: 10px 12px;
  border: 1px solid var(--accent-warm);
  border-radius: var(--radius-none);
  color: var(--accent-warm);
  font-size: 14px;
}
.problems { list-style: none; display: grid; gap: 4px; }
.session { font-family: var(--font-body); font-size: 13px; word-break: break-all; }
.small { font-size: 13px; }
</style>
