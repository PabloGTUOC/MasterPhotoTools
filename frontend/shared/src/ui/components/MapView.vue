<script setup lang="ts">
/**
 * A map: fixes drawn on it, and a position taken off it.
 *
 * Transport-free, as everything in `components/` is. Points in, a clicked
 * position out — so it can be mounted in a harness with no application around
 * it, and neither front end's transport appears anywhere in this file.
 *
 * **Leaflet is bundled, not loaded from a CDN.** The reason the fonts are
 * self-hosted is the reason this is: the desktop application is offline exactly
 * when somebody is travelling with a card reader, and a CDN that fails takes
 * the map with it silently.
 *
 * **Tiles are the one thing here that needs a network**, and their absence is a
 * state rather than an error: a grey map, a notice, and every coordinate field
 * on the screen still works. Nothing else in this application talks to a third
 * party, which is why the tile host is named on the screen rather than only in
 * the source.
 */
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import type { SourcedPoint } from '@phototools/shared';
import L from 'leaflet';
import 'leaflet/dist/leaflet.css';

const props = withDefaults(
  defineProps<{
    /** The fixes to draw, in time order. */
    points?: SourcedPoint[];
    /** The pin being placed, if one is. */
    selected?: { lat: number; lon: number } | null;
    /** Where to open when there is nothing to show. Berlin, for no better reason. */
    fallback?: { lat: number; lon: number; zoom: number };
    /**
     * The tile source. `VITE_TILE_URL` overrides it for anyone running their
     * own; the default is OpenStreetMap's, whose usage policy a person clicking
     * a map is comfortably inside.
     */
    tileUrl?: string;
    /** Off for a map that only shows. */
    clickable?: boolean;
  }>(),
  {
    points: () => [],
    selected: null,
    fallback: () => ({ lat: 52.520008, lon: 13.404954, zoom: 12 }),
    tileUrl: '',
    clickable: true,
  },
);

const emit = defineEmits<{ (e: 'update:position', value: { lat: number; lon: number }): void }>();

const host = ref<HTMLDivElement | null>(null);
/** Leaflet's objects are not reactive data; wrapping them in `ref` would make
 * Vue walk an entire map instance on every change. */
const map = shallowRef<L.Map | null>(null);
const drawn = shallowRef<L.LayerGroup | null>(null);
const pin = shallowRef<L.Marker | null>(null);
const tilesFailed = ref(false);

const TILE_URL =
  props.tileUrl ||
  (import.meta as unknown as { env?: Record<string, string> }).env?.VITE_TILE_URL ||
  'https://tile.openstreetmap.org/{z}/{x}/{y}.png';

/** A marker drawn in CSS rather than from an image file.
 *
 * Leaflet's default marker is a PNG resolved relative to its stylesheet, which
 * breaks in a bundle often enough to be a known nuisance — and a bracket drawn
 * in the application's own colours belongs here more than a blue teardrop does.
 */
function bracket(className: string): L.DivIcon {
  return L.divIcon({ className, html: '<i></i>', iconSize: [16, 16], iconAnchor: [8, 8] });
}

function draw() {
  const instance = map.value;
  if (!instance) return;

  drawn.value?.remove();
  const layer = L.layerGroup().addTo(instance);
  drawn.value = layer;

  const path: [number, number][] = [];
  for (const point of props.points) {
    path.push([point.lat, point.lon]);
    L.marker([point.lat, point.lon], { icon: bracket(`fix fix--${point.source}`) })
      .bindTooltip(
        `${new Date(point.at * 1000).toISOString().slice(11, 19)} UTC · ${label(point.source)}<br>${point.track_name}`,
      )
      .addTo(layer);
  }

  // Dashed, and said so in the legend: the line is drawn between fixes, not
  // travelled. Nothing here knows the route somebody took between two points
  // and this application does not invent one.
  if (path.length > 1) {
    L.polyline(path, { color: '#00ff94', weight: 1, opacity: 0.5, dashArray: '4 6' }).addTo(layer);
  }

  if (path.length) instance.fitBounds(L.latLngBounds(path), { padding: [24, 24], maxZoom: 16 });
}

function label(source: SourcedPoint['source']): string {
  if (source === 'placed') return 'placed by hand';
  return source === 'inferred' ? 'inferred' : 'recorded';
}

function drawPin() {
  const instance = map.value;
  if (!instance) return;
  pin.value?.remove();
  pin.value = null;
  if (!props.selected) return;
  pin.value = L.marker([props.selected.lat, props.selected.lon], {
    icon: bracket('fix fix--pin'),
    draggable: props.clickable,
  }).addTo(instance);
  pin.value.on('dragend', () => {
    const at = pin.value?.getLatLng();
    if (at) emit('update:position', { lat: at.lat, lon: at.lng });
  });
}

