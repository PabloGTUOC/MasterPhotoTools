<script setup lang="ts">
/**
 * Develop view (RP-1): RAW folder development with built-in film simulations.
 *
 * - Folder: PathField, lists RAWs. Empty folder explains itself.
 * - Photographs: thumbnail grid with name and assigned look.
 *   Thumbnails load sequentially (one at a time, zero overlap).
 * - Selection: Click selects one, Cmd-click toggles, Shift-click selects range, Select all.
 * - Look: Picker grouped None (neutral), Fujifilm (5), Ricoh (7), My LUTs.
 *   Choosing a look assigns it to the current selection only.
 * - Exposure: AdjustmentSlider (-4 to +4 EV) applied to the selection.
 * - Preview: Shows most recently clicked photograph at its assigned look and exposure.
 *   Latest-wins rendering discipline (stale responses discarded).
 * - Output: JPEG (quality 1-100, default 95) or 16-bit ProPhoto RGB TIFF into <folder>/Developed.
 * - Run: Dry-run lock discipline. Run disabled until reviewed dry run exists for exact settings.
 *   Any change invalidates the dry run. Skipped items listed with reasons. JobProgress with cancel.
 * - State persistence: Module-level state preserves folder, assignments, and selection across tab switches.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { BrowserEntry } from '@phototools/shared';
import type {
  BuiltinLookInfo,
  DevelopItem,
  DevelopPlan,
  LutLibraryList,
  OutputFormat,
} from '@host/api';
import { desktop } from '@host/api';
import AdjustmentSlider from '../components/AdjustmentSlider.vue';
import JobProgress from '@ui/components/JobProgress.vue';
import PathField from '@ui/components/PathField.vue';
import { useRoots } from '@ui/useRoots';

// ---------------------------------------------------------------------------
// Module-level state: survives tab switching / view unmounting (RP-1)
// ---------------------------------------------------------------------------

import {
  moduleFolder,
  moduleAssignments,
  moduleSelection,
  moduleLastClicked,
  moduleFormat,
  moduleScope,
  moduleJobId,
  moduleIsDeveloping,
} from '../developState';

// ---------------------------------------------------------------------------
// Component instance state
// ---------------------------------------------------------------------------

const { roots, failure: rootsError } = useRoots();
const listRoots = (p: string): Promise<BrowserEntry[]> => desktop.list(p);

// UI controls
const folder = ref(moduleFolder.value);
const photos = ref<string[]>([]);
const isFolderLoading = ref(false);
const folderError = ref<string | null>(null);

// Thumbnails
const thumbnailUrls = ref<Record<string, string>>({});
const thumbnailErrors = ref<Record<string, string>>({});
let thumbnailToken = 0;

// Selection
const selection = ref<string[]>([...moduleSelection.value]);
const lastClicked = ref<string | null>(moduleLastClicked.value);

// Scope: All photographs vs Selected only (RP-1 review item 5)
const scope = ref<'all' | 'selected'>(moduleScope.value);
watch(scope, (s) => {
  moduleScope.value = s;
});

// Looks library
const builtinLooks = ref<BuiltinLookInfo[]>([]);
const lutLibrary = ref<LutLibraryList>({ luts: [], errors: [] });

// Preview
const previewUrl = ref<string | null>(null);
const isPreviewLoading = ref(false);
const previewError = ref<string | null>(null);
let previewSeq = 0;

interface WantedPreviewState {
  folder: string;
  name: string;
  look: string | null;
  exposure_ev: number;
  draft: boolean;
}
let previewInFlight = false;
let pendingPreviewState: WantedPreviewState | null = null;

// Output format
const outputType = ref<'jpeg' | 'tiff'>(moduleFormat.value.kind === 'tiff' ? 'tiff' : 'jpeg');
const jpegQuality = ref(moduleFormat.value.kind === 'jpeg' ? moduleFormat.value.quality : 95);

// Dry run & review state
interface ReviewedState {
  folder: string;
  itemsJson: string;
  formatJson: string;
}
const reviewed = ref<ReviewedState | null>(null);
const reviewedPlan = ref<DevelopPlan | null>(null);
const isPlanning = ref(false);
const planError = ref<string | null>(null);

// Job execution (RP-1 review item 4)
const jobId = ref<string | null>(moduleJobId.value);
const isDeveloping = ref(moduleIsDeveloping.value);

watch(jobId, (id) => {
  moduleJobId.value = id;
});
watch(isDeveloping, (d) => {
  moduleIsDeveloping.value = d;
});

// Sync with module state and handle folder changes (RP-1 review item 2)
let lastLoadedFolder = moduleFolder.value;

watch(folder, (newFolder) => {
  const trimmed = newFolder.trim();
  if (lastLoadedFolder && trimmed !== lastLoadedFolder) {
    // Clear assignments and selection when folder changes, but not on remount
    moduleAssignments.value = {};
    moduleSelection.value = [];
    moduleLastClicked.value = null;
    selection.value = [];
    lastClicked.value = null;
    reviewed.value = null;
    reviewedPlan.value = null;
    for (const u of Object.values(thumbnailUrls.value)) {
      URL.revokeObjectURL(u);
    }
    thumbnailUrls.value = {};
    if (previewUrl.value) {
      URL.revokeObjectURL(previewUrl.value);
      previewUrl.value = null;
    }
  }
  lastLoadedFolder = trimmed;
  moduleFolder.value = newFolder;
  loadFolder();
});

watch(selection, (s) => {
  moduleSelection.value = [...s];
}, { deep: true });
watch(lastClicked, (c) => {
  moduleLastClicked.value = c;
});

// Computed format
const currentFormat = computed<OutputFormat>(() => {
  if (outputType.value === 'tiff') {
    return { kind: 'tiff' };
  }
  return { kind: 'jpeg', quality: Math.round(Number(jpegQuality.value)) };
});

watch(currentFormat, (fmt) => {
  moduleFormat.value = fmt;
}, { deep: true });

// Filtered look options
const fujifilmLooks = computed(() => builtinLooks.value.filter((l) => l.maker.toLowerCase() === 'fujifilm'));
const ricohLooks = computed(() => builtinLooks.value.filter((l) => l.maker.toLowerCase() === 'ricoh'));

// Output destination
const outputDestination = computed(() => {
  if (!folder.value.trim()) return '';
  return `${folder.value.replace(/\/+$/, '')}/Developed`;
});

// Active look for selection
const selectedLook = computed(() => {
  if (selection.value.length === 0) return '';
  const first = moduleAssignments.value[selection.value[0]]?.look ?? null;
  const allSame = selection.value.every(
    (name) => (moduleAssignments.value[name]?.look ?? null) === first,
  );
  if (!allSame) return '__mixed__';
  return first ?? '';
});

// Active exposure for selection
const selectedExposure = computed(() => {
  if (selection.value.length === 0) return 0.0;
  const first = moduleAssignments.value[selection.value[0]]?.exposure_ev ?? 0.0;
  const allSame = selection.value.every(
    (name) => (moduleAssignments.value[name]?.exposure_ev ?? 0.0) === first,
  );
  if (!allSame) return 0.0;
  return first;
});

// ---------------------------------------------------------------------------
// Actions: folder scan & sequential thumbnail loading
// ---------------------------------------------------------------------------

// Helper: look display name on thumbnails (RP-1 review item 6)
function lookDisplayName(lookId: string | null | undefined): string {
  if (!lookId) return 'None';
  const builtin = builtinLooks.value.find((b) => b.id === lookId);
  if (builtin) return builtin.name;
  const lut = lutLibrary.value.luts.find((l) => l.name === lookId);
  if (lut) return lut.name;
  if (lookId.startsWith('builtin:')) {
    return lookId.slice('builtin:'.length);
  }
  return lookId;
}

// ---------------------------------------------------------------------------
// Actions: folder scan & sequential thumbnail loading
// ---------------------------------------------------------------------------

async function loadFolder() {
  const currentPath = folder.value.trim();
  if (!currentPath) {
    photos.value = [];
    folderError.value = null;
    return;
  }

  isFolderLoading.value = true;
  folderError.value = null;
  for (const u of Object.values(thumbnailUrls.value)) {
    URL.revokeObjectURL(u);
  }
  thumbnailUrls.value = {};
  thumbnailErrors.value = {};
  const currentToken = ++thumbnailToken;

  try {
    const names = await desktop.listDevelopFolder(currentPath);
    if (currentToken !== thumbnailToken) return;

    photos.value = names;

    // Initialize assignments for new photos
    const assignments = { ...moduleAssignments.value };
    for (const name of names) {
      if (!assignments[name]) {
        assignments[name] = { look: null, exposure_ev: 0.0 };
      }
    }
    moduleAssignments.value = assignments;

    // Keep selection valid
    const validSelection = selection.value.filter((s) => names.includes(s));
    if (validSelection.length === 0 && names.length > 0) {
      selection.value = [names[0]];
      lastClicked.value = names[0];
    } else {
      selection.value = validSelection;
      if (!lastClicked.value || !names.includes(lastClicked.value)) {
        lastClicked.value = validSelection[0] ?? names[0] ?? null;
      }
    }

    // Trigger preview for active photo
    triggerPreview();

    // Load thumbnails sequentially (one at a time, zero overlap)
    for (const name of names) {
      if (currentToken !== thumbnailToken) break;
      try {
        const bytes = await desktop.developThumbnail(currentPath, name);
        if (currentToken !== thumbnailToken) break;
        const blob = new Blob([bytes], { type: 'image/jpeg' });
        thumbnailUrls.value[name] = URL.createObjectURL(blob);
      } catch (err: unknown) {
        if (currentToken !== thumbnailToken) break;
        thumbnailErrors.value[name] = err instanceof Error ? err.message : String(err);
      }
    }
  } catch (err: unknown) {
    if (currentToken !== thumbnailToken) return;
    photos.value = [];
    folderError.value = err instanceof Error ? err.message : String(err);
  } finally {
    if (currentToken === thumbnailToken) {
      isFolderLoading.value = false;
    }
  }
}

// ---------------------------------------------------------------------------
// Actions: Selection
// ---------------------------------------------------------------------------

function handlePhotoClick(name: string, event: MouseEvent) {
  if (event.metaKey || event.ctrlKey) {
    // Toggle
    const next = new Set(selection.value);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    selection.value = Array.from(next);
  } else if (event.shiftKey && lastClicked.value && photos.value.includes(lastClicked.value)) {
    // Range select
    const idx1 = photos.value.indexOf(lastClicked.value);
    const idx2 = photos.value.indexOf(name);
    const [start, end] = idx1 < idx2 ? [idx1, idx2] : [idx2, idx1];
    const range = photos.value.slice(start, end + 1);
    const next = new Set(selection.value);
    for (const item of range) next.add(item);
    selection.value = Array.from(next);
  } else {
    // Single select
    selection.value = [name];
  }
  lastClicked.value = name;
  triggerPreview();
}

function selectAll() {
  selection.value = [...photos.value];
  if (photos.value.length > 0 && !lastClicked.value) {
    lastClicked.value = photos.value[0];
    triggerPreview();
  }
}

// ---------------------------------------------------------------------------
// Actions: Look & Exposure assignment
// ---------------------------------------------------------------------------

function onLookChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (value === '__mixed__') return;
  const look = value === '' || value === 'none' ? null : value;

  const updated = { ...moduleAssignments.value };
  for (const name of selection.value) {
    const existing = updated[name] ?? { look: null, exposure_ev: 0.0 };
    updated[name] = { ...existing, look };
  }
  moduleAssignments.value = updated;
  triggerPreview();
}

function onExposureInput(exposure_ev: number) {
  const updated = { ...moduleAssignments.value };
  for (const name of selection.value) {
    const existing = updated[name] ?? { look: null, exposure_ev: 0.0 };
    updated[name] = { ...existing, exposure_ev };
  }
  moduleAssignments.value = updated;
  triggerPreview(true);
}

function onExposureChange(exposure_ev: number) {
  const updated = { ...moduleAssignments.value };
  for (const name of selection.value) {
    const existing = updated[name] ?? { look: null, exposure_ev: 0.0 };
    updated[name] = { ...existing, exposure_ev };
  }
  moduleAssignments.value = updated;
  triggerPreview(false);
}

// ---------------------------------------------------------------------------
// Actions: Serialized at-most-one-in-flight preview rendering (RP-1 review item 1)
// ---------------------------------------------------------------------------

async function triggerPreview(draft = false) {
  const targetPhoto = lastClicked.value;
  const currentPath = folder.value.trim();
  if (!targetPhoto || !currentPath || !photos.value.includes(targetPhoto)) {
    if (previewUrl.value) {
      URL.revokeObjectURL(previewUrl.value);
      previewUrl.value = null;
    }
    previewError.value = null;
    pendingPreviewState = null;
    return;
  }

  const assignment = moduleAssignments.value[targetPhoto] ?? { look: null, exposure_ev: 0.0 };
  const wanted: WantedPreviewState = {
    folder: currentPath,
    name: targetPhoto,
    look: assignment.look,
    exposure_ev: assignment.exposure_ev,
    draft,
  };

  if (previewInFlight) {
    pendingPreviewState = wanted;
    return;
  }

  await executePreview(wanted);
}

async function executePreview(wanted: WantedPreviewState) {
  previewInFlight = true;
  pendingPreviewState = null;
  const seq = ++previewSeq;
  isPreviewLoading.value = true;
  previewError.value = null;

  try {
    const bytes = await desktop.previewDevelop(
      wanted.folder,
      wanted.name,
      wanted.look,
      wanted.exposure_ev,
      wanted.draft,
    );
    if (seq === previewSeq) {
      if (previewUrl.value) {
        URL.revokeObjectURL(previewUrl.value);
      }
      const blob = new Blob([bytes], { type: 'image/jpeg' });
      previewUrl.value = URL.createObjectURL(blob);
    }
  } catch (err: unknown) {
    if (seq === previewSeq) {
      previewError.value = err instanceof Error ? err.message : String(err);
    }
  } finally {
    previewInFlight = false;
    if (pendingPreviewState !== null) {
      const next = pendingPreviewState;
      pendingPreviewState = null;
      executePreview(next);
    } else if (seq === previewSeq) {
      isPreviewLoading.value = false;
    }
  }
}

// ---------------------------------------------------------------------------
// Actions: Dry run & Lock discipline (RP-1 review item 5)
// ---------------------------------------------------------------------------

function makeItems(): DevelopItem[] {
  const targetPhotos =
    scope.value === 'selected'
      ? photos.value.filter((name) => selection.value.includes(name))
      : photos.value;

  return targetPhotos.map((name) => {
    const a = moduleAssignments.value[name] ?? { look: null, exposure_ev: 0.0 };
    return {
      name,
      look: a.look,
      exposure_ev: a.exposure_ev,
    };
  });
}

const currentItemsJson = computed(() =>
  JSON.stringify({
    scope: scope.value,
    items: makeItems(),
  })
);
const currentFormatJson = computed(() => JSON.stringify(currentFormat.value));

const settingsChanged = computed(() => {
  if (!reviewed.value) return false;
  return (
    reviewed.value.folder !== folder.value.trim() ||
    reviewed.value.itemsJson !== currentItemsJson.value ||
    reviewed.value.formatJson !== currentFormatJson.value
  );
});

const canPlan = computed(() => {
  if (isPlanning.value || isDeveloping.value) return false;
  if (!folder.value.trim() || photos.value.length === 0) return false;
  if (scope.value === 'selected' && selection.value.length === 0) return false;
  return true;
});

const canRun = computed(() => {
  return (
    reviewed.value !== null &&
    !settingsChanged.value &&
    !isPlanning.value &&
    !isDeveloping.value &&
    (reviewedPlan.value?.actions.length ?? 0) > 0
  );
});

const lockReasons = computed(() => {
  if (isDeveloping.value) {
    return ['Development is in progress.'];
  }
  const reasons: string[] = [];
  if (isPlanning.value) {
    reasons.push('Work is in progress.');
  }
  if (!folder.value.trim()) {
    reasons.push('Choose a folder of RAW files.');
  } else if (photos.value.length === 0) {
    reasons.push('The selected folder contains no RAW files.');
  } else if (scope.value === 'selected' && selection.value.length === 0) {
    reasons.push('Select at least one photograph to develop.');
  }
  if (settingsChanged.value) {
    reasons.push(
      'Settings have changed since the dry run — run it again so the review matches what will be developed.',
    );
  } else if (!reviewed.value) {
    reasons.push(
      'Run is unavailable until a dry run of these settings has been reviewed.',
    );
  }
  return reasons;
});

async function runDryRun() {
  if (!canPlan.value) return;
  isPlanning.value = true;
  planError.value = null;

  try {
    const items = makeItems();
    const plan = await desktop.planDevelop(folder.value.trim(), items, currentFormat.value);
    reviewedPlan.value = plan;
    reviewed.value = {
      folder: folder.value.trim(),
      itemsJson: currentItemsJson.value,
      formatJson: currentFormatJson.value,
    };
  } catch (err: unknown) {
    reviewedPlan.value = null;
    reviewed.value = null;
    planError.value = err instanceof Error ? err.message : String(err);
  } finally {
    isPlanning.value = false;
  }
}

async function runDevelop() {
  if (!canRun.value) return;
  isDeveloping.value = true;
  planError.value = null;

  try {
    const items = makeItems();
    const id = await desktop.applyDevelop(folder.value.trim(), items, currentFormat.value);
    jobId.value = id;
  } catch (err: unknown) {
    isDeveloping.value = false;
    planError.value = err instanceof Error ? err.message : String(err);
  }
}

async function handleCancelJob() {
  if (!jobId.value) return;
  try {
    await desktop.cancelJob(jobId.value);
  } catch (err: unknown) {
    planError.value = `Failed to cancel job: ${err instanceof Error ? err.message : String(err)}`;
  }
}

function handleJobDone() {
  isDeveloping.value = false;
}

// ---------------------------------------------------------------------------
// Lifecycle & initialization (RP-1 review items 3, 4)
// ---------------------------------------------------------------------------

onBeforeUnmount(() => {
  thumbnailToken++;
  for (const u of Object.values(thumbnailUrls.value)) {
    URL.revokeObjectURL(u);
  }
  thumbnailUrls.value = {};
  if (previewUrl.value) {
    URL.revokeObjectURL(previewUrl.value);
    previewUrl.value = null;
  }
});

onMounted(async () => {
  try {
    const [looks, luts] = await Promise.all([
      desktop.listBuiltinLooks(),
      desktop.listLuts(),
    ]);
    builtinLooks.value = looks;
    lutLibrary.value = luts;
  } catch (err) {
    console.error('Failed to load looks:', err);
  }

  // Re-attach to existing job if running, or clear if finished
  if (jobId.value) {
    try {
      const currentJob = await desktop.job(jobId.value);
      if (
        currentJob.status === 'completed' ||
        currentJob.status === 'cancelled' ||
        currentJob.status === 'failed'
      ) {
        isDeveloping.value = false;
      }
    } catch {
      isDeveloping.value = false;
    }
  }

  if (folder.value) {
    loadFolder();
  }
});
</script>

<template>
  <div class="develop-page">
    <header class="page-head">
      <h1>Develop</h1>
      <p class="muted">
        Open a folder of RAW photographs, assign film simulation looks, and develop to JPEG or 16-bit ProPhoto TIFF.
      </p>
    </header>

    <!-- Folder selection -->
    <section class="section-box">
      <PathField
        v-model="folder"
        label="RAW folder"
        placeholder="/path/to/raw/photos"
        :roots="roots"
        :roots-error="rootsError"
        :list="listRoots"
        :disabled="isDeveloping || isPlanning"
        data-testid="develop-folder-input"
      />
      <div v-if="folderError" class="error-line" role="alert">
        {{ folderError }}
      </div>
      <div v-else-if="!isFolderLoading && folder.trim() && photos.length === 0" class="muted-note">
        No RAW files found in this folder.
      </div>
    </section>

    <!-- Main workspace: Photographs Grid + Preview & Controls -->
    <div v-if="photos.length > 0" class="workspace">
      <!-- Left: Photographs grid -->
      <section class="photos-panel">
        <header class="panel-head">
          <span class="mono">{{ photos.length }} photographs</span>
          <button
            type="button"
            class="secondary select-all-btn"
            data-testid="select-all-button"
            :disabled="isDeveloping"
            @click="selectAll"
          >
            Select all
          </button>
        </header>

        <div class="thumbnail-grid" data-testid="thumbnail-grid">
          <div
            v-for="name in photos"
            :key="name"
            class="thumbnail-card"
            :class="{ active: selection.includes(name), 'is-last-clicked': lastClicked === name }"
            :data-filename="name"
            data-testid="thumbnail-item"
            @click="handlePhotoClick(name, $event)"
          >
            <div class="thumb-container">
              <img
                v-if="thumbnailUrls[name]"
                :src="thumbnailUrls[name]"
                :alt="name"
                class="thumb-img"
              />
              <div v-else-if="thumbnailErrors[name]" class="thumb-error mono">
                <span>{{ name }}</span>
              </div>
              <div v-else class="thumb-placeholder">
                <span class="cursor">_</span>
              </div>
            </div>
            <div class="thumb-meta">
              <span class="thumb-name mono" :title="name">{{ name }}</span>
              <span class="thumb-look mono">
                {{ lookDisplayName(moduleAssignments[name]?.look) }}
              </span>
            </div>
          </div>
        </div>
      </section>

      <!-- Right: Preview and adjustments -->
      <aside class="controls-panel">
        <!-- Preview display -->
        <div class="preview-box" data-testid="preview-container">
          <div class="preview-header">
            <span class="preview-title mono">
              {{ lastClicked ?? 'No photograph selected' }}
            </span>
            <span v-if="isPreviewLoading" class="preview-loading mono">
              Developing<span class="cursor">_</span>
            </span>
          </div>

          <div class="preview-display">
            <img
              v-if="previewUrl"
              :src="previewUrl"
              :alt="lastClicked ?? 'Preview'"
              class="preview-img"
              data-testid="preview-image"
            />
            <div v-else-if="previewError" class="preview-error">
              {{ previewError }}
            </div>
            <div v-else class="preview-empty muted">
              Click a photograph to preview
            </div>
          </div>
        </div>

        <!-- Adjustments for selection -->
        <div class="adjustments-box" :class="{ disabled: selection.length === 0 }">
          <div class="selection-summary mono">
            Selection: {{ selection.length }} of {{ photos.length }} selected
          </div>

          <!-- Film look picker -->
          <div class="control-row">
            <label class="control-label" for="develop-look-select">Film look</label>
            <select
              id="develop-look-select"
              class="look-select"
              data-testid="look-select"
              :disabled="selection.length === 0 || isDeveloping"
              :value="selectedLook"
              @change="onLookChange"
            >
              <option v-if="selectedLook === '__mixed__'" value="__mixed__" disabled>
                (Multiple looks selected)
              </option>
              <optgroup label="Neutral">
                <option value="">None (neutral)</option>
              </optgroup>
              <optgroup v-if="fujifilmLooks.length" label="Fujifilm">
                <option
                  v-for="look in fujifilmLooks"
                  :key="look.id"
                  :value="look.id"
                >
                  {{ look.name }}
                </option>
              </optgroup>
              <optgroup v-if="ricohLooks.length" label="Ricoh">
                <option
                  v-for="look in ricohLooks"
                  :key="look.id"
                  :value="look.id"
                >
                  {{ look.name }}
                </option>
              </optgroup>
              <optgroup v-if="lutLibrary.luts.length" label="My LUTs">
                <option
                  v-for="lut in lutLibrary.luts"
                  :key="lut.name"
                  :value="lut.name"
                >
                  {{ lut.name }}
                </option>
              </optgroup>
            </select>
          </div>

          <!-- Exposure adjustment -->
          <div class="control-row">
            <AdjustmentSlider
              label="Exposure"
              :min="-4"
              :max="4"
              :step="0.1"
              unit=" EV"
              :default-value="0"
              :model-value="selectedExposure"
              :disabled="selection.length === 0 || isDeveloping"
              test-id="exposure-slider"
              @change="onExposureChange"
              @update:model-value="onExposureInput"
            />
          </div>
        </div>

        <!-- Output format settings -->
        <div class="output-box">
          <span class="box-title mono">Output settings</span>
          <div class="format-toggle" role="radiogroup" aria-label="Output format">
            <label class="format-option">
              <input
                v-model="outputType"
                type="radio"
                value="jpeg"
                :disabled="isDeveloping"
                data-testid="format-jpeg"
              />
              <span>JPEG (sRGB)</span>
            </label>
            <label class="format-option">
              <input
                v-model="outputType"
                type="radio"
                value="tiff"
                :disabled="isDeveloping"
                data-testid="format-tiff"
              />
              <span>TIFF (16-bit ProPhoto RGB)</span>
            </label>
          </div>

          <div v-if="outputType === 'jpeg'" class="quality-row">
            <label for="develop-jpeg-quality" class="control-label">Quality</label>
            <input
              id="develop-jpeg-quality"
              v-model.number="jpegQuality"
              type="number"
              min="1"
              max="100"
              class="quality-input"
              data-testid="jpeg-quality-input"
              :disabled="isDeveloping"
            />
          </div>

          <div class="destination-note muted mono">
            Destination: <code>{{ outputDestination }}</code>
          </div>
        </div>

        <!-- Run and Dry-run action buttons -->
        <div class="actions-box">
          <div class="scope-toggle" role="radiogroup" aria-label="Develop scope">
            <label class="scope-option">
              <input
                v-model="scope"
                type="radio"
                value="all"
                :disabled="isDeveloping"
                data-testid="scope-all"
              />
              <span>All photographs ({{ photos.length }})</span>
            </label>
            <label class="scope-option">
              <input
                v-model="scope"
                type="radio"
                value="selected"
                :disabled="isDeveloping"
                data-testid="scope-selected"
              />
              <span>Selected only ({{ selection.length }})</span>
            </label>
          </div>

          <div class="action-buttons">
            <button
              type="button"
              class="secondary dry-run-btn"
              data-testid="dry-run-button"
              :disabled="!canPlan"
              @click="runDryRun"
            >
              <template v-if="isPlanning">
                Planning<span class="cursor">_</span>
              </template>
              <template v-else>
                Dry run
              </template>
            </button>

            <button
              type="button"
              class="primary run-btn"
              data-testid="run-button"
              :disabled="!canRun"
              @click="runDevelop"
            >
              <template v-if="isDeveloping && !jobId">
                Starting<span class="cursor">_</span>
              </template>
              <template v-else>
                Develop
              </template>
            </button>
          </div>

          <div
            v-if="lockReasons.length"
            class="gate-explanation"
            data-testid="gate-explanation"
            role="status"
          >
            <p v-for="reason in lockReasons" :key="reason">{{ reason }}</p>
          </div>
        </div>
      </aside>
    </div>

    <!-- Review plan details -->
    <section v-if="reviewedPlan" class="review-section" data-testid="review-section">
      <header class="review-head">
        <h2>Review summary</h2>
        <span class="badge">{{ reviewedPlan.actions.length }} to develop</span>
      </header>

      <div class="rule-box">
        <p class="rule-line">
          Format:
          <strong>
            {{ reviewedPlan.format.kind === 'tiff' ? '16-bit TIFF (ProPhoto RGB)' : `JPEG quality ${reviewedPlan.format.quality}` }}
          </strong>
          into <code>{{ outputDestination }}</code>
        </p>
        <p class="rule-line muted">
          Existing files are never overwritten. Metadata and EXIF orientation are preserved on all outputs.
        </p>
      </div>

      <div v-if="reviewedPlan.skipped.length" class="skipped-section" data-testid="skipped-section">
        <h3>Skipped items ({{ reviewedPlan.skipped.length }})</h3>
        <ul class="skipped-list">
          <li
            v-for="skip in reviewedPlan.skipped"
            :key="skip.file"
            class="skipped-row mono"
            data-testid="skipped-item"
          >
            <span class="skipped-file">{{ skip.file }}</span>
            <span class="skipped-reason">— {{ skip.reason }}</span>
          </li>
        </ul>
      </div>
    </section>

    <!-- Errors and feedback -->
    <div v-if="planError" class="error-banner" role="alert">
      {{ planError }}
    </div>

    <!-- Active Job Progress with Cancel -->
    <section v-if="jobId" class="job-section" data-testid="job-section">
      <JobProgress
        :job-id="jobId"
        :can-cancel="true"
        @cancel="handleCancelJob"
        @done="handleJobDone"
      />
    </section>
  </div>
</template>

<style scoped>
.develop-page {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-4);
  max-width: 1400px;
  margin: 0 auto;
}

.page-head {
  display: grid;
  gap: var(--space-1);
}

.page-head h1 {
  font-family: var(--font-label);
  font-size: 20px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text);
  margin: 0;
}

.section-box {
  display: grid;
  gap: var(--space-2);
}

.workspace {
  display: grid;
  grid-template-columns: 1fr 400px;
  gap: var(--space-4);
  align-items: start;
}

.photos-panel {
  display: grid;
  gap: var(--space-3);
  border: var(--border-hair);
  padding: var(--space-3);
  background: var(--bg-surface);
}

.panel-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.select-all-btn {
  min-height: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  border-radius: 2px;
}

.thumbnail-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: var(--space-2);
  max-height: 700px;
  overflow-y: auto;
}

.thumbnail-card {
  display: grid;
  gap: var(--space-1);
  padding: var(--space-2);
  border: var(--border-hair);
  background: var(--bg);
  cursor: pointer;
  border-radius: 2px;
  min-height: 140px;
}

.thumbnail-card.active {
  border-color: var(--accent);
  background: var(--bg-surface);
}

.thumbnail-card.is-last-clicked {
  outline: 1px solid var(--accent);
}

.thumb-container {
  width: 100%;
  aspect-ratio: 3 / 2;
  background: var(--bg-deep);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.thumb-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.thumb-error {
  padding: var(--space-1);
  font-size: 11px;
  color: var(--text-muted);
  text-align: center;
  word-break: break-all;
}

.thumb-placeholder {
  color: var(--text-muted);
}

.thumb-meta {
  display: grid;
  gap: 2px;
  font-size: 11px;
}

.thumb-name {
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.thumb-look {
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.controls-panel {
  display: grid;
  gap: var(--space-3);
}

.preview-box {
  border: var(--border-hair);
  padding: var(--space-3);
  background: var(--bg-surface);
  display: grid;
  gap: var(--space-2);
}

.preview-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
}

.preview-title {
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.preview-loading {
  color: var(--accent);
}

.preview-display {
  width: 100%;
  aspect-ratio: 3 / 2;
  background: var(--bg-deep);
  display: flex;
  align-items: center;
  justify-content: center;
  border: var(--border-hair);
  overflow: hidden;
}

.preview-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.preview-empty,
.preview-error {
  font-size: 12px;
  padding: var(--space-3);
  text-align: center;
}

.preview-error {
  color: var(--danger);
}

.adjustments-box,
.output-box,
.actions-box {
  border: var(--border-hair);
  padding: var(--space-3);
  background: var(--bg-surface);
  display: grid;
  gap: var(--space-3);
}

.adjustments-box.disabled {
  opacity: 0.5;
  pointer-events: none;
}

.selection-summary {
  font-size: 12px;
  color: var(--text-muted);
}

.control-row {
  display: grid;
  gap: var(--space-1);
}

.control-label {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
}

.look-select {
  min-height: 40px;
  padding: var(--space-2);
  background: var(--bg);
  color: var(--text);
  border: var(--border-hair);
  border-radius: 2px;
  font-family: var(--font-mono);
  font-size: 13px;
}

.box-title {
  font-size: 12px;
  color: var(--text-muted);
  text-transform: uppercase;
}

.format-toggle {
  display: grid;
  gap: var(--space-2);
}

.format-option {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  cursor: pointer;
  font-size: 13px;
}

.quality-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.quality-input {
  min-height: 40px;
  width: 80px;
  padding: var(--space-1) var(--space-2);
  background: var(--bg);
  color: var(--text);
  border: var(--border-hair);
  border-radius: 2px;
  font-family: var(--font-mono);
  font-size: 14px;
}

.destination-note {
  font-size: 11px;
  word-break: break-all;
}

.action-buttons {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-2);
}

.dry-run-btn,
.run-btn {
  min-height: 40px;
  padding: var(--space-2);
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.05em;
  border-radius: 2px;
}

.gate-explanation {
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.4;
}

.review-section {
  display: grid;
  gap: var(--space-3);
  border: var(--border-hair);
  padding: var(--space-4);
  background: var(--bg-surface);
}

.review-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.review-head h2 {
  font-family: var(--font-label);
  font-size: 16px;
  margin: 0;
}

.badge {
  font-family: var(--font-mono);
  font-size: 12px;
  padding: 2px 8px;
  background: var(--bg-deep);
  border: var(--border-hair);
  border-radius: 2px;
}

.rule-box {
  background: var(--bg);
  border: var(--border-hair);
  padding: var(--space-3);
  display: grid;
  gap: var(--space-1);
}

.rule-line {
  margin: 0;
  font-size: 13px;
}

.skipped-section {
  display: grid;
  gap: var(--space-2);
}

.skipped-section h3 {
  font-size: 13px;
  margin: 0;
}

.skipped-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  gap: 4px;
  max-height: 160px;
  overflow-y: auto;
}

.skipped-row {
  font-size: 12px;
  display: flex;
  gap: var(--space-2);
}

.skipped-file {
  color: var(--text);
}

.skipped-reason {
  color: var(--text-muted);
}

.scope-toggle {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.scope-option {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  cursor: pointer;
  font-size: 13px;
  user-select: none;
}

.error-banner,
.error-line {
  color: var(--danger);
  font-size: 12px;
}

.cursor {
  animation: blink 1s step-start infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}
</style>
