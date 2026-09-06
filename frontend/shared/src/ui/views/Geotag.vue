<script setup lang="ts">
/**
 * Geotagging — join photographs to the places they were taken.
 *
 * Three steps in the order the work happens: load a track into the library,
 * see what a folder of photographs already carries, then match the ones that
 * are missing a position.
 *
 * Beyond the specification, which mentions neither GPS nor GPX; the reasoning
 * is in `docs/geotag-plan.md` (G9, G11).
 */
import { computed, ref, useTemplateRef } from 'vue';
import type {
  Decision,
  GeoScanRow,
  GeotagPreview,
  GeotagRequest,
  RecordedConflict,
  Resolution,
  TrackImportPreview,
  TrackSummary,
} from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import PathListField from '../components/PathListField.vue';
import TrackLibrary from '../components/TrackLibrary.vue';
import { useRoots } from '../useRoots';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const { roots } = useRoots();
const list = (path: string) => api.list(path);

const busy = ref(false);

// --- the library ----------------------------------------------------------
const tracks = ref<TrackSummary[]>([]);
const trackPath = ref('');
const pending = ref<TrackImportPreview | null>(null);

/** The settled disagreements for one track, once somebody asks to see them. */
const history = ref<{ id: string; conflicts: RecordedConflict[] } | null>(null);

/**
 * What the library last did, or last could not do.
 *
 * Kept apart from the tool's own failure line. Not being able to read the
 * library is a fact about the library, and putting it beside the button that
 * writes to photographs reads as though writing had failed.
 */
const note = ref<string | null>(null);
const noteTone = ref<'ok' | 'warn'>('ok');

function say(message: string, tone: 'ok' | 'warn' = 'ok') {
  note.value = message;
  noteTone.value = tone;
}

// --- the photographs ------------------------------------------------------
const paths = ref('');
const recursive = ref(true);
const scanned = ref<GeoScanRow[] | null>(null);

// --- the match ------------------------------------------------------------
const offset = ref('+02:00');
const clockCorrection = ref(0);
const mode = ref<'CarriedForward' | 'Nearest'>('CarriedForward');
/**
 * How old a fix may be before the answer is "this track does not know", in
 * hours.
 *
 * Hours rather than minutes because that is the scale the question lives on: a
 * movement tracker is right however long it has been silent, so the number is
 * not about staleness but about a photograph falling outside what the track
 * covers at all.
 */
const oldestFixHours = ref(12);
const overwriteExisting = ref(false);
const writeAltitude = ref(true);
const preview = ref<GeotagPreview | null>(null);

function pathList(): string[] {
  return paths.value
    .split('\n')
    .map((p) => p.trim())
    .filter(Boolean);
}

/**
 * `+02:00` as minutes east, or null where the field is empty.
 *
 * Empty is a real answer and not the same as zero: with no offset, a file that
 * carries none of its own is skipped rather than read as UTC — which would
 * move every photograph by whatever the offset really was.
 */
function offsetMinutes(): number | null {
  const raw = offset.value.trim();
  if (!raw) return null;
  const match = /^([+-])?(\d{1,2}):?(\d{2})$/.exec(raw);
  if (!match) return null;
  const sign = match[1] === '-' ? -1 : 1;
  return sign * (Number(match[2]) * 60 + Number(match[3]));
}

/** Minutes east as `+02:00`, which is how EXIF writes it. */
function formatOffset(minutes: number): string {
  const sign = minutes < 0 ? '-' : '+';
  const total = Math.abs(minutes);
  return `${sign}${String(Math.floor(total / 60)).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`;
}

const offsetIsReadable = computed(() => !offset.value.trim() || offsetMinutes() !== null);

function request(): GeotagRequest {
  return {
    paths: pathList(),
    recursive: recursive.value,
    utc_offset_minutes: offsetMinutes(),
    clock_correction_seconds: Number(clockCorrection.value) || 0,
    limits: {
      mode: mode.value,
      max_edge_seconds: Math.max(0, Number(oldestFixHours.value) || 0) * 3600,
    },
    overwrite_existing: overwriteExisting.value,
    write_altitude: writeAltitude.value,
  };
}

