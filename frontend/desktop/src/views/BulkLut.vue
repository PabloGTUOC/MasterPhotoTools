<script setup lang="ts">
/**
 * Batch Grade view: a 3D LUT (ED-8) or a saved preset (ED-18) across folders or files.
 *
 * - Source: "3D LUT" applies one LUT at a chosen strength, written as `_lut`; "Preset"
 *   applies a whole saved look, written as `_edit`. Presets never carry a crop, so a
 *   batch never reframes anything (ED-17).
 * - A preset run is locked to the recipe hash the dry run reviewed; if the preset is
 *   replaced after the review, core refuses the run and the refusal is shown here.
 *
 * Dedicated workflow for applying a 3D LUT across folders or files:
 * - Inputs: Source photographs (folders/files via PathListField), recursive toggle,
 *   3D LUT selector with blend intensity (LutPicker), and output folder (PathField).
 * - Dry run (planBulkLut): Calculates graded counts, skipped files with reasons,
 *   explains '_lut' suffix output naming, and sequentially previews sample frames.
 * - Lock discipline: Run button remains locked until a dry run exists for exact current
 *   settings. Changing any parameter invalidates the dry run and lists all lock reasons.
 *   The run action passes the reviewed lut_sha256 from the dry run.
 * - Job execution: Followed with JobProgress with cancellation support. Written, skipped,
 *   failed, and metadata skipped counts are reported; refusals from core are shown verbatim.
 */
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import type { BrowserEntry } from '@phototools/shared';
import type {
  AdjustmentRecipe,
  BulkLutPlanSummary,
  LutLibraryList,
} from '@host/api';
import { desktop } from '@host/api';
import JobProgress from '@ui/components/JobProgress.vue';
import LutPicker from '@ui/components/LutPicker.vue';
import type { LutItem } from '@ui/components/LutPicker.vue';
import PathField from '@ui/components/PathField.vue';
import PathListField from '@ui/components/PathListField.vue';
import { useRoots } from '@ui/useRoots';

interface SampleFramePreview {
  path: string;
  width?: number;
  height?: number;
  pixels?: Uint8ClampedArray;
  error?: string;
}

type Source = 'lut' | 'preset';

interface ReviewedSettings {
  source: Source;
  inputs: string;
  recursive: boolean;
  lutName: string;
  lutSha256: string;
  intensity: number;
  presetName: string;
  /** The preset's recipe hash the dry run reported, passed back verbatim to the run. */
  recipeSha256: string;
  outDir: string;
}

/** What the review shows, whichever source planned it. */
type ReviewedPlan = Pick<BulkLutPlanSummary, 'actions_count' | 'skipped' | 'sample_frames'>;

const { roots, failure: rootsError } = useRoots();
const listRoots = (p: string): Promise<BrowserEntry[]> => desktop.list(p);

// Form state
const source = ref<Source>('lut');
const selectedPreset = ref('');
const presetNames = ref<string[]>([]);
const inputsText = ref('');
const recursive = ref(false);
const selectedLut = ref<LutItem | null>(null);
const intensity = ref(1.0);
const outDir = ref('');

// Library state
const lutList = ref<LutLibraryList>({ luts: [], errors: [] });

// Operation state
const isBusy = ref(false);
const isGrading = ref(false);
const isGeneratingPreviews = ref(false);
const failure = ref<string | null>(null);

// Reviewed dry-run state
const reviewed = ref<ReviewedSettings | null>(null);
const reviewedPlan = ref<ReviewedPlan | null>(null);
const samplePreviews = ref<SampleFramePreview[]>([]);
const sampleCanvasRefs = new Map<number, HTMLCanvasElement>();

// Active job state
const jobId = ref<string | null>(null);

const parsedInputs = computed(() =>
  inputsText.value
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean),
);

