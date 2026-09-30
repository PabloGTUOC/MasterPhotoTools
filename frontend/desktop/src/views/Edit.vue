<script setup lang="ts">
/**
 * Single-image editor view (ED-7).
 *
 * Dedicated workspace for tuning one photograph at a time:
 * - Viewport: True neutral 18% grey surround (`--canvas-surround`), free of scanlines (`--z-canvas: 60`).
 * - Rendering discipline: At most one IPC `renderPreview` in flight, latest-wins, drag frames (720p) during scrub, settle frame (1440p) on release.
 * - Painting: RGBA8 directly onto canvas with `putImageData`, displayed upright via CSS transform reflecting EXIF orientation.
 * - Controls: Parametric sliders with double-click reset to 0 (identity), 3D LUT selector with intensity, before/after compare (`\` or hold button).
 * - Safe saving: 300 ms debounced `saveRecipe`; card volumes are read-only with safety banner (G5).
 * - Sidecar safety: Unreadable sidecars disable autosave to avoid overwriting existing edits.
 * - Full export: `exportEditedImage` with collision protection and destination validation.
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type {
  AdjustmentRecipe,
  HslAdjustments,
  LutLibraryList,
  LutRef,
  OpenPreviewResult,
  PreviewStage,
  ToneCurves,
} from '@host/api';
import { desktop } from '@host/api';
import AdjustmentSlider from '@ui/components/AdjustmentSlider.vue';
import CollapsibleSection from '@ui/components/CollapsibleSection.vue';
import CurveEditor from '@ui/components/CurveEditor.vue';
import LutPicker from '@ui/components/LutPicker.vue';
import PathField from '@ui/components/PathField.vue';
import { useRoots } from '@ui/useRoots';

const sourcePath = ref('');
const exportDir = ref('');
const sessionId = ref<string | null>(null);
const orientation = ref<number>(1);
const isReadOnly = ref(false);
const autosaveDisabled = ref(false);
const loading = ref(false);
const error = ref<string | null>(null);
const saveStatus = ref<string>('');
const exportStatus = ref<string | null>(null);
const exportError = ref<string | null>(null);
const isBefore = ref(false);

const canvasRef = ref<HTMLCanvasElement | null>(null);
const viewportContainerRef = ref<HTMLDivElement | null>(null);

// Container dimensions for responsive fit
const containerWidth = ref(0);
const containerHeight = ref(0);
const frameWidth = ref(1280);
const frameHeight = ref(854);
let resizeObserver: ResizeObserver | null = null;

// Roots and listing for PathField
const { roots, failure: rootsError } = useRoots();
const listRoots = (p: string) => desktop.list(p);

// Library LUTs
const lutList = ref<LutLibraryList>({ luts: [], errors: [] });

// 8-band HSL definitions (knot angles in OkLCh from reference sRGB colors)
interface HslBandDef {
  key: keyof HslAdjustments;
  label: string;
  hueDeg: number;
}

const HSL_BANDS: HslBandDef[] = [
  { key: 'red', label: 'Red', hueDeg: 29.23 },
  { key: 'orange', label: 'Orange', hueDeg: 52.78 },
  { key: 'yellow', label: 'Yellow', hueDeg: 109.77 },
  { key: 'green', label: 'Green', hueDeg: 142.50 },
  { key: 'aqua', label: 'Aqua', hueDeg: 194.77 },
  { key: 'blue', label: 'Blue', hueDeg: 264.05 },
  { key: 'purple', label: 'Purple', hueDeg: 293.77 },
  { key: 'magenta', label: 'Magenta', hueDeg: 328.36 },
];

const selectedHslBand = ref<keyof HslAdjustments>('red');

const currentBandDef = computed(() => {
  return HSL_BANDS.find((b) => b.key === selectedHslBand.value) ?? HSL_BANDS[0];
});

function createIdentityRecipe(sourceSha = ''): AdjustmentRecipe {
  return {
    version: 2,
    source_sha256: sourceSha,
    exposure: 0.0,
    temperature: 0.0,
    tint: 0.0,
    highlights: 0.0,
    shadows: 0.0,
    contrast: 0.0,
    saturation: 0.0,
    vibrance: 0.0,
    lut: null,
    lut_intensity: 1.0,
    whites: 0.0,
    blacks: 0.0,
    brightness: 0.0,
    hue: 0.0,
    curves: {
      luma: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
      red: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
      green: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
      blue: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
    },
    hsl: {
      red: { hue: 0, saturation: 0, luminance: 0 },
      orange: { hue: 0, saturation: 0, luminance: 0 },
      yellow: { hue: 0, saturation: 0, luminance: 0 },
      green: { hue: 0, saturation: 0, luminance: 0 },
      aqua: { hue: 0, saturation: 0, luminance: 0 },
      blue: { hue: 0, saturation: 0, luminance: 0 },
      purple: { hue: 0, saturation: 0, luminance: 0 },
      magenta: { hue: 0, saturation: 0, luminance: 0 },
    },
  };
}

const recipe = ref<AdjustmentRecipe>(createIdentityRecipe());

// Rendering discipline state
let inFlight = false;
let pending: { recipe: AdjustmentRecipe; stage: PreviewStage } | null = null;
let saveDebounceTimer: number | undefined;

// Whether orientation swaps width and height
const swapsAxes = computed(() => [5, 6, 7, 8].includes(orientation.value));

// Orientation CSS transform (checked against EXIF standard 1-8)
const canvasTransform = computed(() => {
  switch (orientation.value) {
    case 1:
      return 'none';
    case 2:
      return 'scaleX(-1)';
    case 3:
      return 'rotate(180deg)';
    case 4:
      return 'scaleY(-1)';
    case 5:
      // Transpose: reflection across main diagonal (y, x)
      return 'rotate(90deg) scaleY(-1)';
    case 6:
      // Rotate 90 deg CW
      return 'rotate(90deg)';
    case 7:
      // Transverse: reflection across anti-diagonal (-y, -x)
      return 'rotate(90deg) scaleX(-1)';
    case 8:
      // Rotate 270 deg CW
      return 'rotate(270deg)';
    default:
      return 'none';
  }
});

// Sizing so rotated canvas fits inside viewport container without overflowing or clipping
const canvasStyle = computed(() => {
  const cw = containerWidth.value;
  const ch = containerHeight.value;
  const fw = frameWidth.value || 1280;
  const fh = frameHeight.value || 854;

  if (cw <= 0 || ch <= 0) {
    return {
      maxWidth: '100%',
      maxHeight: '100%',
      objectFit: 'contain' as const,
      transform: canvasTransform.value,
      position: 'relative' as const,
      zIndex: 'var(--z-canvas)',
    };
  }

  if (swapsAxes.value) {
    // Rotated image is H wide and W high visually
    const aVis = fh / fw;
    let vW: number;
    let vH: number;
    if (cw / ch > aVis) {
      vH = ch;
      vW = ch * aVis;
    } else {
      vW = cw;
      vH = cw / aVis;
    }
    // Because canvas layout is rotated by 90/270 deg, its DOM width must be vH and DOM height vW
    const lW = vH;
    const lH = vW;
    return {
      width: `${Math.round(lW)}px`,
      height: `${Math.round(lH)}px`,
      transform: canvasTransform.value,
      position: 'relative' as const,
      zIndex: 'var(--z-canvas)',
    };
  } else {
    const a = fw / fh;
    let lW: number;
    let lH: number;
    if (cw / ch > a) {
      lH = ch;
      lW = ch * a;
    } else {
      lW = cw;
      lH = cw / a;
    }
    return {
      width: `${Math.round(lW)}px`,
      height: `${Math.round(lH)}px`,
      transform: canvasTransform.value,
      position: 'relative' as const,
      zIndex: 'var(--z-canvas)',
    };
  }
});

async function refreshLuts() {
  try {
    lutList.value = await desktop.listLuts();
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    error.value = `Failed to list LUTs: ${msg}`;
    console.error('Failed to list LUTs:', err);
  }
}

async function handleImportLut(path: string) {
  try {
    error.value = null;
    await desktop.importLut(path);
    await refreshLuts();
  } catch (err: unknown) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

function paintPixels(frame: { width: number; height: number; pixels: Uint8ClampedArray }, stage: PreviewStage) {
  const canvas = canvasRef.value;
  if (!canvas) return;

  frameWidth.value = frame.width;
  frameHeight.value = frame.height;

  if (canvas.width !== frame.width || canvas.height !== frame.height) {
    canvas.width = frame.width;
    canvas.height = frame.height;
  }
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  const imgData = new ImageData(frame.pixels, frame.width, frame.height);
  ctx.putImageData(imgData, 0, 0);

  // Dispatch custom event for test harness and verification
  canvas.dispatchEvent(
    new CustomEvent('frame-painted', {
      detail: {
        width: frame.width,
        height: frame.height,
        stage,
        timestamp: performance.now(),
      },
    }),
  );
}

async function executeRender(r: AdjustmentRecipe, stage: PreviewStage) {
  if (!sessionId.value) return;
  inFlight = true;

  try {
    const frame = await desktop.renderPreview(sessionId.value, r, stage);
    paintPixels(frame, stage);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    error.value = `Preview render failed: ${msg}`;
    console.error('Preview render error:', err);
  } finally {
    inFlight = false;
    if (pending) {
      const next = pending;
      pending = null;
      executeRender(next.recipe, next.stage);
    }
  }
}

function requestRender(stage: PreviewStage) {
  if (!sessionId.value) return;
  const current = isBefore.value ? createIdentityRecipe() : { ...recipe.value };

  if (inFlight) {
    // Latest wins: replace pending request
    pending = { recipe: current, stage };
  } else {
    executeRender(current, stage);
  }
}

function scheduleSave() {
  if (isReadOnly.value) {
    saveStatus.value = 'Read-only: card media';
    return;
  }
  if (autosaveDisabled.value) {
    saveStatus.value = 'Autosave disabled (sidecar error)';
    return;
  }
  if (!sourcePath.value || !sessionId.value) return;

  if (saveDebounceTimer) {
    window.clearTimeout(saveDebounceTimer);
  }

  saveStatus.value = 'Saving…';
  saveDebounceTimer = window.setTimeout(async () => {
    saveDebounceTimer = undefined;
    try {
      await desktop.saveRecipe(sourcePath.value, recipe.value);
      saveStatus.value = 'Saved';
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      saveStatus.value = `Couldn't save: ${msg}`;
    }
  }, 300);
}

function onRecipeValueInput() {
  requestRender('Drag');
}

function onRecipeValueChange() {
  requestRender('Settle');
  scheduleSave();
}

function setLut(lut: LutRef | null) {
  recipe.value.lut = lut;
  onRecipeValueChange();
}

function setLutIntensity(val: number) {
  recipe.value.lut_intensity = val;
  onRecipeValueInput();
}

function onLutIntensityChange(val: number) {
  recipe.value.lut_intensity = val;
  onRecipeValueChange();
}

function setBefore(active: boolean) {
  if (isBefore.value === active) return;
  isBefore.value = active;
  requestRender('Settle');
}

function isTextInputFocused(): boolean {
  const active = document.activeElement;
  if (!active) return false;
  const tag = active.tagName.toLowerCase();
  if (tag === 'textarea') return true;
  if (tag === 'input') {
    const type = (active as HTMLInputElement).type.toLowerCase();
    return (
      type === 'text' ||
      type === 'number' ||
      type === 'search' ||
      type === 'password' ||
      type === 'email'
    );
  }
  return false;
}

function handleKeyDown(e: KeyboardEvent) {
  if (e.key === '\\') {
    if (isTextInputFocused()) return;
    e.preventDefault();
    setBefore(true);
  }
}

function handleKeyUp(e: KeyboardEvent) {
  if (e.key === '\\') {
    if (isTextInputFocused()) return;
    e.preventDefault();
    setBefore(false);
  }
}

async function openImage(path: string) {
  if (!path.trim()) return;
  loading.value = true;
  error.value = null;
  saveStatus.value = '';
  exportStatus.value = null;
  exportError.value = null;
  autosaveDisabled.value = false;

  if (sessionId.value) {
    await desktop.closePreview(sessionId.value).catch(() => {});
    sessionId.value = null;
  }

  try {
    const info: OpenPreviewResult = await desktop.openPreview(path.trim());
    sessionId.value = info.session_id;
    orientation.value = info.orientation;
    isReadOnly.value = info.read_only;

    // Load existing sidecar recipe if present, distinguishing between no sidecar and load failure
    let existing: AdjustmentRecipe | null = null;
    let recipeLoadFailed = false;
    try {
      existing = await desktop.loadRecipe(path.trim());
    } catch (err: unknown) {
      recipeLoadFailed = true;
      const msg = err instanceof Error ? err.message : String(err);
      error.value = `Failed to load recipe sidecar: ${msg}`;
      saveStatus.value = `Autosave disabled: sidecar failed to load (${msg})`;
      autosaveDisabled.value = true;
    }

    if (existing) {
      const defaultHsl = createIdentityRecipe().hsl!;
      recipe.value = {
        ...createIdentityRecipe(),
        ...existing,
        curves: existing.curves
          ? {
              luma: existing.curves.luma ?? [{ x: 0, y: 0 }, { x: 1, y: 1 }],
              red: existing.curves.red ?? [{ x: 0, y: 0 }, { x: 1, y: 1 }],
              green: existing.curves.green ?? [{ x: 0, y: 0 }, { x: 1, y: 1 }],
              blue: existing.curves.blue ?? [{ x: 0, y: 0 }, { x: 1, y: 1 }],
            }
          : createIdentityRecipe().curves,
        hsl: existing.hsl
          ? {
              red: { ...defaultHsl.red, ...existing.hsl.red },
              orange: { ...defaultHsl.orange, ...existing.hsl.orange },
              yellow: { ...defaultHsl.yellow, ...existing.hsl.yellow },
              green: { ...defaultHsl.green, ...existing.hsl.green },
              aqua: { ...defaultHsl.aqua, ...existing.hsl.aqua },
              blue: { ...defaultHsl.blue, ...existing.hsl.blue },
              purple: { ...defaultHsl.purple, ...existing.hsl.purple },
              magenta: { ...defaultHsl.magenta, ...existing.hsl.magenta },
            }
          : defaultHsl,
      };
      saveStatus.value = 'Saved';
      autosaveDisabled.value = false;
    } else if (!recipeLoadFailed) {
      recipe.value = createIdentityRecipe();
      saveStatus.value = isReadOnly.value ? 'Read-only: card media' : '';
      autosaveDisabled.value = false;
    } else {
      recipe.value = createIdentityRecipe();
    }

    // Default export directory to image directory
    const parts = path.trim().split('/');
    parts.pop();
    exportDir.value = parts.join('/') || '/';

    // Initial render
    requestRender('Settle');
  } catch (err: unknown) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    loading.value = false;
  }
}

async function handleExport() {
  if (!sourcePath.value || !exportDir.value) return;
  exportStatus.value = null;
  exportError.value = null;

  try {
    const res = await desktop.exportEditedImage(
      sourcePath.value.trim(),
      recipe.value,
      exportDir.value.trim(),
    );
    exportStatus.value = res.path;
    if (res.metadata_skipped) {
      exportStatus.value += ` (Metadata skipped: ${res.metadata_skipped.reason})`;
    }
  } catch (err: unknown) {
    exportError.value = err instanceof Error ? err.message : String(err);
  }
}

function resetAll() {
  recipe.value = createIdentityRecipe();
  onRecipeValueChange();
}

function resetBasic() {
  recipe.value.exposure = 0.0;
  recipe.value.contrast = 0.0;
  recipe.value.highlights = 0.0;
  recipe.value.shadows = 0.0;
  recipe.value.whites = 0.0;
  recipe.value.blacks = 0.0;
  recipe.value.brightness = 0.0;
  recipe.value.temperature = 0.0;
  recipe.value.tint = 0.0;
  recipe.value.saturation = 0.0;
  recipe.value.vibrance = 0.0;
  recipe.value.hue = 0.0;
  onRecipeValueChange();
}

function resetToneCurve() {
  recipe.value.curves = {
    luma: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
    red: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
    green: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
    blue: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
  };
  onRecipeValueChange();
}

function onCurvesInput(curves: ToneCurves) {
  recipe.value.curves = curves;
  onRecipeValueInput();
}

function onCurvesChange(curves: ToneCurves) {
  recipe.value.curves = curves;
  onRecipeValueChange();
}

function isBandModified(key: keyof HslAdjustments): boolean {
  const b = recipe.value.hsl?.[key];
  if (!b) return false;
  return b.hue !== 0 || b.saturation !== 0 || b.luminance !== 0;
}

function selectHslBand(key: keyof HslAdjustments) {
  selectedHslBand.value = key;
}

function resetCurrentHslBand() {
  if (recipe.value.hsl) {
    recipe.value.hsl[selectedHslBand.value] = { hue: 0, saturation: 0, luminance: 0 };
    onRecipeValueChange();
  }
}

function resetHsl() {
  recipe.value.hsl = {
    red: { hue: 0, saturation: 0, luminance: 0 },
    orange: { hue: 0, saturation: 0, luminance: 0 },
    yellow: { hue: 0, saturation: 0, luminance: 0 },
    green: { hue: 0, saturation: 0, luminance: 0 },
    aqua: { hue: 0, saturation: 0, luminance: 0 },
    blue: { hue: 0, saturation: 0, luminance: 0 },
    purple: { hue: 0, saturation: 0, luminance: 0 },
    magenta: { hue: 0, saturation: 0, luminance: 0 },
  };
  onRecipeValueChange();
}

function onHslSliderInput(prop: 'hue' | 'saturation' | 'luminance', val: number) {
  if (!recipe.value.hsl) {
    recipe.value.hsl = createIdentityRecipe().hsl!;
  }
  recipe.value.hsl[selectedHslBand.value][prop] = val;
  onRecipeValueInput();
}

function onHslSliderChange(prop: 'hue' | 'saturation' | 'luminance', val: number) {
  if (!recipe.value.hsl) {
    recipe.value.hsl = createIdentityRecipe().hsl!;
  }
  recipe.value.hsl[selectedHslBand.value][prop] = val;
  onRecipeValueChange();
}

function resetGrading() {}

function resetLook() {
  recipe.value.lut = null;
  recipe.value.lut_intensity = 1.0;
  onRecipeValueChange();
}

function resetGeometry() {}

function resetEffects() {}

watch(sourcePath, (newPath) => {
  if (newPath) {
    openImage(newPath);
  }
});

onMounted(() => {
  refreshLuts();
  window.addEventListener('keydown', handleKeyDown);
  window.addEventListener('keyup', handleKeyUp);

  if (viewportContainerRef.value) {
    resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (entry.contentRect.width > 0 && entry.contentRect.height > 0) {
          containerWidth.value = entry.contentRect.width;
          containerHeight.value = entry.contentRect.height;
        }
      }
    });
    resizeObserver.observe(viewportContainerRef.value);
  }
});

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown);
  window.removeEventListener('keyup', handleKeyUp);

  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }

  // Flush pending save immediately on unmount (Item 7)
  if (saveDebounceTimer) {
    window.clearTimeout(saveDebounceTimer);
    saveDebounceTimer = undefined;
    if (!isReadOnly.value && !autosaveDisabled.value && sourcePath.value && sessionId.value) {
      desktop.saveRecipe(sourcePath.value, recipe.value).catch((err: unknown) => {
        console.error('Failed to flush recipe save on unmount:', err);
      });
    }
  }

  if (sessionId.value) {
    desktop.closePreview(sessionId.value).catch(() => {});
  }
});
</script>

<template>
  <div class="edit-screen">
    <!-- Top toolbar: image path input, loading indicator, and card safety banner -->
    <div class="edit-screen__toolbar">
      <PathField
        v-model="sourcePath"
        label="Photograph to edit"
        :roots="roots"
        :roots-error="rootsError"
        :list="listRoots"
        placeholder="/path/to/image.jpg"
        :selectable="['jpg', 'jpeg', 'tif', 'tiff', 'cr2', 'nef', 'arw', 'dng', 'png']"
      />

      <div v-if="loading" class="edit-screen__loading" data-testid="edit-loading">
        Loading photograph<span class="edit-screen__cursor">_</span>
      </div>

      <div v-if="error" class="error edit-screen__error" data-testid="edit-error">
        {{ error }}
      </div>

      <!-- Card Read-Only Banner (G5) -->
      <div v-if="isReadOnly" class="edit-screen__card-banner" data-testid="card-readonly-banner">
        Card media is read-only (G5). Copy files to a working folder to save edits.
      </div>
    </div>

    <!-- Workspace: canvas viewport on the left, adjustment panel on the right -->
    <div class="edit-screen__workspace">
      <!-- Photograph Canvas Viewport -->
      <div class="canvas-viewport" data-testid="canvas-viewport">
        <div ref="viewportContainerRef" class="canvas-viewport__container">
          <canvas
            ref="canvasRef"
            class="canvas-viewport__canvas"
            data-testid="edit-canvas"
            :style="canvasStyle"
          ></canvas>
        </div>

        <!-- Before / After toggle button -->
        <div class="canvas-viewport__overlay">
          <button
            type="button"
            class="secondary canvas-viewport__before-btn"
            data-testid="before-after-btn"
            :class="{ active: isBefore }"
            @pointerdown="setBefore(true)"
            @pointerup="setBefore(false)"
            @pointerleave="setBefore(false)"
          >
            {{ isBefore ? 'Before (Original)' : 'Before / After (Hold or \\)' }}
          </button>
        </div>
      </div>

      <!-- Side panel: Parametric adjustments & LUT grading -->
      <aside class="adjustment-panel">
        <div class="adjustment-panel__header">
          <h2 class="adjustment-panel__title">Adjustments</h2>
          <button
            type="button"
            class="ghost adjustment-panel__reset-btn"
            data-testid="reset-all-btn"
            @click="resetAll"
          >
            Reset all
          </button>
        </div>

        <div class="adjustment-panel__sections">
          <!-- 1. Basic -->
          <CollapsibleSection
            title="Basic"
            test-id="section-basic"
            :default-open="true"
            @reset="resetBasic"
          >
            <AdjustmentSlider
              v-model="recipe.exposure"
              label="Exposure"
              :min="-5.0"
              :max="5.0"
              :step="0.1"
              unit="EV"
              test-id="exposure"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.contrast"
              label="Contrast"
              :min="-100"
              :max="100"
              :step="1"
              test-id="contrast"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.highlights"
              label="Highlights"
              :min="-100"
              :max="100"
              :step="1"
              test-id="highlights"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.shadows"
              label="Shadows"
              :min="-100"
              :max="100"
              :step="1"
              test-id="shadows"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.whites"
              label="Whites"
              :min="-100"
              :max="100"
              :step="1"
              test-id="whites"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.blacks"
              label="Blacks"
              :min="-100"
              :max="100"
              :step="1"
              test-id="blacks"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.brightness"
              label="Brightness"
              :min="-100"
              :max="100"
              :step="1"
              test-id="brightness"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.temperature"
              label="Temperature"
              :min="-100"
              :max="100"
              :step="1"
              test-id="temperature"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.tint"
              label="Tint"
              :min="-100"
              :max="100"
              :step="1"
              test-id="tint"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.saturation"
              label="Saturation"
              :min="-100"
              :max="100"
              :step="1"
              test-id="saturation"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.vibrance"
              label="Vibrance"
              :min="-100"
              :max="100"
              :step="1"
              test-id="vibrance"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />

            <AdjustmentSlider
              v-model="recipe.hue"
              label="Hue"
              :min="-180"
              :max="180"
              :step="1"
              unit="°"
              test-id="hue"
              @update:model-value="onRecipeValueInput"
              @change="onRecipeValueChange"
            />
          </CollapsibleSection>

          <!-- 2. Tone Curve -->
          <CollapsibleSection
            title="Tone Curve"
            test-id="section-tone-curve"
            :default-open="false"
            @reset="resetToneCurve"
          >
            <CurveEditor
              :model-value="recipe.curves"
              test-id="tone-curve-editor"
              :disabled="loading || isReadOnly"
              @update:model-value="onCurvesInput"
              @change="onCurvesChange"
            />
          </CollapsibleSection>

          <!-- 3. Colour / HSL -->
          <CollapsibleSection
            title="Colour / HSL"
            test-id="section-hsl"
            :default-open="false"
            @reset="resetHsl"
          >
            <div class="hsl-panel" data-testid="hsl-panel">
              <div class="hsl-swatches" role="tablist" aria-label="HSL Colour Bands">
                <button
                  v-for="band in HSL_BANDS"
                  :key="band.key"
                  type="button"
                  class="hsl-swatch"
                  :class="{ active: selectedHslBand === band.key, modified: isBandModified(band.key) }"
                  :style="{ backgroundColor: `oklch(0.7 0.2 ${band.hueDeg}deg)` }"
                  :title="band.label"
                  :aria-label="band.label"
                  :aria-selected="selectedHslBand === band.key"
                  role="tab"
                  tabindex="0"
                  :data-testid="`hsl-swatch-${band.key}`"
                  @click="selectHslBand(band.key)"
                  @keydown.enter="selectHslBand(band.key)"
                  @keydown.space.prevent="selectHslBand(band.key)"
                >
                  <span
                    v-if="isBandModified(band.key)"
                    class="hsl-swatch-dot"
                    :data-testid="`hsl-dot-${band.key}`"
                    aria-hidden="true"
                  />
                </button>
              </div>

              <div class="hsl-controls">
                <div class="hsl-band-header">
                  <span class="hsl-band-title">{{ currentBandDef.label }}</span>
                  <button
                    type="button"
                    class="hsl-reset-band-btn"
                    :disabled="!isBandModified(selectedHslBand) || loading || isReadOnly"
                    data-testid="hsl-reset-band-btn"
                    @click="resetCurrentHslBand"
                  >
                    Reset {{ currentBandDef.label }}
                  </button>
                </div>

                <AdjustmentSlider
                  label="Hue"
                  :model-value="recipe.hsl?.[selectedHslBand]?.hue ?? 0"
                  :min="-180"
                  :max="180"
                  :step="1"
                  unit="°"
                  :disabled="loading || isReadOnly"
                  test-id="hsl-slider-hue"
                  @update:model-value="(v) => onHslSliderInput('hue', v)"
                  @change="(v) => onHslSliderChange('hue', v)"
                />

                <AdjustmentSlider
                  label="Saturation"
                  :model-value="recipe.hsl?.[selectedHslBand]?.saturation ?? 0"
                  :min="-100"
                  :max="100"
                  :step="1"
                  unit="%"
                  :disabled="loading || isReadOnly"
                  test-id="hsl-slider-saturation"
                  @update:model-value="(v) => onHslSliderInput('saturation', v)"
                  @change="(v) => onHslSliderChange('saturation', v)"
                />

                <AdjustmentSlider
                  label="Luminance"
                  :model-value="recipe.hsl?.[selectedHslBand]?.luminance ?? 0"
                  :min="-100"
                  :max="100"
                  :step="1"
                  unit="%"
                  :disabled="loading || isReadOnly"
                  test-id="hsl-slider-luminance"
                  @update:model-value="(v) => onHslSliderInput('luminance', v)"
                  @change="(v) => onHslSliderChange('luminance', v)"
                />
              </div>
            </div>
          </CollapsibleSection>

          <!-- 4. Colour Grading -->
          <CollapsibleSection
            title="Colour Grading"
            test-id="section-grading"
            :default-open="false"
            @reset="resetGrading"
          >
            <div class="section-placeholder">3-way colour grading available in ED-12</div>
          </CollapsibleSection>

          <!-- 5. Look -->
          <CollapsibleSection
            title="Look"
            test-id="section-look"
            :default-open="true"
            @reset="resetLook"
          >
            <LutPicker
              :model-value="recipe.lut ?? null"
              :intensity="recipe.lut_intensity ?? 1.0"
              :luts="lutList.luts"
              :errors="lutList.errors"
              :roots="roots"
              :roots-error="rootsError"
              :list="listRoots"
              @update:model-value="setLut"
              @update:intensity="setLutIntensity"
              @change="onLutIntensityChange"
              @import="handleImportLut"
            />
          </CollapsibleSection>

          <!-- 6. Geometry -->
          <CollapsibleSection
            title="Geometry"
            test-id="section-geometry"
            :default-open="false"
            @reset="resetGeometry"
          >
            <div class="section-placeholder">Crop, rotate, and straighten available in ED-13</div>
          </CollapsibleSection>

          <!-- 7. Effects -->
          <CollapsibleSection
            title="Effects"
            test-id="section-effects"
            :default-open="false"
            @reset="resetEffects"
          >
            <div class="section-placeholder">Vignette and film grain available in ED-15 & ED-16</div>
          </CollapsibleSection>
        </div>

        <!-- Save Status Line -->
        <div class="adjustment-panel__status" data-testid="save-status">
          <span v-if="saveStatus" :class="{ error: saveStatus.startsWith('Couldn\'t') || saveStatus.startsWith('Autosave disabled') }">
            {{ saveStatus }}
          </span>
        </div>

        <!-- Export Render Section -->
        <div class="adjustment-panel__export">
          <span class="adjustment-panel__group-title">Export Render</span>
          <PathField
            v-model="exportDir"
            label="Destination folder"
            :roots="roots"
            :roots-error="rootsError"
            :list="listRoots"
            placeholder="/path/to/output_folder"
          />

          <button
            type="button"
            class="primary adjustment-panel__export-btn"
            data-testid="export-btn"
            :disabled="!sessionId || !exportDir"
            @click="handleExport"
          >
            Export render
          </button>

          <div v-if="exportStatus" class="adjustment-panel__export-success" data-testid="export-success">
            Exported to: {{ exportStatus }}
          </div>

          <div v-if="exportError" class="error adjustment-panel__export-error" data-testid="export-error">
            {{ exportError }}
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.edit-screen {
  display: grid;
  grid-template-rows: auto 1fr;
  gap: var(--space-3);
  height: 100%;
  min-height: 0;
}

.edit-screen__toolbar {
  display: grid;
  gap: var(--space-2);
}

.edit-screen__loading {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--accent);
}

.edit-screen__cursor {
  animation: blink 1s steps(1) infinite;
}

.edit-screen__error {
  padding: var(--space-2);
  border: 1px solid var(--danger);
}

.edit-screen__card-banner {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.05em;
  padding: var(--space-2) var(--space-3);
  background: rgba(255, 179, 71, 0.1);
  border: 1px solid var(--amber);
  color: var(--amber);
}

.edit-screen__workspace {
  display: grid;
  grid-template-columns: 1fr 340px;
  gap: var(--space-4);
  min-height: 540px;
  height: 100%;
}

/* --- Canvas Viewport (§1.1 isolated surround above scanlines) ------------ */