async function run<T>(work: () => Promise<T>): Promise<T | undefined> {
  busy.value = true;
  page.value?.setFailure(null);
  try {
    return await work();
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
    return undefined;
  } finally {
    busy.value = false;
  }
}

async function loadTracks() {
  try {
    tracks.value = await api.tracks();
  } catch (e) {
    say(
      `Could not read the track library: ${e instanceof Error ? e.message : String(e)}`,
      'warn',
    );
  }
}
loadTracks();

async function previewTrack() {
  if (!trackPath.value.trim()) {
    page.value?.setFailure('Choose a .gpx file first.');
    return;
  }
  note.value = null;
  const result = await run(() => api.previewTrackImport(trackPath.value.trim()));
  if (result) pending.value = result;
}

async function importTrack(resolution: Resolution, overrides: Decision[]) {
  const result = await run(() =>
    api.commitTrackImport({ path: trackPath.value.trim(), resolution, overrides }),
  );
  if (!result) return;

  pending.value = null;
  say(describeImport(result));
  await loadTracks();
}

function describeImport(result: {
  added: number;
  identical: number;
  kept_existing: number;
  took_new: number;
  stale_overrides: number[];
  name: string;
}): string {
  const parts = [`${result.added} fix(es) added from ${result.name}`];
  if (result.identical) parts.push(`${result.identical} already held`);
  if (result.kept_existing) parts.push(`${result.kept_existing} kept as the library had them`);
  if (result.took_new) parts.push(`${result.took_new} replaced`);
  // A decision that turned out not to be needed is reported, never dropped:
  // the library moved between the preview and the commit, and somebody who
  // settled a disagreement deserves to know it was no longer there.
  if (result.stale_overrides.length) {
    parts.push(
      `${result.stale_overrides.length} decision(s) were no longer needed and were not applied`,
    );
  }
  return `${parts.join(', ')}.`;
}

async function showHistory(id: string) {
  if (history.value?.id === id) {
    history.value = null;
    return;
  }
  const conflicts = await run(() => api.trackConflicts(id));
  if (conflicts) history.value = { id, conflicts };
}

async function removeTrack(id: string) {
  const removed = await run(() => api.deleteTrack(id));
  if (removed === undefined) return;
  say(
    `Forgot that track and the ${removed} fix(es) still attributed to it. Re-import the file to bring them back.`,
  );
  await loadTracks();
}

async function scan() {
  const first = pathList()[0];
  if (!first) {
    page.value?.setFailure('Add a folder of photographs.');
    return;
  }
  const rows = await run(() => api.scanGeo({ path: first, recursive: recursive.value }));
  if (rows) scanned.value = rows;
}

async function previewMatch() {
  if (!pathList().length) {
    page.value?.setFailure('Add a folder of photographs.');
    return;
  }
  if (!offsetIsReadable.value) {
    page.value?.setFailure('The offset should look like +02:00, or be left empty.');
    return;
  }
  const result = await run(() => api.planGeotag(request()));
  if (result) {
    preview.value = result;
    page.value?.setReviewed(true);
  }
}

async function apply() {
  if (!pathList().length) {
    page.value?.setFailure('Add a folder of photographs.');
    return;
  }
  const id = await run(() => api.applyGeotag(request()));
  if (id) page.value?.setJob(id);
}

function useSuggestion() {
  const suggested = preview.value?.suggestion;
  if (suggested) offset.value = formatOffset(suggested.minutes);
}

const counts = computed(() => {
  const rows = scanned.value ?? [];
  return {
    total: rows.length,
    complete: rows.filter((r) => r.status === 'Ok').length,
    missing: rows.filter((r) => r.status === 'NoLocation').length,
    undated: rows.filter((r) => r.status === 'NoDate' || r.status === 'NoDateOrLocation').length,
    unsupported: rows.filter((r) => r.status === 'NotSupported').length,
  };
});

const STATUS_WORDS: Record<GeoScanRow['status'], string> = {
  Ok: 'located',
  NoLocation: 'no location',
  NoDate: 'no date',
  NoDateOrLocation: 'nothing',
  NotSupported: 'not supported',
};

/** A mark as well as a colour: which files need work cannot live in a hue. */
const STATUS_MARKS: Record<GeoScanRow['status'], string> = {
  Ok: '✓',
  NoLocation: '·',
  NoDate: '!',
  NoDateOrLocation: '!',
  NotSupported: '–',
};

