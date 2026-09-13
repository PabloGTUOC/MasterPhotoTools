<script setup lang="ts">
/**
 * The timeline: where you have been, and where you say you were.
 *
 * **Beyond the specification**, like the rest of geotagging; the reasoning is in
 * `docs/timeline-plan.md`. Shared rather than web-only because both transports
 * hold a timeline of their own — the Mac's and the NAS's — and a map of one is
 * as useful as a map of the other.
 *
 * The blank days are the point. A roll of film from 2013 has nothing to match
 * against, and the strip is what shows which days those are; the pin is what
 * fills one.
 *
 * **A pin is not a fix.** It goes in labelled `placed`, by the same road a
 * `.gpx` goes in by, so it meets the same conflict rule — an instant already
 * held is put to the person, never overwritten by whoever clicked last.
 */
import { computed, onMounted, ref, useTemplateRef } from 'vue';
import type {
  PlacedStop,
  SourcedPoint,
  TrackImportPreview,
  TrackImportResult,
} from '@phototools/shared';
import { api } from '@host/api';
import ToolPage from '../components/ToolPage.vue';
import MapView from '../components/MapView.vue';
import PathField from '../components/PathField.vue';
import { useRoots } from '../useRoots';

const page = useTemplateRef<InstanceType<typeof ToolPage>>('page');

const { roots, failure: rootsError } = useRoots();
const list = (path: string) => api.list(path);

const busy = ref(false);

/**
 * Minutes east of UTC. The Mac's own offset to begin with, because that is
 * right far more often than UTC is, and wrong in a way somebody can see.
 *
 * `getTimezoneOffset` is minutes *west*, which is the opposite sign of every
 * other offset anybody writes down.
 */
const offsetMinutes = ref(-new Date().getTimezoneOffset());

/** The day on the map, as `YYYY-MM-DD` in the offset above. */
const day = ref(new Date().toISOString().slice(0, 10));

const points = ref<SourcedPoint[]>([]);
const coverage = ref<Map<string, number>>(new Map());
const extent = ref<[number, number] | null>(null);

// The pin, and what is claimed about it.
const placeName = ref('');
const lat = ref<string>('');
const lon = ref<string>('');
const fromTime = ref('12:00');
const toTime = ref('18:00');

const pending = ref<TrackImportPreview | null>(null);
const stored = ref<TrackImportResult | null>(null);
const exportPath = ref('');