const settingsChanged = computed(() => {
  if (!reviewed.value) return false;
  const r = reviewed.value;
  if (
    r.source !== source.value ||
    r.inputs !== inputsText.value.trim() ||
    r.recursive !== recursive.value ||
    r.outDir !== outDir.value.trim()
  ) {
    return true;
  }
  return source.value === 'lut'
    ? r.lutName !== (selectedLut.value?.name ?? '') || r.intensity !== intensity.value
    : r.presetName !== selectedPreset.value;
});

const hasSource = computed(() =>
  source.value === 'lut' ? selectedLut.value !== null : selectedPreset.value !== '',
);

const canPlan = computed(() => {
  return (
    !isBusy.value &&
    !isGrading.value &&
    parsedInputs.value.length > 0 &&
    hasSource.value &&
    outDir.value.trim().length > 0
  );
});

const canRun = computed(() => {
  return (
    reviewed.value !== null &&
    !settingsChanged.value &&
    !isBusy.value &&
    !isGrading.value &&
    (reviewedPlan.value?.actions_count ?? 0) > 0 &&
    hasSource.value &&
    outDir.value.trim().length > 0
  );
});

const lockReasons = computed(() => {
  if (isGrading.value) {
    return ['A grade is in progress.'];
  }
  const reasons: string[] = [];
  if (isBusy.value) {
    reasons.push('Work is in progress.');
  }
  if (settingsChanged.value) {
    reasons.push(
      'Settings have changed since the dry run — run it again so the review matches what will be graded.',
    );
  } else if (!reviewed.value) {
    reasons.push(
      'Run is unavailable until a dry run of these settings has been reviewed.',
    );
  }
  if (!parsedInputs.value.length) {
    reasons.push('No input files or folders selected.');
  }
  if (!hasSource.value) {
    reasons.push(source.value === 'lut' ? 'No 3D LUT selected.' : 'No preset selected.');
  }
  if (!outDir.value.trim()) {
    reasons.push('No destination folder selected.');
  }
  if (reviewed.value && !settingsChanged.value && reviewedPlan.value?.actions_count === 0) {
    reasons.push('No files matched the selected inputs.');
  }
  return reasons;
});

const runButtonLabel = computed(() => {
  if (reviewed.value && !settingsChanged.value && reviewedPlan.value) {
    return `Grade ${reviewedPlan.value.actions_count} photos`;
  }
  return 'Grade photos';
});

async function refreshLuts() {
  try {
    lutList.value = await desktop.listLuts();
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    failure.value = `Failed to list LUTs: ${msg}`;
  }
}

async function refreshPresets() {
  try {
    const list = await desktop.listPresets();
    presetNames.value = list.presets.map((p) => p.name);
    if (list.errors.length) {
      failure.value = `Some presets could not be read: ${list.errors
        .map((e) => `${e.name} (${e.error})`)
        .join('; ')}`;
    }
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    failure.value = `Failed to list presets: ${msg}`;
  }
}

async function handleImportLut(path: string) {
  try {
    failure.value = null;
    const entry = await desktop.importLut(path);
    await refreshLuts();
    selectedLut.value = { name: entry.name, sha256: entry.sha256, format: entry.format };
  } catch (err: unknown) {
    failure.value = err instanceof Error ? err.message : String(err);
  }
}

function onLutChange(lut: { name: string; sha256: string } | null) {
  selectedLut.value = lut ? { name: lut.name, sha256: lut.sha256 } : null;
}

function onIntensityInput(val: number) {
  intensity.value = val;
}

function onIntensityChange(val: number) {
  intensity.value = val;
}

function setSampleCanvas(el: HTMLCanvasElement | null, idx: number) {
  if (el) {
    sampleCanvasRefs.set(idx, el);
    paintSampleCanvas(el, samplePreviews.value[idx]);
  } else {
    sampleCanvasRefs.delete(idx);
  }
}