function stamp(value: string | null): string {
  return value ? value.replace('T', ' ').slice(0, 19) : '—';
}

/** A span of seconds, as a person would say it. */
function gap(seconds: number): string {
  if (seconds < 120) return `${seconds} s`;
  if (seconds < 7200) return `${Math.round(seconds / 60)} min`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  return minutes ? `${hours} h ${minutes} min` : `${hours} h`;
}

/**
 * Skips gathered by reason.
 *
 * One line per file is right when four files were declined for four reasons
 * and useless when three hundred were declined for one — an empty library
 * prints the same sentence three hundred times, and the sentence is the answer,
 * not the file names. The names are still there, behind the count.
 */
const skipGroups = computed(() => {
  const groups = new Map<string, string[]>();
  for (const skip of preview.value?.plan.skipped ?? []) {
    const names = groups.get(skip.reason) ?? [];
    names.push(skip.file.split('/').pop() ?? skip.file);
    groups.set(skip.reason, names);
  }
  return [...groups.entries()]
    .map(([reason, files]) => ({ reason, files }))
    .sort((a, b) => b.files.length - a.files.length);
});

const suggestionLine = computed(() => {
  const s = preview.value?.suggestion;
  if (!s) return null;
  const best = formatOffset(s.minutes);
  if (s.confident) {
    return `These photographs sit on the track at ${best}, within ${gap(s.median_gap_seconds)} of a recorded fix.`;
  }
  if (s.plausible_low_minutes !== s.plausible_high_minutes) {
    return `The track cannot separate ${formatOffset(s.plausible_low_minutes)} from ${formatOffset(
      s.plausible_high_minutes,
    )}; ${best} fits best. Its own sampling is too coarse to tell them apart.`;
  }
  return `${best} fits best, but only within ${gap(s.median_gap_seconds)} of a fix — the library may not cover these photographs at all.`;
});
</script>

