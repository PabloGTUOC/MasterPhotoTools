<script setup lang="ts">
/**
 * The GPS track library: what has been imported, and what importing a new file
 * would do.
 *
 * Transport-free, like everything in `components/`: the tracks, the preview and
 * the busy flag arrive as props, and every decision leaves as an event. What
 * makes that worth doing here is the conflict table — the one part of this
 * feature a person has to *judge*, and the part worth being able to put in
 * front of a harness with no application around it.
 */
import { ref, watch } from 'vue';
import type {
  BrowserEntry,
  Decision,
  PointConflict,
  Resolution,
  TrackImportPreview,
  TrackSummary,
} from '@phototools/shared';
import PathField from './PathField.vue';

const props = defineProps<{
  tracks: TrackSummary[];
  /** The pending import, once previewed. Null before, and after committing. */
  preview: TrackImportPreview | null;
  path: string;
  roots: string[];
  list: (path: string) => Promise<BrowserEntry[]>;
  busy?: boolean;
}>();

const emit = defineEmits<{
  'update:path': [value: string];
  preview: [];
  cancel: [];
  import: [resolution: Resolution, overrides: Decision[]];
  remove: [id: string];
}>();

/** The default for every disagreement this import turns up. */
const resolution = ref<Resolution>('KeepExisting');

/**
 * The instants decided against that default, by timestamp.
 *
 * Keyed by instant rather than by index: the commit recomputes the diff, so a
 * decision has to name the moment it is about, not a row in a list that may no
 * longer be the same list.
 */
const overrides = ref<Record<number, 'Existing' | 'New'>>({});

// A new preview is a new set of disagreements; carrying decisions across would
// apply an answer to a question nobody was asked.
watch(
  () => props.preview,
  () => {
    overrides.value = {};
    resolution.value = 'KeepExisting';
  },
);

function decide(conflict: PointConflict, take: 'Existing' | 'New') {
  overrides.value = { ...overrides.value, [conflict.at]: take };
}

/** What this conflict will do, whether decided individually or by the default. */
function choice(conflict: PointConflict): 'Existing' | 'New' {
  return overrides.value[conflict.at] ?? (resolution.value === 'TakeNew' ? 'New' : 'Existing');
}

function commit() {
  const decisions: Decision[] = Object.entries(overrides.value).map(([at, take]) => ({
    at: Number(at),
    take,
  }));
  emit('import', resolution.value, decisions);
}

/** A Unix instant as UTC, which is the only zone a fix is in. */
function utc(seconds: number | null): string {
  if (seconds === null) return '—';
  return new Date(seconds * 1000).toISOString().replace('T', ' ').slice(0, 19);
}

function day(seconds: number | null): string {
  if (seconds === null) return '—';
  return new Date(seconds * 1000).toISOString().slice(0, 10);
}

function coordinate(point: { lat: number; lon: number }): string {
  return `${point.lat.toFixed(6)}, ${point.lon.toFixed(6)}`;
}