function paintSampleCanvas(canvas: HTMLCanvasElement, sample?: SampleFramePreview) {
  if (!canvas || !sample || !sample.pixels || !sample.width || !sample.height) return;
  if (canvas.width !== sample.width || canvas.height !== sample.height) {
    canvas.width = sample.width;
    canvas.height = sample.height;
  }
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const imgData = new ImageData(sample.pixels, sample.width, sample.height);
  ctx.putImageData(imgData, 0, 0);
}

watch(
  samplePreviews,
  async (newSamples) => {
    await nextTick();
    newSamples.forEach((sample, idx) => {
      const canvas = sampleCanvasRefs.get(idx);
      if (canvas) {
        paintSampleCanvas(canvas, sample);
      }
    });
  },
  { deep: true },
);

async function runDryRun() {
  if (!canPlan.value) return;
  isBusy.value = true;
  failure.value = null;
  samplePreviews.value = [];
  sampleCanvasRefs.clear();

  try {
    // The recipe each sample is rendered with: the LUT alone, or the whole preset.
    let previewRecipe: Partial<AdjustmentRecipe> | null = null;
    let previewError: string | null = null;

    if (source.value === 'lut') {
      if (!selectedLut.value) return;
      const plan = await desktop.planBulkLut(
        parsedInputs.value,
        selectedLut.value.name,
        intensity.value,
        outDir.value.trim(),
        recursive.value,
      );
      reviewedPlan.value = plan;
      reviewed.value = {
        source: 'lut',
        inputs: inputsText.value.trim(),
        recursive: recursive.value,
        lutName: selectedLut.value.name,
        lutSha256: plan.lut_sha256,
        intensity: intensity.value,
        presetName: '',
        recipeSha256: '',
        outDir: outDir.value.trim(),
      };
      previewRecipe = {
        lut: { name: selectedLut.value.name, sha256: plan.lut_sha256 },
        lut_intensity: intensity.value,
      };
    } else {
      const name = selectedPreset.value;
      const plan = await desktop.planBulkEdit(
        parsedInputs.value,
        { kind: 'Preset', name },
        outDir.value.trim(),
        recursive.value,
      );
      reviewedPlan.value = plan;
      reviewed.value = {
        source: 'preset',
        inputs: inputsText.value.trim(),
        recursive: recursive.value,
        lutName: '',
        lutSha256: '',
        intensity: 1,
        presetName: name,
        recipeSha256: plan.recipe_sha256,
        outDir: outDir.value.trim(),
      };
      try {
        previewRecipe = await desktop.loadPreset(name);
      } catch (err: unknown) {
        previewError = err instanceof Error ? err.message : String(err);
      }
    }

    // Sequential preview session rendering: at most one open session at a time
    isGeneratingPreviews.value = true;
    for (const framePath of reviewedPlan.value?.sample_frames ?? []) {
      if (!previewRecipe) {
        samplePreviews.value.push({
          path: framePath,
          error: `Couldn't preview ${fileName(framePath)}: ${previewError}`,
        });
        continue;
      }
      let sessionId: string | null = null;
      try {
        const openRes = await desktop.openPreview(framePath);
        sessionId = openRes.session_id;
        const rendered = await desktop.renderPreview(
          sessionId,
          previewRecipe as AdjustmentRecipe,
          'Settle',
        );
        samplePreviews.value.push({
          path: framePath,
          width: rendered.width,
          height: rendered.height,
          pixels: rendered.pixels,
        });
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        samplePreviews.value.push({
          path: framePath,
          error: `Couldn't preview ${fileName(framePath)}: ${msg}`,
        });
      } finally {
        if (sessionId) {
          try {
            await desktop.closePreview(sessionId);
          } catch {
            // ignore close error
          }
        }
      }
    }
  } catch (err: unknown) {
    failure.value = err instanceof Error ? err.message : String(err);
  } finally {
    isGeneratingPreviews.value = false;
    isBusy.value = false;
  }
}