<template>
  <ToolPage
    ref="page"
    title="Geotag"
    blurb="Place photographs from a GPS track your phone recorded. The track is matched on time, so the camera's clock and its time zone are what decide where each frame lands."
    has-preview
    apply-label="Write positions"
    :busy="busy"
    @preview="previewMatch"
    @apply="apply"
  >
    <template #form>
      <TrackLibrary
        v-model:path="trackPath"
        :tracks="tracks"
        :preview="pending"
        :roots="roots"
        :list="list"
        :busy="busy"
        :history="history"
        @preview="previewTrack"
        @history="showHistory"
        @cancel="pending = null"
        @import="importTrack"
        @remove="removeTrack"
      />

      <p v-if="note" class="note" :data-tone="noteTone" role="status">{{ note }}</p>

      <PathListField
        v-model="paths"
        label="Photographs — folders or files, one per line"
        placeholder="/mnt/photos/2026/berlin"
        :roots="roots"
        :list="list"
      />

      <div class="row">
        <label class="checkbox"><input v-model="recursive" type="checkbox" /> Include subfolders</label>
        <button type="button" class="secondary" :disabled="busy" @click="scan">
          What do these already have?
        </button>
      </div>

      <section v-if="scanned" class="scan" aria-live="polite">
        <h2 class="scan__head">
          // {{ counts.total }} FILES // {{ counts.missing }} MISSING A LOCATION
        </h2>

        <p v-if="!scanned.length" class="muted">
          Nothing here this tool reads. Check the folder, or tick Include subfolders.
        </p>
        <p v-else class="muted">
          {{ counts.complete }} already located, {{ counts.missing }} dated but not located,
          {{ counts.undated }} with no usable date, {{ counts.unsupported }} not supported.
        </p>

        <div v-if="scanned.length" class="scan__table">
          <div class="scan__row scan__row--head" role="row">
            <span role="columnheader">File</span>
            <span role="columnheader">Capture</span>
            <span role="columnheader">Location</span>
            <span role="columnheader">State</span>
          </div>
          <div
            v-for="row in scanned"
            :key="row.path"
            class="scan__row"
            role="row"
            :data-status="row.status"
          >
            <span class="scan__name" :title="row.path">{{ row.name }}</span>
            <span class="scan__date">
              {{ stamp(row.capture) }}
              <small v-if="row.utc_offset_minutes !== null" class="muted">
                {{ formatOffset(row.utc_offset_minutes) }} from the file
              </small>
              <small v-else-if="row.tag" class="muted">{{ row.tag }}</small>
            </span>
            <span class="scan__date">
              <template v-if="row.location">
                {{ row.location.latitude }} {{ row.location.latitude_ref }}
                {{ row.location.longitude }} {{ row.location.longitude_ref }}
              </template>
              <template v-else>—</template>
            </span>
            <span class="scan__state">
              <span class="mark" aria-hidden="true">{{ STATUS_MARKS[row.status] }}</span>
              {{ STATUS_WORDS[row.status] }}
            </span>
          </div>
        </div>
      </section>

      <fieldset class="field">
        <legend>The camera's clock</legend>
        <label class="field">
          <span>UTC offset</span>
          <input v-model="offset" type="text" placeholder="+02:00" inputmode="text" />
          <small class="muted">
            EXIF records local time with no zone, so this is what turns a capture time into a
            moment. A file that recorded its own offset uses that instead. Leave it empty and any
            file without one is skipped rather than guessed at.
          </small>
        </label>
        <label class="field">
          <span>Clock correction (seconds)</span>
          <input v-model.number="clockCorrection" type="number" step="1" />
          <small class="muted">For a camera whose clock runs fast or slow. Positive is later.</small>
        </label>
      </fieldset>

      <fieldset class="field">
        <legend>Matching</legend>
        <label class="radio">
          <input v-model="mode" type="radio" value="CarriedForward" />
          Carry forward — the last position recorded before the photograph
        </label>
        <label class="radio">
          <input v-model="mode" type="radio" value="Nearest" />
          Nearest — the closest fix in time, from either side
        </label>
        <small class="muted">
          Every position written is one the phone recorded; nothing is computed between two of
          them. A tracker that reports when you move leaves no gaps to fill — between two fixes you
          were still at the first, which is why carrying forward is the default and why the nearer
          fix is often the wrong one.
        </small>
        <div class="row">
          <label class="field">
            <span>Stop trusting a fix after (hours)</span>
            <input v-model.number="oldestFixHours" type="number" min="0" step="1" />
            <small class="muted">
              Not about the fix going stale — a tracker that reports on movement is right however
              long it stays quiet, because the silence <em>is</em> the evidence that you did not
              move. This is for the other case: a photograph from a day this track knows nothing
              about, which would otherwise take the last fix of a different trip and look exactly
              like a real answer. 0 removes the limit; every row reports the age of the fix it
              used.
            </small>
          </label>
        </div>
        <div class="row">
          <label class="checkbox">
            <input v-model="overwriteExisting" type="checkbox" />
            Overwrite a position the file already has
          </label>
          <label class="checkbox">
            <input v-model="writeAltitude" type="checkbox" /> Write altitude
          </label>
        </div>
      </fieldset>
    </template>

    <template #preview>
      <section v-if="preview" class="scan" aria-live="polite">
        <h2 class="scan__head">
          // {{ preview.matched }} WOULD BE LOCATED // {{ preview.unmatched }} SKIPPED
        </h2>

        <p v-if="suggestionLine" class="suggestion">
          {{ suggestionLine }}
          <button
            v-if="preview.suggestion && formatOffset(preview.suggestion.minutes) !== offset"
            type="button"
            class="ghost"
            @click="useSuggestion"
          >
            Use {{ formatOffset(preview.suggestion.minutes) }}
          </button>
        </p>

        <div v-if="preview.plan.actions.length" class="scan__table">
          <div class="scan__row scan__row--plan scan__row--head" role="row">
            <span role="columnheader">File</span>
            <span role="columnheader">Capture</span>
            <span role="columnheader">Would become</span>
            <span role="columnheader">From a fix</span>
          </div>
          <div
            v-for="action in preview.plan.actions"
            :key="action.path"
            class="scan__row scan__row--plan"
            role="row"
          >
            <span class="scan__name" :title="action.path">{{ action.name }}</span>
            <span class="scan__date">
              {{ stamp(action.capture) }}
              <small class="muted">
                {{ formatOffset(action.offset_minutes) }}
                {{ action.offset_source === 'File' ? 'from the file' : 'as set' }}
              </small>
            </span>
            <span class="scan__date">
              {{ action.exif.latitude }} {{ action.exif.latitude_ref }}
              {{ action.exif.longitude }} {{ action.exif.longitude_ref }}
              <small v-if="action.replaces" class="muted">
                over {{ action.replaces.latitude }} {{ action.replaces.latitude_ref }}
              </small>
            </span>
            <!-- How far the answer is from an observation. The whole measure of
                 how much to trust the row, so it is a column and not a note. -->
            <span class="scan__date">
              {{ gap(action.gap_seconds) }}
              <small class="muted">{{ action.method.toLowerCase() }}</small>
            </span>
          </div>
        </div>

        <ul v-if="skipGroups.length" class="skipped">
          <li v-for="group in skipGroups" :key="group.reason">
            <span class="mark" aria-hidden="true">✕</span>
            <template v-if="group.files.length === 1">{{ group.files[0] }}</template>
            <strong v-else>{{ group.files.length }} files</strong>
            — {{ group.reason }}
            <details v-if="group.files.length > 1">
              <summary>which files</summary>
              <p class="names">{{ group.files.join(', ') }}</p>
            </details>
          </li>
        </ul>
      </section>
    </template>
  </ToolPage>