.canvas-viewport {
  display: grid;
  grid-template-rows: 1fr auto;
  background: var(--canvas-surround);
  border: var(--border-hair);
  min-height: 480px;
  overflow: hidden;
}

.canvas-viewport__container {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  overflow: hidden;
  padding: var(--space-4);
}

.canvas-viewport__canvas {
  object-fit: contain;
  position: relative;
  z-index: var(--z-canvas);
  image-rendering: auto;
}

.canvas-viewport__overlay {
  display: flex;
  justify-content: center;
  padding: var(--space-2);
  background: rgba(10, 10, 15, 0.6);
}

.canvas-viewport__before-btn {
  min-height: 40px;
  padding: var(--space-2) var(--space-4);
  font-size: 12px;
}

.canvas-viewport__before-btn.active {
  background: var(--accent);
  color: var(--void);
}

/* --- Adjustment Sidebar Panel ------------------------------------------- */

.adjustment-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-3);
  background: var(--bg-elevated);
  border: var(--border-hair);
  overflow-y: auto;
  max-height: calc(100vh - 180px);
}

.adjustment-panel__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.adjustment-panel__title {
  font-size: 20px;
  font-family: var(--font-display);
}

.adjustment-panel__reset-btn {
  min-height: 40px;
  padding: var(--space-1) var(--space-2);
  font-size: 12px;
}