async function runBulkLut() {
  if (!canRun.value || !reviewed.value) return;
  isGrading.value = true;
  failure.value = null;
  jobId.value = null;

  try {
    const r = reviewed.value;
    // Invariant: the reviewed hash is passed verbatim, never recomputed here.
    const id =
      r.source === 'lut'
        ? await desktop.applyBulkLut(
            parsedInputs.value,
            r.lutName,
            r.intensity,
            r.outDir,
            r.lutSha256,
            r.recursive,
          )
        : await desktop.applyBulkEdit(
            parsedInputs.value,
            { kind: 'Preset', name: r.presetName },
            r.outDir,
            r.recipeSha256,
            r.recursive,
          );
    jobId.value = id;
  } catch (err: unknown) {
    failure.value = err instanceof Error ? err.message : String(err);
    isGrading.value = false;
  }
}

async function handleCancelJob() {
  if (!jobId.value) return;
  try {
    const cancelled = await desktop.cancelJob(jobId.value);
    if (!cancelled) {
      failure.value = 'The job had already finished.';
    }
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    failure.value = `Failed to cancel job: ${msg}`;
  }
}

function handleJobDone() {
  isGrading.value = false;
}

function fileName(p: string): string {
  const parts = p.split(/[/\\]/);
  return parts[parts.length - 1] || p;
}

onMounted(() => {
  refreshLuts();
  refreshPresets();
});
</script>