</template>

<style scoped>
.row {
  display: flex;
  gap: var(--space-3);
  align-items: flex-end;
  flex-wrap: wrap;
}
.note {
  font-size: 13px;
  color: var(--accent);
}
.note[data-tone='warn'] {
  color: var(--accent-warm);
}
.suggestion {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
  font-size: 13px;
  color: var(--text-muted);
  border-left: 2px solid var(--accent-warm);
  padding-left: var(--space-3);
}

.scan {
  display: grid;
  gap: var(--space-3);
}
.scan__head {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.scan__table {
  border: var(--border-hair);
  background: var(--bg-elevated);
  /* A long scan scrolls inside its own box rather than pushing the controls
     off the screen. */
  max-height: 50vh;
  overflow-y: auto;
}
.scan__row {
  display: grid;
  grid-template-columns: minmax(90px, 1.2fr) 160px 170px 116px;
  gap: var(--space-2);
  align-items: center;
  padding: var(--space-2) var(--space-3);
  border-bottom: var(--border-hair);
  font-family: var(--font-body);
  font-size: 13px;
}
.scan__row:last-child {
  border-bottom: none;
}
.scan__row--head {
  position: sticky;
  top: 0;
  background: var(--bg-panel);
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--text-muted);
}
.scan__name {
  color: var(--text-heading);
  overflow-wrap: anywhere;
}
.scan__date {
  display: grid;
  font-variant-numeric: tabular-nums;
}
.scan__date small {
  font-size: 11px;
}
.scan__state {
  display: inline-flex;
  align-items: baseline;
  gap: var(--space-1);
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.mark {
  font-weight: 700;
}
.scan__row[data-status='Ok'] .scan__state {
  color: var(--accent);
}
.scan__row[data-status='NoLocation'] .scan__state {
  color: var(--text-muted);
}
.scan__row[data-status='NoDate'] .scan__state,
.scan__row[data-status='NoDateOrLocation'] .scan__state {
  color: var(--accent-warm);
}
.scan__row[data-status='NotSupported'] .scan__state {
  color: var(--text-muted);
}

.skipped {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  font-size: 12px;
  color: var(--text-muted);
}
.skipped .mark {
  color: var(--danger);
}
.skipped strong {
  color: var(--text-heading);
}
.skipped summary {
  cursor: pointer;
  font-family: var(--font-label);
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.skipped .names {
  padding-top: var(--space-1);
  overflow-wrap: anywhere;
}

/* On a phone the coordinate is what falls away last; the verdict never does. */
@media (max-width: 560px) {
  .scan__row {
    grid-template-columns: minmax(80px, 1fr) 132px 104px;
  }
  .scan__row > :nth-child(3) {
    display: none;
  }
  .scan__row--plan {
    grid-template-columns: minmax(80px, 1fr) 120px 92px;
  }
  .scan__row--plan > :nth-child(3) {
    display: none;
  }
}
</style>