/** A distance, at the precision it is worth reading at. */
function metres(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)} km` : `${value.toFixed(0)} m`;
}
</script>

<template>
  <section class="library">
    <h2 class="library__head">// TRACK LIBRARY // {{ props.tracks.length }} IMPORTED</h2>

    <div v-if="props.tracks.length" class="rows">
      <div class="row row--head" role="row">
        <span role="columnheader">File</span>
        <span role="columnheader">Covers</span>
        <span role="columnheader">Fixes</span>
        <span role="columnheader"></span>
      </div>
      <div v-for="track in props.tracks" :key="track.id" class="row" role="row">
        <span class="row__name" :title="track.source_path">
          {{ track.name }}
          <small v-if="track.creator" class="muted">{{ track.creator }}</small>
        </span>
        <span class="row__span">
          {{ day(track.first_fix) }}
          <small class="muted">to {{ day(track.last_fix) }}</small>
        </span>
        <span class="row__count">
          {{ track.points_added }}
          <small v-if="track.points_identical" class="muted">
            {{ track.points_identical }} already held
          </small>
        </span>
        <span class="row__act">
          <button type="button" class="ghost" :disabled="props.busy" @click="emit('remove', track.id)">
            Forget
          </button>
        </span>
      </div>
    </div>

    <p v-else class="muted">
      No tracks yet. Load the <code>.gpx</code> your phone exported and every photograph taken
      while it was recording can be placed.
    </p>

    <PathField
      :model-value="props.path"
      label="Track file (.gpx)"
      placeholder="/mnt/photos/tracks/track.gpx"
      keep-file-name
      file-name-fallback="track.gpx"
      choose-label="Use this folder"
      :selectable="['gpx']"
      :roots="props.roots"
      :list="props.list"
      @update:model-value="emit('update:path', $event)"
    />

    <div class="actions">
      <button type="button" class="secondary" :disabled="props.busy" @click="emit('preview')">
        Read the track
      </button>
    </div>

    <!-- The pending import. Nothing is stored until this is answered. -->
    <div v-if="props.preview" class="pending">
      <h3 class="library__head">
        // {{ props.preview.name }} // {{ props.preview.point_count }} FIXES
      </h3>

      <p v-if="props.preview.already_imported_at !== null" class="muted">
        This exact file was imported on {{ day(props.preview.already_imported_at) }}. Importing it
        again adds nothing it did not add then.
      </p>

      <ul class="counts">
        <li><strong>{{ props.preview.new_points }}</strong> new</li>
        <li><strong>{{ props.preview.identical_points }}</strong> already held</li>
        <li :data-warn="props.preview.conflicts.length > 0">
          <strong>{{ props.preview.conflicts.length }}</strong> disagree
        </li>
      </ul>

      <details v-if="props.preview.sample.length" class="sample">
        <summary>What will be written</summary>
        <ul>
          <li v-for="(point, i) in props.preview.sample" :key="i">
            {{ point.latitude }} {{ point.latitude_ref }} {{ point.longitude }}
            {{ point.longitude_ref }}
            <span class="muted">
              · {{ point.date_stamp }} {{ point.time_stamp }} UTC<template
                v-if="point.altitude"
              >
                · {{ point.altitude }} m</template
              >
            </span>
          </li>
        </ul>
      </details>

      <details v-if="props.preview.rejected.length" class="sample">
        <summary>{{ props.preview.rejected.length }} point(s) this file cannot offer</summary>
        <ul>
          <li v-for="rejected in props.preview.rejected" :key="rejected.index" class="muted">
            <template v-if="rejected.index">point {{ rejected.index }} — </template>
            {{ rejected.reason }}
          </li>
        </ul>
      </details>

      <!-- The disagreements. Every fix came from one phone, so these are a
           fault rather than a tie to break, and they are put to a person. -->
      <section v-if="props.preview.conflicts.length" class="conflicts">
        <h4 class="conflicts__head">
          // {{ props.preview.conflicts.length }} INSTANT(S) THIS FILE DISAGREES ABOUT
        </h4>
        <p class="muted">
          Every fix comes from one phone, so this should not happen. The distance says which fault
          it is: a few metres is two apps disagreeing about one reading; a kilometre is a different
          device, or an export with the wrong offset.
        </p>

        <div class="row row--conflict row--head" role="row">
          <span role="columnheader">Instant (UTC)</span>
          <span role="columnheader">In the library</span>
          <span role="columnheader">In this file</span>
          <span role="columnheader">Apart</span>
          <span role="columnheader">Keep</span>
        </div>
        <div
          v-for="conflict in props.preview.conflicts"
          :key="conflict.at"
          class="row row--conflict"
          role="row"
        >
          <span class="row__span">{{ utc(conflict.at) }}</span>
          <span class="row__coord">
            {{ coordinate(conflict.existing) }}
            <small class="muted">{{ conflict.existing_track_name }}</small>
          </span>
          <span class="row__coord">{{ coordinate(conflict.incoming) }}</span>
          <span class="row__count">{{ metres(conflict.metres) }}</span>
          <span class="row__act">
            <button
              type="button"
              :class="choice(conflict) === 'Existing' ? 'primary' : 'ghost'"
              @click="decide(conflict, 'Existing')"
            >
              Library
            </button>
            <button
              type="button"
              :class="choice(conflict) === 'New' ? 'primary' : 'ghost'"
              @click="decide(conflict, 'New')"
            >
              File
            </button>
          </span>
        </div>

        <fieldset class="field">
          <legend>Everything not decided above</legend>
          <label class="radio">
            <input v-model="resolution" type="radio" value="KeepExisting" />
            Keep what the library holds
          </label>
          <label class="radio">
            <input v-model="resolution" type="radio" value="TakeNew" />
            Take what this file says
          </label>
        </fieldset>
      </section>

      <div class="actions">
        <button type="button" class="primary" :disabled="props.busy" @click="commit">
          Import into the library
        </button>
        <button type="button" class="ghost" :disabled="props.busy" @click="emit('cancel')">
          Cancel
        </button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.library {
  display: grid;
  gap: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
  padding: var(--space-3);
}
.library__head,
.conflicts__head {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.conflicts__head {
  color: var(--accent-warm);
}

.rows {
  border: var(--border-hair);
  background: var(--bg-elevated);
  max-height: 40vh;
  overflow-y: auto;
}
.row {
  display: grid;
  grid-template-columns: minmax(90px, 1.4fr) 160px 110px 88px;
  gap: var(--space-2);
  align-items: center;
  padding: var(--space-2) var(--space-3);
  border-bottom: var(--border-hair);
  font-family: var(--font-body);
  font-size: 13px;
}
.row:last-child {
  border-bottom: none;
}
.row--head {
  position: sticky;
  top: 0;
  background: var(--bg-panel);
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--text-muted);
}
.row--conflict {
  grid-template-columns: minmax(120px, 1fr) minmax(120px, 1fr) minmax(120px, 1fr) 80px 150px;
}
.row__name {
  color: var(--text-heading);
  overflow-wrap: anywhere;
  display: grid;
}
.row__span,
.row__count,
.row__coord {
  display: grid;
  font-variant-numeric: tabular-nums;
}
.row__name small,
.row__span small,
.row__count small,
.row__coord small {
  font-size: 11px;
}
.row__act {
  display: flex;
  gap: var(--space-1);
  flex-wrap: wrap;
}

.pending {
  display: grid;
  gap: var(--space-3);
  border-top: var(--border-hair);
  padding-top: var(--space-3);
}
.counts {
  list-style: none;
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-4);
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
}
.counts strong {
  color: var(--text-heading);
  font-size: 15px;
}
/* A disagreement is the one count worth a colour, and it keeps the number in
   front of the word so it reads at a glance. */
.counts li[data-warn='true'] strong {
  color: var(--accent-warm);
}

.sample {
  font-size: 12px;
  color: var(--text-muted);
}
.sample summary {
  cursor: pointer;
  font-family: var(--font-label);
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.sample ul {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  padding-top: var(--space-2);
  font-variant-numeric: tabular-nums;
}

.conflicts {
  display: grid;
  gap: var(--space-2);
  border: var(--border-hair);
  padding: var(--space-3);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
}

/* On a phone the two coordinates matter more than which file they came from,
   and the decision never falls away. */
@media (max-width: 560px) {
  .row {
    grid-template-columns: minmax(80px, 1fr) 120px 84px;
  }
  .row > :nth-child(4) {
    grid-column: 1 / -1;
  }
  .row--conflict {
    grid-template-columns: minmax(80px, 1fr) 96px;
  }
  .row--conflict > :nth-child(3),
  .row--conflict > :nth-child(4) {
    grid-column: 1 / -1;
  }
  .row--conflict > :nth-child(5) {
    grid-column: 1 / -1;
  }
}
</style>