/** `YYYY-MM-DD` plus `HH:MM` in the chosen offset, as Unix seconds UTC. */
function instant(date: string, time: string): number | null {
  const parsed = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  const clock = /^(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(time.trim());
  if (!parsed || !clock) return null;
  const utc = Date.UTC(
    Number(parsed[1]),
    Number(parsed[2]) - 1,
    Number(parsed[3]),
    Number(clock[1]),
    Number(clock[2]),
    Number(clock[3] ?? 0),
  );
  return Math.floor(utc / 1000) - offsetMinutes.value * 60;
}

const dayStart = computed(() => instant(day.value, '00:00') ?? 0);
const dayEnd = computed(() => dayStart.value + 86_399);

/** The UTC the times on screen actually mean, shown so the offset is visible. */
const asUtc = computed(() => {
  const from = instant(day.value, fromTime.value);
  const to = instant(day.value, toTime.value);
  if (from === null || to === null) return null;
  const stamp = (at: number) => new Date(at * 1000).toISOString().slice(0, 19).replace('T', ' ');
  return from === to ? `${stamp(from)}Z` : `${stamp(from)}Z → ${stamp(to)}Z`;
});

const offsetLabel = computed(() => {
  const minutes = offsetMinutes.value;
  const sign = minutes < 0 ? '-' : '+';
  const absolute = Math.abs(minutes);
  return `UTC${sign}${String(Math.floor(absolute / 60)).padStart(2, '0')}:${String(absolute % 60).padStart(2, '0')}`;
});

/** The days of the month on screen, with what the library holds for each. */
const month = computed(() => {
  const [year, monthNumber] = day.value.split('-').map(Number);
  if (!year || !monthNumber) return [];
  const days = new Date(Date.UTC(year, monthNumber, 0)).getUTCDate();
  return Array.from({ length: days }, (_, index) => {
    const date = `${day.value.slice(0, 8)}${String(index + 1).padStart(2, '0')}`;
    return { date, fixes: coverage.value.get(date) ?? 0 };
  });
});

const selected = computed(() => {
  const latitude = Number(lat.value);
  const longitude = Number(lon.value);
  if (lat.value === '' || lon.value === '' || Number.isNaN(latitude) || Number.isNaN(longitude)) {
    return null;
  }
  return { lat: latitude, lon: longitude };
});

/** A day key in the chosen offset, for the coverage map. */
function dayKey(at: number): string {
  return new Date((at + offsetMinutes.value * 60) * 1000).toISOString().slice(0, 10);
}

async function load() {
  busy.value = true;
  page.value?.setFailure(null);
  try {
    const [year, monthNumber] = day.value.split('-').map(Number);
    const monthFrom = Math.floor(Date.UTC(year, monthNumber - 1, 1) / 1000) - offsetMinutes.value * 60;
    const monthTo = Math.floor(Date.UTC(year, monthNumber, 1) / 1000) - offsetMinutes.value * 60 - 1;

    // Two calls, and deliberately: the strip wants a month of counts and the
    // map wants a day of fixes. One call for both would carry a month of
    // positions across to draw thirty dots.
    const [days, today] = await Promise.all([
      api.timeline({
        from: monthFrom,
        to: monthTo,
        offset_minutes: offsetMinutes.value,
        include_points: false,
      }),
      api.timeline({ from: dayStart.value, to: dayEnd.value, offset_minutes: offsetMinutes.value }),
    ]);

    coverage.value = new Map(days.coverage.map((entry) => [dayKey(entry.day), entry.fixes]));
    points.value = today.points;
    extent.value = today.extent ?? days.extent;
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

/** Open on a day the library actually holds something for. */
onMounted(async () => {
  try {
    const probe = await api.timeline({ from: 0, to: 0, offset_minutes: offsetMinutes.value, include_points: false });
    if (probe.extent) day.value = dayKey(probe.extent[1]);
  } catch {
    // Not fatal, and not worth a message: the day stays today and `load` will
    // report anything genuinely wrong.
  }
  await load();
});

function pickPosition(position: { lat: number; lon: number }) {
  lat.value = position.lat.toFixed(6);
  lon.value = position.lon.toFixed(6);
}

function stops(): PlacedStop[] | null {
  const position = selected.value;
  const from = instant(day.value, fromTime.value);
  const to = instant(day.value, toTime.value);
  if (!position || from === null || to === null) return null;
  return [
    {
      name: placeName.value.trim() || 'Placed by hand',
      lat: position.lat,
      lon: position.lon,
      from,
      to,
    },
  ];
}

/** Why the pin cannot be stored as it stands, or `null` when it can. */
function problem(): string | null {
  if (!selected.value) return 'Click the map, or type a latitude and longitude.';
  if (instant(day.value, fromTime.value) === null) return 'The from-time reads as HH:MM.';
  if (instant(day.value, toTime.value) === null) return 'The to-time reads as HH:MM.';
  if (instant(day.value, toTime.value)! < instant(day.value, fromTime.value)!) {
    return 'The span ends before it starts.';
  }
  return null;
}

async function preview() {
  const trouble = problem();
  if (trouble) {
    page.value?.setFailure(trouble);
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  stored.value = null;
  try {
    pending.value = await api.previewPlacedPoints(stops()!);
    page.value?.setReviewed(true);
  } catch (e) {
    pending.value = null;
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

async function apply() {
  const trouble = problem();
  if (trouble) {
    page.value?.setFailure(trouble);
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    stored.value = await api.placePoints({
      stops: stops()!,
      // The library keeps what it already holds unless somebody says otherwise:
      // an existing fix was recorded or decided on, and this pin is neither.
      resolution: 'KeepExisting',
      overrides: [],
    });
    pending.value = null;
    await load();
  } catch (e) {
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

async function exportDay() {
  if (!exportPath.value.trim()) {
    page.value?.setFailure('Name a file to write the day to.');
    return;
  }
  busy.value = true;
  page.value?.setFailure(null);
  try {
    const written = await api.exportTimeline({
      from: dayStart.value,
      to: dayEnd.value,
      path: exportPath.value.trim(),
      name: day.value,
    });
    page.value?.setFailure(null);
    stored.value = null;
    exported.value = `${written.points} fixes written to ${written.path}`;
  } catch (e) {
    exported.value = null;
    page.value?.setFailure(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = false;
  }
}

const exported = ref<string | null>(null);

/** A fix's time, in the offset on screen rather than in UTC. */
function localTime(at: number): string {
  return new Date((at + offsetMinutes.value * 60) * 1000).toISOString().slice(11, 19);
}

function sourceWord(source: SourcedPoint['source']): string {
  if (source === 'placed') return 'placed by hand';
  return source === 'inferred' ? 'inferred' : 'recorded';
}
</script>

<template>
  <ToolPage
    ref="page"
    title="Timeline"
    blurb="Where you have been, and where you say you were. Place a point on a day nothing recorded, and the Geotag tab can match photographs to it."
    has-preview
    apply-label="Store this point"
    :busy="busy"
    @preview="preview"
    @apply="apply"
  >
    <template #form>
      <div class="row">
        <label class="field">
          <span>Day</span>
          <input v-model="day" type="date" @change="load" />
        </label>

        <label class="field">
          <span>UTC offset, minutes east</span>
          <input v-model.number="offsetMinutes" type="number" step="15" @change="load" />
          <small class="muted">{{ offsetLabel }} — the same offset belongs in Geotag.</small>
        </label>
      </div>

      <section class="strip" aria-label="Days holding fixes this month">
        <button
          v-for="entry in month"
          :key="entry.date"
          type="button"
          class="strip__day"
          :class="{ 'strip__day--has': entry.fixes > 0, 'strip__day--on': entry.date === day }"
          :title="`${entry.date} — ${entry.fixes} fixes`"
          @click="
            day = entry.date;
            load();
          "
        >
          {{ entry.date.slice(8) }}
        </button>
      </section>

      <MapView :points="points" :selected="selected" @update:position="pickPosition" />

      <p v-if="!points.length" class="note" role="status">
        Nothing recorded this day. Click the map to say where you were.
      </p>

      <div class="row">
        <label class="field">
          <span>Place</span>
          <input v-model="placeName" type="text" placeholder="Alexanderplatz" />
        </label>
        <label class="field">
          <span>Latitude</span>
          <input v-model="lat" type="text" inputmode="decimal" placeholder="52.521918" />
        </label>
        <label class="field">
          <span>Longitude</span>
          <input v-model="lon" type="text" inputmode="decimal" placeholder="13.413215" />
        </label>
      </div>

      <div class="row">
        <label class="field">
          <span>From</span>
          <input v-model="fromTime" type="text" placeholder="14:00" />
        </label>
        <label class="field">
          <span>To</span>
          <input v-model="toTime" type="text" placeholder="18:00" />
          <small class="muted">The same time twice is one instant.</small>
        </label>
      </div>

      <p v-if="asUtc" class="note" role="status">
        {{ offsetLabel }} — stored as {{ asUtc }}. A span is its two ends; nothing between them is
        invented.
      </p>

      <details class="export">
        <summary>Export this day as a .gpx</summary>
        <PathField
          v-model="exportPath"
          label="Write to"
          placeholder="/library/tracks/2013-05-01.gpx"
          :roots="roots"
          :roots-error="rootsError"
          :list="list"
          keep-file-name
          :file-name-fallback="`${day}.gpx`"
        />
        <button type="button" class="secondary" :disabled="busy" @click="exportDay">
          Write the file
        </button>
        <p v-if="exported" class="note" role="status">{{ exported }}</p>
      </details>
    </template>

    <template #preview>
      <section v-if="pending" class="review" aria-live="polite">
        <h2>{{ pending.new_points }} new, {{ pending.identical_points }} already held</h2>
        <p v-if="pending.conflicts.length" class="error" role="alert">
          {{ pending.conflicts.length }} of these instants already hold a different position. Storing
          keeps what the library has — a recorded fix outranks a pin.
        </p>
        <p v-else-if="pending.already_imported_at" class="muted">
          This exact placement is already in the library.
        </p>
      </section>

      <p v-if="stored" class="note" role="status">
        Stored: {{ stored.added }} points added, {{ stored.identical }} already held,
        {{ stored.kept_existing }} left as they were.
      </p>

      <section v-if="points.length" class="fixes">
        <h2>{{ points.length }} fixes on {{ day }}</h2>
        <ol class="fixes__list">
          <li v-for="point in points" :key="`${point.at}-${point.track_id}`">
            <span class="fixes__time">{{ localTime(point.at) }}</span>
            <span class="fixes__where">{{ point.lat.toFixed(5) }}, {{ point.lon.toFixed(5) }}</span>
            <span class="badge" :data-source="point.source">{{ sourceWord(point.source) }}</span>
            <span class="fixes__track">{{ point.track_name }}</span>
          </li>
        </ol>
      </section>
    </template>
  </ToolPage>
</template>

<style scoped>
.row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  align-items: flex-start;
}

.row > .field {
  flex: 1 1 180px;
}

/* One cell per day of the month. It scrolls rather than wraps into a grid a
   thumb has to aim at: the strip is read left to right, like the month is. */
.strip {
  display: flex;
  gap: 2px;
  overflow-x: auto;
  padding-bottom: var(--space-1);
  scrollbar-width: none;
}
.strip::-webkit-scrollbar {
  display: none;
}

.strip__day {
  flex: 0 0 auto;
  min-width: 40px;
  min-height: 40px;
  font-family: var(--font-body);
  font-size: 12px;
  color: var(--text-disabled);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  cursor: pointer;
}

/* A day the library holds something for. The whole point of the strip is which
   ones are not lit. */
.strip__day--has {
  color: var(--accent);
  border-color: var(--accent);
}

.strip__day--on {
  background: var(--bg-elevated);
  border-color: var(--accent-warm);
  color: var(--accent-warm);
}

.export {
  border: 1px solid var(--border);
  padding: var(--space-3);
  display: grid;
  gap: var(--space-3);
}
.export summary {
  font-family: var(--font-label);
  letter-spacing: 0.08em;
  cursor: pointer;
  min-height: 24px;
}

.fixes__list {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  font-family: var(--font-body);
  font-size: 13px;
}
.fixes__list li {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  align-items: baseline;
  border-bottom: 1px solid var(--border);
  padding: var(--space-1) 0;
}
.fixes__time {
  color: var(--accent);
}
.fixes__where {
  color: var(--text);
}
.fixes__track {
  color: var(--text-disabled);
}

/* No glow: this is chip-sized type, where a glow smears the glyph that carries
   the meaning. */
.badge[data-source='placed'] {
  color: var(--accent-warm);
  border-color: var(--accent-warm);
}
.badge[data-source='inferred'] {
  color: var(--text-muted);
  border-color: var(--text-muted);
}
</style>