<template>
  <div class="bulk-lut-page">
    <header class="page-head">
      <h1>Batch Grade</h1>
      <p class="muted">
        Apply a 3D LUT or a saved preset across photographs into a destination folder.
      </p>
    </header>

    <div class="form-container">
      <div class="inputs-section">
        <PathListField
          v-model="inputsText"
          label="Source photographs"
          placeholder="/path/to/folder or /path/to/image.jpg (one per line)"
          :roots="roots"
          :roots-error="rootsError"
          :list="listRoots"
          :disabled="isGrading || isBusy"
          :rows="4"
        />

        <label class="checkbox">
          <input
            v-model="recursive"
            type="checkbox"
            :disabled="isGrading || isBusy"
          />
          <span>Include subfolders</span>
        </label>
      </div>

      <div class="source-section">
        <span class="source-label" id="batch-source-label">Grade with</span>
        <div class="source-toggle" role="group" aria-labelledby="batch-source-label">
          <button
            type="button"
            class="source-toggle__btn"
            :class="{ active: source === 'lut' }"
            :aria-pressed="source === 'lut'"
            :disabled="isGrading || isBusy"
            data-testid="source-lut"
            @click="source = 'lut'"
          >
            3D LUT
          </button>
          <button
            type="button"
            class="source-toggle__btn"
            :class="{ active: source === 'preset' }"
            :aria-pressed="source === 'preset'"
            :disabled="isGrading || isBusy"
            data-testid="source-preset"
            @click="source = 'preset'"
          >
            Preset
          </button>
        </div>
      </div>

      <div v-if="source === 'preset'" class="preset-section">
        <label class="source-label" for="batch-preset">Preset</label>
        <select
          id="batch-preset"
          v-model="selectedPreset"
          class="preset-select"
          data-testid="batch-preset-select"
          :disabled="isGrading || isBusy || presetNames.length === 0"
        >
          <option value="" disabled>
            {{ presetNames.length ? 'Choose a preset…' : 'No presets yet: save one in Edit' }}
          </option>
          <option v-for="p in presetNames" :key="p" :value="p">{{ p }}</option>
        </select>
        <p class="muted preset-note">
          The whole look is applied: tone, colour, curves, grading, effects and any LUT the
          preset uses. Crop and rotation are never part of a preset.
        </p>
      </div>

      <div v-else class="lut-section">
        <LutPicker
          :model-value="selectedLut"
          :intensity="intensity"
          :luts="lutList.luts"
          :errors="lutList.errors"
          :roots="roots"
          :roots-error="rootsError"
          :list="listRoots"
          :disabled="isGrading || isBusy"
          @update:model-value="onLutChange"
          @update:intensity="onIntensityInput"
          @change="onIntensityChange"
          @import="handleImportLut"
        />
      </div>

      <div class="destination-section">
        <PathField
          v-model="outDir"
          label="Destination folder"
          placeholder="/path/to/destination"
          :roots="roots"
          :roots-error="rootsError"
          :list="listRoots"
          :disabled="isGrading || isBusy"
        />
      </div>

      <div class="actions-section">
        <div class="button-row">
          <button
            type="button"
            class="secondary"
            :disabled="!canPlan"
            data-testid="dry-run-button"
            @click="runDryRun"
          >
            <template v-if="isBusy">
              Planning<span class="cursor">_</span>
            </template>
            <template v-else>
              Dry run
            </template>
          </button>

          <button
            type="button"
            class="primary"
            :disabled="!canRun"
            data-testid="run-button"
            @click="runBulkLut"
          >
            <template v-if="isGrading && !jobId">
              Starting<span class="cursor">_</span>
            </template>
            <template v-else>
              {{ runButtonLabel }}
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
    </div>

    <!-- Review Plan & Sample Previews -->
    <section v-if="reviewedPlan" class="review-section" data-testid="review-section">
      <header class="review-head">
        <h2>Review summary</h2>
        <span class="badge">{{ reviewedPlan.actions_count }} to grade</span>
      </header>
      <p v-if="reviewed?.source === 'preset'" class="rule-line" data-testid="review-preset">
        Preset: <strong>{{ reviewed.presetName }}</strong>. If it is changed before the run, the run
        is refused and asks for another dry run.
      </p>

      <div class="rule-box">
        <p class="rule-line">
          Output naming:
          <code>&lt;filename&gt;{{ reviewed?.source === 'preset' ? '_edit' : '_lut' }}.&lt;ext&gt;</code>
          in destination folder.
        </p>
        <p class="rule-line muted">
          Existing files are never overwritten. Metadata and EXIF orientation are preserved on all outputs.
        </p>
      </div>

      <!-- Skipped Items -->
      <div v-if="reviewedPlan.skipped.length" class="skipped-box" data-testid="skipped-box">
        <h3 class="box-title">Skipped items ({{ reviewedPlan.skipped.length }})</h3>
        <ul class="skipped-list">
          <li v-for="skip in reviewedPlan.skipped" :key="skip.file" class="skipped-row">
            <span class="skipped-file mono">{{ fileName(skip.file) }}</span>
            <span class="skipped-reason muted">— {{ skip.reason }}</span>
          </li>
        </ul>
      </div>

      <!-- Sample Frames Strip -->
      <div v-if="reviewedPlan.sample_frames.length" class="samples-box" data-testid="samples-box">
        <h3 class="box-title">Sample frame preview strip</h3>
        <p v-if="isGeneratingPreviews" class="muted">
          Rendering sample previews sequentially<span class="cursor">_</span>
        </p>
        <div class="sample-strip" data-testid="sample-strip">
          <div
            v-for="(sample, idx) in samplePreviews"
            :key="sample.path"
            class="sample-card"
            :class="{ 'sample-card--error': !!sample.error }"
            data-testid="sample-frame"
          >
            <div class="sample-canvas-surround">
              <canvas
                v-if="!sample.error && sample.pixels"
                :ref="(el) => setSampleCanvas(el as HTMLCanvasElement | null, idx)"
                class="sample-canvas"
                :width="sample.width"
                :height="sample.height"
              ></canvas>
              <div v-else class="sample-error-display" data-testid="sample-error">
                <span class="sample-error-icon">✕</span>
                <span class="sample-error-text">{{ sample.error }}</span>
              </div>
            </div>
            <span class="sample-filename mono" :title="sample.path">{{ fileName(sample.path) }}</span>
          </div>
        </div>
      </div>
    </section>

    <!-- Error / Refusal from Core -->
    <p v-if="failure" class="error" data-testid="refusal-message" role="alert">
      {{ failure }}
    </p>

    <!-- Job Progress with Cancel -->
    <section v-if="jobId" class="job-section">
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
.bulk-lut-page {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-4);
  max-width: 900px;
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

