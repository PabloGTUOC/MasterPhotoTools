<script setup lang="ts">
/**
 * The card review experience (Phase 13).
 *
 * A desktop view, and deliberately not a shared one. Specification §2.3 puts
 * scanning, validating, converting and resizing on the Mac, because that is
 * where the card reader is — the server has no card to read, so a shared view
 * would be a view one of its two builds could not honestly render.
 *
 * The shape of the work is: look → scan → review → decide in bulk → derive →
 * hand over. The last step is the one this screen has to be clear about,
 * because after it the desktop has nothing left to do and everything that
 * happens next happens somewhere else.
 */
import { computed, ref } from 'vue';
import type {
  CardScan,
  CardSummary,
  CardValidation,
  ShotVerdict,
  ThresholdOverrides,
} from '@phototools/shared';
import BulkActions from '@ui/components/BulkActions.vue';
import JobProgress from '@ui/components/JobProgress.vue';
import PathField from '@ui/components/PathField.vue';
import ShotGrid from '@ui/components/ShotGrid.vue';
import { useRoots } from '@ui/useRoots';
import { desktop } from '../api';

/** Where the desktop is in the card's life. */
type Stage = 'idle' | 'reviewing' | 'handed-over';

const cardPath = ref('');
const derivedDir = ref('');
const stagingDir = ref('');

// The folders the pickers may offer, and the lister they walk with. A card
// mounted under /Volumes only appears once /Volumes is a configured root —
// G6 refuses it otherwise, and scan_card would refuse it too.
const { roots } = useRoots();

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
/** The session the server now holds, once the handoff has finished. */
const sessionId = ref<string | null>(null);

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

/** One action, applied to every shot sharing one failure (F13). */
async function applyBulk(request: {
  failure: string;
  action: string;
  date: string | null;
}) {
  const out = derivedDir.value.trim();
  if (!out) {
    failure.value = 'Set an output folder — remediation writes new files, never over the originals.';
    return;
  }

  await guard(async () => {
    const result = await desktop.remediate({
      path: cardPath.value.trim(),
      failure: request.failure,
      action: request.action,
      date: request.date,
      out_dir: out,
      dry_run: false,
      thresholds: thresholds(),
    });
    if (typeof result === 'string') jobId.value = result;

    // The card has changed underneath the verdicts, so they are re-read rather
    // than patched: a resize alters dimensions, which alters two of the three
    // rules.
    validation.value = await desktop.validateCard({
      path: cardPath.value.trim(),
      thresholds: thresholds(),
    });
  });
}

/** F14 — derive JPEGs for the RAW-only shots. */
async function derive() {
  const out = derivedDir.value.trim();
  if (!out) {
    failure.value = 'Set an output folder for the derived JPEGs.';
    return;
  }
  const id = await guard(() =>
    desktop.deriveRaw({
      path: cardPath.value.trim(),
      out_dir: out,
      thresholds: thresholds(),
    }),
  );
  if (id) jobId.value = id;
}

/**
 * Hand the derivatives to the server (F16).
 *
 * The point at which the desktop's work ends. The screen says so afterwards,
 * because "nothing is happening here any more" is otherwise indistinguishable
 * from "something has gone quiet".
 */
async function handOff() {
  const out = derivedDir.value.trim();
  const staging = stagingDir.value.trim();
  if (!out || !staging) {
    failure.value = 'Set both the derived folder and the staging folder on the NAS share.';
    return;
  }

  await guard(async () => {
    const id = await desktop.handOffCard(cardPath.value.trim(), out, staging);
    jobId.value = id;

    let last = '';
    await desktop.watchJob(id, (event) => (last = event.message));

    // The session id is the only handle anybody has on this card once the
    // server owns it, and publishing is addressed by session.
    sessionId.value = last.match(/session (\S+)/)?.[1] ?? null;
    stage.value = 'handed-over';
  });
}

const awaitingDerivation = computed(() => scan.value?.awaiting_derivation ?? 0);
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
        :list="list"
      />

      <PathField
        v-model="derivedDir"
        label="Output folder for new files"
        placeholder="~/Pictures/ingest/2024-05-01"
        hint="Remediation and derivation write here. Originals are never modified."
        :roots="roots"
        :list="list"
      />

      <PathField
        v-model="stagingDir"
        label="Staging folder on the NAS share"
        placeholder="/Volumes/photos/staging"
        :roots="roots"
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
      <section v-if="validation" class="counts">
        <span class="pill" data-tone="ok">{{ validation.passing }} passing</span>
        <span class="pill" data-tone="bad">{{ validation.failing }} failing</span>
        <span v-if="awaitingDerivation" class="pill">
          {{ awaitingDerivation }} awaiting derivation
        </span>
      </section>

      <p v-if="validation?.clock_offset" class="notice">
        Every date on this card sits about {{ validation.clock_offset.median_age_days }} days
        from now, within a {{ validation.clock_offset.spread_days }}-day spread — a camera
        clock that was never set, not {{ validation.clock_offset.affected }} separate mistakes.
        One bulk shift of <code>{{ validation.clock_offset.shift }}</code> corrects all of them.
      </p>

      <BulkActions
        :groups="validation?.groups ?? []"
        :filter="filter"
        :busy="busy"
        @filter="filter = $event"
        @apply="applyBulk"
      />

      <ShotGrid :shots="shots" :verdicts="verdicts" :filter="filter" />

      <div class="row">
        <button
          v-if="awaitingDerivation"
          type="button"
          class="secondary"
          :disabled="busy"
          @click="derive"
        >
          Derive {{ awaitingDerivation }} RAW-only shot{{ awaitingDerivation === 1 ? '' : 's' }}
        </button>
        <button type="button" class="primary" :disabled="busy" @click="handOff">
          Hand over to the server
        </button>
      </div>

      <ul v-if="scan?.problems.length" class="problems">
        <li v-for="problem in scan.problems" :key="problem.rel_path" class="muted small">
          {{ problem.rel_path }} — {{ problem.detail }}
        </li>
      </ul>
    </template>

    <!-- Task 5: the moment the desktop's work ends has to be unmistakable. -->
    <section v-if="stage === 'handed-over'" class="handover" aria-live="polite">
      <h2>The server has taken over</h2>
      <p>
        The derivatives are on the NAS and verified by hash. Nothing further
        happens on this machine — you can close the lid.
      </p>
      <p v-if="sessionId" class="session">
        Session <code>{{ sessionId }}</code>
      </p>
      <p class="muted small">
        Publishing happens on the server, from the web interface, and needs a dry
        run reviewed first.
      </p>
      <button type="button" class="ghost" @click="stage = 'reviewing'">
        Back to the review
      </button>
    </section>
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
.handover {
  display: grid;
  gap: 10px;
  justify-items: start;
  padding: 16px;
  border: 1px solid var(--accent);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
}
.handover h2 {
  font-family: var(--font-label);
  font-size: 18px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.session { font-family: var(--font-body); font-size: 13px; word-break: break-all; }
.small { font-size: 13px; }
</style>