onMounted(() => {
  if (!host.value) return;
  // Leaflet's own attribution is a 13px link in a corner, which is below the
  // 40px every control on this screen has to clear. The credit is not optional
  // — OpenStreetMap asks for it and it is theirs — so it is rendered below the
  // map instead, at a size somebody can actually tap.
  const instance = L.map(host.value, { attributionControl: false }).setView(
    [props.fallback.lat, props.fallback.lon],
    props.fallback.zoom,
  );
  const tiles = L.tileLayer(TILE_URL, { maxZoom: 19 });
  tiles.on('tileerror', () => {
    tilesFailed.value = true;
  });
  tiles.addTo(instance);

  instance.on('click', (event: L.LeafletMouseEvent) => {
    if (!props.clickable) return;
    emit('update:position', { lat: event.latlng.lat, lon: event.latlng.lng });
  });

  map.value = instance;
  draw();
  drawPin();
});

onBeforeUnmount(() => {
  map.value?.remove();
  map.value = null;
});

watch(() => props.points, draw, { deep: false });
watch(() => props.selected, drawPin, { deep: true });
</script>

<template>
  <div class="map">
    <div ref="host" class="map__canvas" data-testid="map-canvas"></div>

    <p v-if="tilesFailed" class="map__offline" role="status">
      No map tiles — offline, or the tile server is unreachable. The coordinate fields still work.
    </p>

    <p class="map__credit">
      Tiles from
      <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noreferrer noopener">
        © OpenStreetMap contributors
      </a>
    </p>

    <p class="map__legend">
      <span class="key key--recorded"></span> recorded
      <span class="key key--placed"></span> placed by hand
      <span class="key key--inferred"></span> inferred
      <span class="map__caveat">— the line is drawn between fixes, not travelled</span>
    </p>
  </div>
</template>

<style scoped>
.map {
  display: grid;
  gap: var(--space-2);
}

/* A fixed height rather than an aspect ratio: on a phone in landscape an
   aspect-ratio map is taller than the viewport and the controls below it become
   unreachable, which check:layout would pass and a thumb would not. */
.map__canvas {
  height: 320px;
  max-width: 100%;
  border: 1px solid var(--border-strong);
  background: var(--bg-panel);
}

@media (max-width: 480px) {
  .map__canvas {
    height: 240px;
  }
}

.map__offline,
.map__credit,
.map__legend {
  font-family: var(--font-body);
  font-size: 12px;
  color: var(--text-muted);
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}

.map__offline {
  color: var(--accent-warm);
}

.map__caveat {
  color: var(--text-disabled);
}

/* 40px because it is a link on a screen, and every control here clears 40px. */
.map__credit a {
  display: inline-flex;
  align-items: center;
  min-height: 40px;
  color: var(--text-muted);
}

.key {
  width: 10px;
  height: 10px;
  border: 1px solid currentColor;
}
.key--recorded {
  color: var(--accent);
}
.key--placed {
  color: var(--accent-warm);
}
.key--inferred {
  color: var(--text-muted);
}
</style>

<style>
/* Unscoped: Leaflet builds these elements itself, outside this component's
   scope attribute, so a scoped rule would never reach them. */
.leaflet-container {
  background: var(--bg-panel);
  font-family: var(--font-body);
}

/* A dark map with no second tile provider and no key. Behind one class so it
   can be turned off by anyone who would rather read a normal map. */
.leaflet-tile-pane {
  filter: invert(1) hue-rotate(180deg) saturate(0.6) brightness(0.95);
}

/* Leaflet draws its zoom buttons at 30px; on this screen a control clears 40px. */
.leaflet-touch .leaflet-bar a,
.leaflet-bar a {
  width: 40px;
  height: 40px;
  line-height: 40px;
  background: var(--bg-elevated);
  color: var(--text);
  border-bottom-color: var(--border);
  border-radius: 0;
}
.leaflet-bar a:hover {
  background: var(--bg-panel);
  color: var(--accent);
}
.leaflet-touch .leaflet-bar {
  border: 1px solid var(--border-strong);
  border-radius: 0;
}

.fix i {
  display: block;
  width: 10px;
  height: 10px;
  margin: 3px;
  border: 1px solid var(--accent);
}
.fix--placed i {
  border-color: var(--accent-warm);
}
.fix--inferred i {
  border-color: var(--text-muted);
}
.fix--pin i {
  width: 14px;
  height: 14px;
  margin: 1px;
  border: 2px solid var(--accent-warm);
  box-shadow: var(--glow-amber);
}
</style>