.form-container {
  display: grid;
  gap: var(--space-4);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  padding: var(--space-4);
}

.inputs-section,
.source-section,
.preset-section,
.lut-section,
.destination-section,
.actions-section {
  display: grid;
  gap: var(--space-3);
}

.source-label {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.source-toggle {
  display: grid;
  grid-template-columns: 1fr 1fr;
  max-width: 360px;
  gap: var(--space-1);
}

.source-toggle__btn {
  min-height: 40px;
  font-size: 13px;
  color: var(--text-muted);
  background: var(--bg-elevated);
  border: var(--border-hair);
  border-radius: var(--radius-none);
}

.source-toggle__btn.active {
  color: var(--accent);
  border: var(--border-active);
}

.preset-select {
  min-height: 40px;
  font-size: 16px;
  max-width: 480px;
}

.preset-note {
  margin: 0;
  font-size: 13px;
}

.button-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.gate-explanation {
  display: grid;
  gap: var(--space-1);
  padding: var(--space-3);
  border: 1px solid var(--accent-warm);
  border-radius: var(--radius-none);
  background: var(--bg-elevated);
  color: var(--accent-warm);
  font-family: var(--font-body);
  font-size: 13px;
  line-height: 1.4;
}

.gate-explanation p {
  margin: 0;
}

.review-section {
  display: grid;
  gap: var(--space-3);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  padding: var(--space-4);
}

.review-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.review-head h2 {
  font-family: var(--font-label);
  font-size: 16px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text);
  margin: 0;
}

.rule-box {
  display: grid;
  gap: var(--space-1);
  padding: var(--space-3);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  background: var(--bg-elevated);
}

.rule-line {
  margin: 0;
  font-size: 13px;
}

.rule-line code {
  color: var(--accent);
  font-family: var(--font-body);
}

.box-title {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin: 0 0 var(--space-2) 0;
}

.skipped-box {
  display: grid;
  gap: var(--space-2);
}

.skipped-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: var(--space-1);
  max-height: 200px;
  overflow-y: auto;
  border: var(--border-hair);
  background: var(--bg-elevated);
}

.skipped-row {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: var(--border-hair);
  font-size: 13px;
}

.skipped-row:last-child {
  border-bottom: none;
}

.skipped-file {
  color: var(--text);
}

.samples-box {
  display: grid;
  gap: var(--space-2);
}

.sample-strip {
  display: flex;
  gap: var(--space-3);
  overflow-x: auto;
  padding-bottom: var(--space-2);
}

.sample-card {
  display: grid;
  gap: var(--space-1);
  flex: 0 0 200px;
  border: var(--border-hair);
  background: var(--bg-elevated);
  padding: var(--space-2);
}

.sample-card--error {
  border-color: var(--danger);
}

.sample-canvas-surround {
  width: 100%;
  aspect-ratio: 3 / 2;
  background: var(--canvas-surround);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.sample-error-display {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  padding: var(--space-2);
  text-align: center;
  color: var(--danger);
  font-family: var(--font-body);
  font-size: 11px;
  line-height: 1.3;
  width: 100%;
  height: 100%;
}

.sample-error-icon {
  font-size: 16px;
  line-height: 1;
}

.sample-error-text {
  word-break: break-word;
}

.sample-canvas {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.sample-filename {
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mono {
  font-family: var(--font-body);
}

.job-section {
  display: grid;
  gap: var(--space-2);
}
</style>