.adjustment-panel__group {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.adjustment-panel__group-title {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.adjustment-panel__status {
  font-family: var(--font-label);
  font-size: 13px;
  min-height: 20px;
  color: var(--accent);
}

.adjustment-panel__export {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.adjustment-panel__export-btn {
  width: 100%;
  min-height: 44px;
}

.adjustment-panel__export-success {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--accent);
  word-break: break-all;
}

.adjustment-panel__export-error {
  font-size: 12px;
  word-break: break-all;
}

.section-placeholder {
  font-family: var(--font-body);
  font-size: 13px;
  color: var(--text-muted);
  font-style: italic;
  padding: var(--space-2) 0;
}

.hsl-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.hsl-swatches {
  display: grid;
  grid-template-columns: repeat(8, 1fr);
  gap: var(--space-1);
}

.hsl-swatch {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 40px;
  min-height: 40px;
  height: 40px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  transition: transform var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.hsl-swatch:hover {
  transform: translateY(-1px);
  border-color: var(--border-strong);
}

.hsl-swatch.active {
  border: var(--border-active);
}

.hsl-swatch-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-none);
  background-color: var(--text);
  border: 1px solid var(--bg);
}

.hsl-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-1);
}

.hsl-band-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 32px;
}

.hsl-band-title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.hsl-reset-band-btn {
  font-family: var(--font-label);
  font-size: 12px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: var(--space-1) var(--space-2);
  min-height: 32px;
  border-radius: var(--radius-none);
  transition: color var(--dur-fast) var(--ease);
}

.hsl-reset-band-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.hsl-reset-band-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
