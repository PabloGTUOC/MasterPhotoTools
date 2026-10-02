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
  ColorWheel as ColorWheelType,
  FilmGrain,
  Geometry,
  HslAdjustments,
  LutLibraryList,
  LocalAdjustments,
  LutRef,
  Mask,
  MaskKind,
  NormalizedCrop,
  PresetList,
  OpenPreviewResult,
  PreviewHistogram,
  PreviewStage,
  ToneCurves,
  Vignette,
  LookEffects,
} from '@host/api';
import { desktop } from '@host/api';
import AdjustmentSlider from '@ui/components/AdjustmentSlider.vue';
import CollapsibleSection from '@ui/components/CollapsibleSection.vue';
import ColorWheel from '@ui/components/ColorWheel.vue';
import CropOverlay from '@ui/components/CropOverlay.vue';
import CurveEditor from '@ui/components/CurveEditor.vue';
import Histogram, { type HistogramMode } from '@ui/components/Histogram.vue';
import MaskOverlay from '@ui/components/MaskOverlay.vue';
import PresetBar from '@ui/components/PresetBar.vue';
import { copiedSettings } from '../recipeClipboard';
import LutPicker from '@ui/components/LutPicker.vue';
import PathField from '@ui/components/PathField.vue';
import { useRoots } from '@ui/useRoots';

const sourcePath = ref('');
const exportDir = ref('');
const sessionId = ref<string | null>(null);
const orientation = ref<number>(1);
const deliveredOrientation = ref<number>(1);
const isReadOnly = ref(false);
const autosaveDisabled = ref(false);
const loading = ref(false);
const error = ref<string | null>(null);
const saveStatus = ref<string>('');
const exportStatus = ref<string | null>(null);
const exportError = ref<string | null>(null);
const isBefore = ref(false);

const canvasRef = ref<HTMLCanvasElement | null>(null);
const clipCanvasRef = ref<HTMLCanvasElement | null>(null);

// ED-15: the histogram and clipping warnings describe the frame last painted.
const histogram = ref<PreviewHistogram | null>(null);
const histogramMode = ref<HistogramMode>('rgb');
const showShadowClip = ref(false);
const showHighlightClip = ref(false);
const clipOverlayActive = computed(() => showShadowClip.value || showHighlightClip.value);
let lastFrame: { width: number; height: number; pixels: Uint8ClampedArray } | null = null;

// ED-20: masks. The frame's map to the stored frame comes with every frame from core.
type Affine = [number, number, number, number, number, number];
const frameToStored = ref<Affine>([1, 0, 0, 0, 1, 0]);
const selectedMaskId = ref<string | null>(null);
const showMaskOverlay = ref(false);
const maskCanvasRef = ref<HTMLCanvasElement | null>(null);
/** The four-mask speed budget (edit plan, Round 3): each mask costs every frame. */
const MASK_LIMIT = 4;
let maskSeq = 0;
let coverageSeq = 0;
let lastRendered: { recipe: AdjustmentRecipe; stage: PreviewStage } | null = null;

// ED-17: presets, copied settings, and the one step back from replacing a look.
const presetList = ref<PresetList>({ presets: [], errors: [] });
const presetNames = computed(() => presetList.value.presets.map((p) => p.name));
const selectedPreset = ref<string | null>(null);
const notice = ref<{ text: string; undo: AdjustmentRecipe | null } | null>(null);
let noticeTimer: number | undefined;
const viewportContainerRef = ref<HTMLDivElement | null>(null);

// Container dimensions for responsive fit
const containerWidth = ref(0);
const containerHeight = ref(0);
const frameWidth = ref(1280);
const frameHeight = ref(854);
const baseProxyWidth = ref(1280);
const baseProxyHeight = ref(854);
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

// Colour Grading definitions (ED-12)
type GradingWheelKey = 'shadows' | 'midtones' | 'highlights' | 'global';

interface GradingWheelDef {
  key: GradingWheelKey;
  label: string;
}

const GRADING_WHEELS: GradingWheelDef[] = [
  { key: 'shadows', label: 'Shadows' },
  { key: 'midtones', label: 'Midtones' },
  { key: 'highlights', label: 'Highlights' },
  { key: 'global', label: 'Global' },
];

const selectedGradingWheel = ref<GradingWheelKey>('shadows');

const currentGradingWheelDef = computed(() => {
  return GRADING_WHEELS.find((w) => w.key === selectedGradingWheel.value) ?? GRADING_WHEELS[0];
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
    grading: {
      shadows: { hue: 0, saturation: 0, luminance: 0 },
      midtones: { hue: 0, saturation: 0, luminance: 0 },
      highlights: { hue: 0, saturation: 0, luminance: 0 },
      global: { hue: 0, saturation: 0, luminance: 0 },
      blending: 50,
      balance: 0,
    },
    geometry: null,
    vignette: null,
    grain: null,
    looks: null,
    masks: [],
  };
}

const recipe = ref<AdjustmentRecipe>(createIdentityRecipe());

// Rendering discipline state
let inFlight = false;
let pending: { recipe: AdjustmentRecipe; stage: PreviewStage } | null = null;
let saveDebounceTimer: number | undefined;


// In ED-13: The view rotates by the delivered orientation of the frame it is painting, and nothing else.
const effectiveOrientation = computed(() => deliveredOrientation.value);

// Whether orientation swaps width and height
const swapsAxes = computed(() => [5, 6, 7, 8].includes(effectiveOrientation.value));

// Orientation CSS transform (checked against EXIF standard 1-8)
const canvasTransform = computed(() => {
  switch (effectiveOrientation.value) {
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
const displayedDimensions = computed(() => {
  const cw = containerWidth.value;
  const ch = containerHeight.value;
  const fw = frameWidth.value || 1280;
  const fh = frameHeight.value || 854;

  if (cw <= 0 || ch <= 0) {
    return { width: fw, height: fh };
  }

  if (swapsAxes.value) {
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
    return { width: Math.round(vH), height: Math.round(vW) };
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
    return { width: Math.round(lW), height: Math.round(lH) };
  }
});

const canvasStyle = computed(() => {
  const { width, height } = displayedDimensions.value;
  return {
    width: `${width}px`,
    height: `${height}px`,
    transform: canvasTransform.value,
    position: 'relative' as const,
    zIndex: 'var(--z-canvas)',
  };
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

/** Reads a colour token as RGB bytes, by letting a canvas normalise whatever CSS form it has. */
function tokenRgb(name: string): [number, number, number] {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const probe = document.createElement('canvas').getContext('2d');
  if (!probe || !value) return [0, 0, 0];
  probe.fillStyle = value;
  const hex = probe.fillStyle;
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  return m ? [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)] : [0, 0, 0];
}

/**
 * Paints the clipping warning on its own canvas over the photograph.
 *
 * The photograph's canvas is never drawn on, so turning the warning off cannot
 * leave a mark, and nothing here reaches the recipe or an export. A pixel counts
 * as clipped when any channel is at 0 or 255, the rule `core` counts by, so the
 * overlay and the percentages under the histogram always agree. Where a pixel is
 * both (a saturated colour), the highlight warning wins: blown highlights are the
 * loss a photographer can least recover.
 */
function paintClipOverlay() {
  const overlay = clipCanvasRef.value;
  if (!overlay || !lastFrame || !clipOverlayActive.value) return;
  const { width, height, pixels } = lastFrame;
  if (overlay.width !== width || overlay.height !== height) {
    overlay.width = width;
    overlay.height = height;
  }
  const ctx = overlay.getContext('2d');
  if (!ctx) return;

  const [hr, hg, hb] = tokenRgb('--clip-highlight');
  const [sr, sg, sb] = tokenRgb('--clip-shadow');
  const hi = showHighlightClip.value;
  const lo = showShadowClip.value;
  const out = new Uint8ClampedArray(pixels.length);
  for (let i = 0; i < pixels.length; i += 4) {
    const r = pixels[i];
    const g = pixels[i + 1];
    const b = pixels[i + 2];
    if (hi && (r === 255 || g === 255 || b === 255)) {
      out[i] = hr;
      out[i + 1] = hg;
      out[i + 2] = hb;
      out[i + 3] = 255;
    } else if (lo && (r === 0 || g === 0 || b === 0)) {
      out[i] = sr;
      out[i + 1] = sg;
      out[i + 2] = sb;
      out[i + 3] = 255;
    }
  }
  ctx.putImageData(new ImageData(out, width, height), 0, 0);
}

watch([showShadowClip, showHighlightClip], paintClipOverlay);

function paintPixels(
  frame: {
    width: number;
    height: number;
    pixels: Uint8ClampedArray;
    orientation?: number;
    histogram?: PreviewHistogram;
    toStored?: Affine;
  },
  stage: PreviewStage,
) {
  const canvas = canvasRef.value;
  if (!canvas) return;

  if (frame.orientation !== undefined) {
    deliveredOrientation.value = frame.orientation;
  }

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

  histogram.value = frame.histogram ?? null;
  if (frame.toStored) frameToStored.value = frame.toStored;
  void refreshMaskCoverage();
  // Kept so the warning can be turned on over the frame already showing.
  lastFrame = { width: frame.width, height: frame.height, pixels: frame.pixels };
  paintClipOverlay();

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

  // In crop mode, render the uncropped upright image under the crop overlay
  let renderRecipe = r;
  if (isCropMode.value) {
    const g = r.geometry
      ? { ...r.geometry, crop: null }
      : { crop: null, rotate: 0, straighten: 0, flip_h: false, flip_v: false, aspect: 'original' };
    renderRecipe = {
      ...r,
      geometry: g,
    };
  }

  try {
    const frame = await desktop.renderPreview(sessionId.value, renderRecipe, stage);
    lastRendered = { recipe: renderRecipe, stage };
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

/** True when the person is working with text, where ⌘C and ⌘V must keep their usual meaning. */
function textIsInPlay(): boolean {
  if (isTextInputFocused()) return true;
  const selection = window.getSelection();
  return !!selection && !selection.isCollapsed && selection.toString().length > 0;
}

function handleKeyDown(e: KeyboardEvent) {
  const mod = e.metaKey || e.ctrlKey;
  if (mod && !e.shiftKey && !e.altKey && (e.key === 'c' || e.key === 'v')) {
    if (textIsInPlay() || !sessionId.value || document.querySelector('[role="dialog"]')) return;
    e.preventDefault();
    if (e.key === 'c') copySettings();
    else pasteSettings();
    return;
  }
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

/**
 * A recipe from disk, a preset or the clipboard, with every field the view binds
 * present: one written before a field existed leaves it out, and a slider bound
 * to a missing field shows nothing rather than its identity value.
 */
function normaliseRecipe(existing: AdjustmentRecipe, sourceSha: string): AdjustmentRecipe {
  const defaultHsl = createIdentityRecipe().hsl!;
  const defaultGrading = createIdentityRecipe().grading!;
  return {
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
    grading: existing.grading
      ? {
          shadows: { ...defaultGrading.shadows, ...existing.grading.shadows },
          midtones: { ...defaultGrading.midtones, ...existing.grading.midtones },
          highlights: { ...defaultGrading.highlights, ...existing.grading.highlights },
          global: { ...defaultGrading.global, ...existing.grading.global },
          blending: existing.grading.blending ?? defaultGrading.blending,
          balance: existing.grading.balance ?? defaultGrading.balance,
        }
      : defaultGrading,
    geometry: existing.geometry ? { ...existing.geometry } : null,
    vignette: existing.vignette
      ? {
          amount: existing.vignette.amount ?? 0,
          midpoint: existing.vignette.midpoint ?? 50,
          roundness: existing.vignette.roundness ?? 0,
          feather: existing.vignette.feather ?? 50,
        }
      : null,
    grain: existing.grain
      ? {
          amount: existing.grain.amount ?? 0,
          size: existing.grain.size ?? 25,
          roughness: existing.grain.roughness ?? 50,
        }
      : null,
    looks: existing.looks
      ? {
          glow_amount: existing.looks.glow_amount ?? 0,
          glow_threshold: existing.looks.glow_threshold ?? 70,
          glow_radius: existing.looks.glow_radius ?? 30,
          halation_amount: existing.looks.halation_amount ?? 0,
          halation_threshold: existing.looks.halation_threshold ?? 80,
          halation_radius: existing.looks.halation_radius ?? 20,
          tone_mapper: existing.looks.tone_mapper ?? null,
        }
      : null,
    masks: (existing.masks ?? []).map(normaliseMask),
    source_sha256: existing.source_sha256 || sourceSha,
  };
}

const ZERO_LOCAL: LocalAdjustments = {
  exposure: 0,
  contrast: 0,
  highlights: 0,
  shadows: 0,
  whites: 0,
  blacks: 0,
  temperature: 0,
  tint: 0,
  saturation: 0,
  vibrance: 0,
};

/** A mask from disk with every field the panel binds, as core's serde defaults fill them. */
function normaliseMask(m: Mask): Mask {
  const kind: MaskKind =
    m.kind.type === 'radial'
      ? { ...m.kind, angle: m.kind.angle ?? 0, feather: m.kind.feather ?? 0.5 }
      : { ...m.kind };
  return {
    id: m.id,
    name: m.name ?? '',
    kind,
    invert: m.invert ?? false,
    opacity: m.opacity ?? 1,
    enabled: m.enabled ?? true,
    adjustments: { ...ZERO_LOCAL, ...(m.adjustments ?? {}) },
  };
}

function cloneRecipe(r: AdjustmentRecipe): AdjustmentRecipe {
  return JSON.parse(JSON.stringify(r)) as AdjustmentRecipe;
}

function fileName(path: string): string {
  return path.trim().split('/').pop() || path;
}

function showNotice(text: string, undo: AdjustmentRecipe | null = null) {
  window.clearTimeout(noticeTimer);
  notice.value = { text, undo };
  // Long enough to read and reach Undo; a notice that stays would cover the viewport bar.
  noticeTimer = window.setTimeout(() => {
    notice.value = null;
  }, 8000);
}

function dismissNotice() {
  window.clearTimeout(noticeTimer);
  notice.value = null;
}

async function refreshPresets() {
  try {
    presetList.value = await desktop.listPresets();
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    error.value = `Failed to list presets: ${msg}`;
  }
}

/**
 * Replaces this photograph's look with another, keeping what is its own: its
 * framing (crop, rotation, straighten, flip) and the hash of its source. The
 * previous recipe is kept for Undo, because a preset applies on one click and a
 * paste on one keystroke, and neither would be safe to offer without a way back.
 */
function replaceLook(incoming: AdjustmentRecipe, text: string) {
  const before = cloneRecipe(recipe.value);
  const next = normaliseRecipe(cloneRecipe(incoming), before.source_sha256 ?? '');
  next.geometry = before.geometry;
  // Masks are this photograph's too, placed on its content (ED-19): never replaced by
  // another photograph's settings or a preset's.
  next.masks = before.masks;
  next.source_sha256 = before.source_sha256;
  recipe.value = next;
  onRecipeValueChange();
  showNotice(text, before);
}

function undoReplace() {
  const previous = notice.value?.undo;
  if (!previous) return;
  recipe.value = previous;
  onRecipeValueChange();
  showNotice('Undone.');
}

async function applyPreset(name: string) {
  if (!sessionId.value) return;
  try {
    const preset = await desktop.loadPreset(name);
    selectedPreset.value = name;
    replaceLook(preset, `Applied preset ${name}. Crop and rotation kept.`);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    error.value = `Couldn't apply preset ${name}: ${msg}`;
  }
}

async function savePreset(name: string, overwrite: boolean) {
  await desktop.savePreset(name, recipe.value, overwrite);
  await refreshPresets();
  selectedPreset.value = name;
  showNotice(overwrite ? `Replaced preset ${name}.` : `Saved preset ${name}.`);
}

async function renamePreset(from: string, to: string) {
  await desktop.renamePreset(from, to);
  await refreshPresets();
  if (selectedPreset.value === from) selectedPreset.value = to;
  showNotice(`Renamed preset ${from} to ${to}.`);
}

async function deletePreset(name: string) {
  await desktop.deletePreset(name);
  await refreshPresets();
  if (selectedPreset.value === name) selectedPreset.value = null;
  showNotice(`Deleted preset ${name}.`);
}

function copySettings() {
  if (!sessionId.value) return;
  copiedSettings.value = { recipe: cloneRecipe(recipe.value), from: fileName(sourcePath.value) };
  showNotice(`Copied settings from ${copiedSettings.value.from}.`);
}

function pasteSettings() {
  const copied = copiedSettings.value;
  if (!copied || !sessionId.value) return;
  selectedPreset.value = null;
  replaceLook(copied.recipe, `Pasted settings from ${copied.from}. Crop and rotation kept.`);
}

async function openImage(path: string) {
  if (!path.trim()) return;
  // Undo belongs to the photograph it was offered on.
  dismissNotice();
  selectedPreset.value = null;
  selectedMaskId.value = null;
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
    deliveredOrientation.value = info.orientation;
    isReadOnly.value = info.read_only;
    baseProxyWidth.value = info.settle[0];
    baseProxyHeight.value = info.settle[1];

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
      recipe.value = normaliseRecipe(existing, info.source_sha256 ?? '');
      isCropMode.value = false;
      saveStatus.value = 'Saved';
      autosaveDisabled.value = false;
    } else if (!recipeLoadFailed) {
      recipe.value = createIdentityRecipe(info.source_sha256 ?? '');
      isCropMode.value = false;
      saveStatus.value = isReadOnly.value ? 'Read-only: card media' : '';
      autosaveDisabled.value = false;
    } else {
      recipe.value = createIdentityRecipe(info.source_sha256 ?? '');
      isCropMode.value = false;
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
  const before = cloneRecipe(recipe.value);
  recipe.value = createIdentityRecipe(before.source_sha256 ?? '');
  selectedMaskId.value = null;
  onRecipeValueChange();
  // Reset all now also clears masks, which can be a lot of work: offer the way back.
  showNotice('Reset every adjustment and mask.', before);
}

// --- Masks (ED-20) ------------------------------------------------------------------

/** The locally adjustable set, in the global sliders' ranges (core's `LocalAdjustments`). */
const MASK_SLIDERS: {
  key: keyof LocalAdjustments;
  label: string;
  min: number;
  max: number;
  step: number;
  unit?: string;
}[] = [
  { key: 'exposure', label: 'Exposure', min: -5, max: 5, step: 0.1, unit: 'EV' },
  { key: 'contrast', label: 'Contrast', min: -100, max: 100, step: 1 },
  { key: 'highlights', label: 'Highlights', min: -100, max: 100, step: 1 },
  { key: 'shadows', label: 'Shadows', min: -100, max: 100, step: 1 },
  { key: 'whites', label: 'Whites', min: -100, max: 100, step: 1 },
  { key: 'blacks', label: 'Blacks', min: -100, max: 100, step: 1 },
  { key: 'temperature', label: 'Temperature', min: -100, max: 100, step: 1 },
  { key: 'tint', label: 'Tint', min: -100, max: 100, step: 1 },
  { key: 'saturation', label: 'Saturation', min: -100, max: 100, step: 1 },
  { key: 'vibrance', label: 'Vibrance', min: -100, max: 100, step: 1 },
];

const masks = computed<Mask[]>(() => recipe.value.masks ?? []);
const selectedMask = computed(() => masks.value.find((m) => m.id === selectedMaskId.value) ?? null);

/** Stored width and height over the long edge: the units mask shapes are measured in. */
const storedAspect = computed(() => {
  const w = baseProxyWidth.value || 1;
  const h = baseProxyHeight.value || 1;
  const long = Math.max(w, h);
  return { ax: w / long, ay: h / long };
});

/** A point given as a fraction of the frame on screen, in stored coordinates. */
function frameToStoredPoint(u: number, v: number): [number, number] {
  const [a, b, c, d, e, f] = frameToStored.value;
  return [a * u + b * v + c, d * u + e * v + f];
}

function addMask(type: 'linear' | 'radial') {
  if (!sessionId.value || masks.value.length >= MASK_LIMIT) return;
  const count = masks.value.filter((m) => m.kind.type === type).length + 1;
  // Placed by where they appear on screen, whatever the orientation or crop: a graduated
  // filter coming down from the top, a radial in the middle.
  const kind: MaskKind =
    type === 'linear'
      ? { type, start: frameToStoredPoint(0.5, 0.1), end: frameToStoredPoint(0.5, 0.55) }
      : {
          type,
          center: frameToStoredPoint(0.5, 0.5),
          radius_x: 0.2,
          radius_y: 0.2,
          angle: 0,
          feather: 0.5,
        };
  maskSeq += 1;
  const mask: Mask = {
    id: `m${Date.now().toString(36)}${maskSeq}`,
    name: `${type === 'linear' ? 'Linear' : 'Radial'} ${count}`,
    kind,
    invert: false,
    opacity: 1,
    enabled: true,
    adjustments: { ...ZERO_LOCAL },
  };
  recipe.value.masks = [...masks.value, mask];
  selectedMaskId.value = mask.id;
  onRecipeValueChange();
}

function updateMaskKind(id: string, kind: MaskKind) {
  const m = masks.value.find((x) => x.id === id);
  if (!m) return;
  m.kind = kind as MaskKind;
  requestRender('Drag');
}

function onMaskCommit() {
  onRecipeValueChange();
}

function deleteMask(id: string) {
  const m = masks.value.find((x) => x.id === id);
  if (!m) return;
  const before = cloneRecipe(recipe.value);
  recipe.value.masks = masks.value.filter((x) => x.id !== id);
  if (selectedMaskId.value === id) selectedMaskId.value = null;
  onRecipeValueChange();
  showNotice(`Deleted mask ${m.name}.`, before);
}

function resetMasks() {
  if (!masks.value.length) return;
  const before = cloneRecipe(recipe.value);
  recipe.value.masks = [];
  selectedMaskId.value = null;
  onRecipeValueChange();
  showNotice('Removed every mask.', before);
}

function setMaskLocal(key: keyof LocalAdjustments, value: number, settle: boolean) {
  const m = selectedMask.value;
  if (!m) return;
  m.adjustments[key] = value;
  if (settle) onRecipeValueChange();
  else requestRender('Drag');
}

function setMaskField(field: 'opacity' | 'feather', percent: number, settle: boolean) {
  const m = selectedMask.value;
  if (!m) return;
  if (field === 'opacity') m.opacity = percent / 100;
  else if (m.kind.type === 'radial') m.kind.feather = percent / 100;
  if (settle) onRecipeValueChange();
  else requestRender('Drag');
}

function toggleMask(id: string, field: 'enabled' | 'invert') {
  const m = masks.value.find((x) => x.id === id);
  if (!m) return;
  m[field] = !m[field];
  onRecipeValueChange();
}

function renameMask(id: string, name: string) {
  const m = masks.value.find((x) => x.id === id);
  if (!m || !name.trim()) return;
  m.name = name.trim();
  scheduleSave();
}

/**
 * Paints where the selected mask acts, from core's own coverage of the frame on screen,
 * on a canvas of its own over the photograph (the ED-15 rule: the photograph's canvas is
 * never drawn on). A newer request supersedes an older one still in flight.
 */
async function refreshMaskCoverage() {
  const canvas = maskCanvasRef.value;
  const id = selectedMaskId.value;
  if (!canvas || !showMaskOverlay.value || !id || !sessionId.value || !lastRendered) return;
  if (!lastRendered.recipe.masks?.some((m) => m.id === id)) return;
  const seq = ++coverageSeq;
  try {
    const cov = await desktop.renderMaskCoverage(
      sessionId.value,
      lastRendered.recipe,
      id,
      lastRendered.stage,
    );
    if (seq !== coverageSeq) return;
    if (canvas.width !== cov.width || canvas.height !== cov.height) {
      canvas.width = cov.width;
      canvas.height = cov.height;
    }
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const [r, g, b] = tokenRgb('--mask-overlay');
    const out = new Uint8ClampedArray(cov.width * cov.height * 4);
    for (let i = 0; i < cov.coverage.length; i++) {
      const a = cov.coverage[i];
      if (a === 0) continue;
      out[i * 4] = r;
      out[i * 4 + 1] = g;
      out[i * 4 + 2] = b;
      // Half strength at full coverage, so the photograph stays readable under it.
      out[i * 4 + 3] = a >> 1;
    }
    ctx.putImageData(new ImageData(out, cov.width, cov.height), 0, 0);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    error.value = `Couldn't show the mask: ${msg}`;
  }
}

watch([showMaskOverlay, selectedMaskId], () => {
  void refreshMaskCoverage();
});

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

function isGradingWheelModified(key: GradingWheelKey): boolean {
  const w = recipe.value.grading?.[key];
  if (!w) return false;
  return w.saturation !== 0 || w.luminance !== 0;
}

function selectGradingWheel(key: GradingWheelKey) {
  selectedGradingWheel.value = key;
}

function resetCurrentGradingWheel() {
  if (recipe.value.grading) {
    recipe.value.grading[selectedGradingWheel.value] = { hue: 0, saturation: 0, luminance: 0 };
    onRecipeValueChange();
  }
}

function resetGrading() {
  recipe.value.grading = {
    shadows: { hue: 0, saturation: 0, luminance: 0 },
    midtones: { hue: 0, saturation: 0, luminance: 0 },
    highlights: { hue: 0, saturation: 0, luminance: 0 },
    global: { hue: 0, saturation: 0, luminance: 0 },
    blending: 50,
    balance: 0,
  };
  onRecipeValueChange();
}

function onGradingWheelInput(val: ColorWheelType) {
  if (!recipe.value.grading) {
    recipe.value.grading = createIdentityRecipe().grading!;
  }
  recipe.value.grading[selectedGradingWheel.value] = val;
  onRecipeValueInput();
}

function onGradingWheelChange(val: ColorWheelType) {
  if (!recipe.value.grading) {
    recipe.value.grading = createIdentityRecipe().grading!;
  }
  recipe.value.grading[selectedGradingWheel.value] = val;
  onRecipeValueChange();
}

function onGradingParamInput(param: 'blending' | 'balance', val: number) {
  if (!recipe.value.grading) {
    recipe.value.grading = createIdentityRecipe().grading!;
  }
  recipe.value.grading[param] = val;
  onRecipeValueInput();
}

function onGradingParamChange(param: 'blending' | 'balance', val: number) {
  if (!recipe.value.grading) {
    recipe.value.grading = createIdentityRecipe().grading!;
  }
  recipe.value.grading[param] = val;
  onRecipeValueChange();
}

function ensureLooks(): LookEffects {
  if (!recipe.value.looks) {
    recipe.value.looks = {
      glow_amount: 0,
      glow_threshold: 70,
      glow_radius: 30,
      halation_amount: 0,
      halation_threshold: 80,
      halation_radius: 20,
      tone_mapper: null,
    };
  }
  return recipe.value.looks;
}

function onLooksSliderInput(prop: keyof LookEffects, val: number) {
  const l = ensureLooks();
  (l as any)[prop] = val;
  onRecipeValueInput();
}

function onLooksSliderChange(prop: keyof LookEffects, val: number) {
  const l = ensureLooks();
  (l as any)[prop] = val;
  onRecipeValueChange();
}

function setToneMapper(mode: 'none' | 'aces') {
  const l = ensureLooks();
  l.tone_mapper = mode === 'aces' ? 'aces' : null;
  onRecipeValueChange();
}

function resetLook() {
  recipe.value.lut = null;
  recipe.value.lut_intensity = 1.0;
  recipe.value.looks = null;
  onRecipeValueChange();
}

// Geometry state & methods (ED-13)
const isCropMode = ref(false);
let savedCropBeforeMode: NormalizedCrop | null = null;

function ensureGeometry(): Geometry {
  if (!recipe.value.geometry) {
    recipe.value.geometry = {
      crop: null,
      rotate: 0,
      straighten: 0,
      flip_h: false,
      flip_v: false,
      aspect: 'original',
    };
  }
  return recipe.value.geometry;
}

const cropModel = computed<NormalizedCrop>({
  get: () => {
    return recipe.value.geometry?.crop ?? { x: 0, y: 0, width: 1, height: 1 };
  },
  set: (val: NormalizedCrop) => {
    const g = ensureGeometry();
    g.crop = val;
  },
});

const straightenAngle = computed<number>({
  get: () => recipe.value.geometry?.straighten ?? 0,
  set: (val: number) => {
    const g = ensureGeometry();
    g.straighten = val;
  },
});

const isFlipH = computed<boolean>(() => recipe.value.geometry?.flip_h ?? false);
const isFlipV = computed<boolean>(() => recipe.value.geometry?.flip_v ?? false);

const selectedAspect = computed<string>({
  get: () => recipe.value.geometry?.aspect ?? 'original',
  set: (val: string) => {
    const g = ensureGeometry();
    g.aspect = val;
  },
});

function getPresetRatio(preset: string, uprightW: number, uprightH: number): number | null {
  switch (preset) {
    case 'original':
      return uprightW / uprightH;
    case 'free':
      return null;
    case '1:1':
      return 1.0;
    case '3:2':
      return 3.0 / 2.0;
    case '2:3':
      return 2.0 / 3.0;
    case '4:3':
      return 4.0 / 3.0;
    case '3:4':
      return 3.0 / 4.0;
    case '16:9':
      return 16.0 / 9.0;
    case '9:16':
      return 9.0 / 16.0;
    case '5:4':
      return 5.0 / 4.0;
    case '4:5':
      return 4.0 / 5.0;
    default:
      return null;
  }
}

function getUprightBaseDimensions(): { width: number; height: number } {
  const w = baseProxyWidth.value || 1280;
  const h = baseProxyHeight.value || 854;
  const exifSwaps = [5, 6, 7, 8].includes(orientation.value);
  const userSwaps = ((recipe.value.geometry?.rotate ?? 0) % 360 + 360) % 180 !== 0;
  const totalSwaps = exifSwaps !== userSwaps;
  if (totalSwaps) {
    return { width: h, height: w };
  }
  return { width: w, height: h };
}

const selectedAspectNumeric = computed<number | null>(() => {
  const g = recipe.value.geometry;
  const aspect = g?.aspect ?? 'original';
  const { width: uW, height: uH } = getUprightBaseDimensions();
  return getPresetRatio(aspect, uW, uH);
});

function calculatePresetCrop(preset: string): NormalizedCrop {
  const g = ensureGeometry();
  g.aspect = preset;
  const { width: uW, height: uH } = getUprightBaseDimensions();
  const ratio = getPresetRatio(preset, uW, uH);
  if (!ratio) {
    return g.crop ?? { x: 0, y: 0, width: 1, height: 1 };
  }
  const imgRatio = uW / uH;
  let normW: number;
  let normH: number;
  if (ratio > imgRatio) {
    normW = 1.0;
    normH = Math.max(16 / uH, (uW / ratio) / uH);
  } else {
    normH = 1.0;
    normW = Math.max(16 / uW, (uH * ratio) / uW);
  }
  const normX = (1.0 - normW) * 0.5;
  const normY = (1.0 - normH) * 0.5;
  return {
    x: Number(normX.toFixed(4)),
    y: Number(normY.toFixed(4)),
    width: Number(normW.toFixed(4)),
    height: Number(normH.toFixed(4)),
  };
}

function toggleCropMode() {
  if (isCropMode.value) {
    isCropMode.value = false;
    requestRender('Settle');
    scheduleSave();
  } else {
    const g = ensureGeometry();
    if (!g.crop) {
      g.crop = calculatePresetCrop(g.aspect || 'original');
    }
    savedCropBeforeMode = g.crop ? { ...g.crop } : null;
    isCropMode.value = true;
    requestRender('Drag');
  }
}

function applyCrop(crop: NormalizedCrop) {
  const g = ensureGeometry();
  g.crop = crop;
  isCropMode.value = false;
  requestRender('Settle');
  scheduleSave();
}

function cancelCrop() {
  const g = ensureGeometry();
  g.crop = savedCropBeforeMode;
  isCropMode.value = false;
  requestRender('Settle');
}

function onAspectChange() {
  const g = ensureGeometry();
  if (selectedAspect.value !== 'free') {
    g.crop = calculatePresetCrop(selectedAspect.value);
  }
  requestRender('Settle');
  scheduleSave();
}

function onStraightenInput() {
  requestRender('Drag');
}

function onStraightenChange() {
  requestRender('Settle');
  scheduleSave();
}

function rotateCW() {
  const g = ensureGeometry();
  g.rotate = (g.rotate + 90) % 360;
  requestRender('Settle');
  scheduleSave();
}

function rotateCCW() {
  const g = ensureGeometry();
  g.rotate = (g.rotate - 90 + 360) % 360;
  requestRender('Settle');
  scheduleSave();
}

function toggleFlipH() {
  const g = ensureGeometry();
  g.flip_h = !g.flip_h;
  requestRender('Settle');
  scheduleSave();
}

function toggleFlipV() {
  const g = ensureGeometry();
  g.flip_v = !g.flip_v;
  requestRender('Settle');
  scheduleSave();
}

function resetGeometry() {
  if (recipe.value.geometry) {
    recipe.value.geometry = null;
    isCropMode.value = false;
    requestRender('Settle');
    scheduleSave();
  }
}

function ensureVignette(): Vignette {
  if (!recipe.value.vignette) {
    recipe.value.vignette = {
      amount: 0,
      midpoint: 50,
      roundness: 0,
      feather: 50,
    };
  }
  return recipe.value.vignette;
}

function onVignetteSliderInput(prop: keyof Vignette, val: number) {
  const v = ensureVignette();
  v[prop] = val;
  onRecipeValueInput();
}

function onVignetteSliderChange(prop: keyof Vignette, val: number) {
  const v = ensureVignette();
  v[prop] = val;
  onRecipeValueChange();
}

function ensureGrain(): FilmGrain {
  if (!recipe.value.grain) {
    recipe.value.grain = {
      amount: 0,
      size: 25,
      roughness: 50,
    };
  }
  return recipe.value.grain;
}

function onGrainSliderInput(prop: keyof FilmGrain, val: number) {
  const g = ensureGrain();
  g[prop] = val;
  onRecipeValueInput();
}

function onGrainSliderChange(prop: keyof FilmGrain, val: number) {
  const g = ensureGrain();
  g[prop] = val;
  onRecipeValueChange();
}

function resetEffects() {
  recipe.value.vignette = null;
  recipe.value.grain = null;
  onRecipeValueChange();
}

watch(sourcePath, (newPath) => {
  if (newPath) {
    openImage(newPath);
  }
});

onMounted(() => {
  refreshLuts();
  refreshPresets();
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
  window.clearTimeout(noticeTimer);
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
          <div
            class="canvas-viewport__stage"
            :style="{
              width: `${displayedDimensions.width}px`,
              height: `${displayedDimensions.height}px`,
              position: 'relative',
            }"
          >
            <canvas
              ref="canvasRef"
              class="canvas-viewport__canvas"
              data-testid="edit-canvas"
              :style="canvasStyle"
            ></canvas>
            <canvas
              v-show="clipOverlayActive"
              ref="clipCanvasRef"
              class="canvas-viewport__clip"
              data-testid="clip-overlay"
              aria-hidden="true"
              :style="{ ...canvasStyle, position: 'absolute', zIndex: 'calc(var(--z-canvas) + 1)' }"
            ></canvas>
            <canvas
              v-show="showMaskOverlay && selectedMask && !isCropMode && !isBefore"
              ref="maskCanvasRef"
              class="canvas-viewport__clip"
              data-testid="mask-coverage"
              aria-hidden="true"
              :style="{ ...canvasStyle, position: 'absolute', zIndex: 'calc(var(--z-canvas) + 1)' }"
            ></canvas>
            <MaskOverlay
              v-if="sessionId && masks.length && !isCropMode && !isBefore"
              :masks="masks"
              :selected-id="selectedMaskId"
              :to-stored="frameToStored"
              :aspect="storedAspect"
              :frame-width="frameWidth"
              :frame-height="frameHeight"
              :style="{ ...canvasStyle, position: 'absolute', zIndex: 'calc(var(--z-canvas) + 2)' }"
              @select="(id) => (selectedMaskId = id)"
              @update="updateMaskKind"
              @commit="onMaskCommit"
            />
            <CropOverlay
              v-if="isCropMode"
              v-model="cropModel"
              :container-width="displayedDimensions.width"
              :container-height="displayedDimensions.height"
              :aspect-ratio="selectedAspectNumeric"
              @apply="applyCrop"
              @cancel="cancelCrop"
            />
          </div>
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
          <button
            type="button"
            class="secondary canvas-viewport__clip-btn canvas-viewport__clip-btn--shadow"
            data-testid="clip-shadow-btn"
            :class="{ active: showShadowClip }"
            :aria-pressed="showShadowClip"
            title="Show crushed shadows in blue"
            @click="showShadowClip = !showShadowClip"
          >
            <span
              class="canvas-viewport__clip-swatch"
              :class="{ lit: (histogram?.shadowClipped ?? 0) > 0 }"
              data-testid="clip-shadow-indicator"
            ></span>
            Shadow clipping
          </button>
          <button
            type="button"
            class="secondary canvas-viewport__clip-btn canvas-viewport__clip-btn--highlight"
            data-testid="clip-highlight-btn"
            :class="{ active: showHighlightClip }"
            :aria-pressed="showHighlightClip"
            title="Show blown highlights in red"
            @click="showHighlightClip = !showHighlightClip"
          >
            <span
              class="canvas-viewport__clip-swatch"
              :class="{ lit: (histogram?.highlightClipped ?? 0) > 0 }"
              data-testid="clip-highlight-indicator"
            ></span>
            Highlight clipping
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

        <PresetBar
          :presets="presetNames"
          :unreadable="presetList.errors"
          :selected="selectedPreset"
          :can-paste="copiedSettings !== null"
          :disabled="!sessionId"
          :save="savePreset"
          :rename="renamePreset"
          :remove="deletePreset"
          @apply="applyPreset"
          @copy="copySettings"
          @paste="pasteSettings"
        />

        <Histogram
          v-model:mode="histogramMode"
          class="adjustment-panel__histogram"
          :histogram="histogram"
        />

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

          <!-- 1b. Masks (ED-20) -->
          <CollapsibleSection
            title="Masks"
            test-id="section-masks"
            :default-open="false"
            @reset="resetMasks"
          >
            <div class="effects-panel" data-testid="masks-panel">
              <div class="mask-add">
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-linear-mask"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
                  @click="addMask('linear')"
                >
                  + Linear
                </button>
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-radial-mask"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
                  @click="addMask('radial')"
                >
                  + Radial
                </button>
              </div>
              <p v-if="masks.length >= MASK_LIMIT" class="mask-note" data-testid="mask-limit">
                Up to {{ MASK_LIMIT }} masks per photograph: each one costs every preview frame.
              </p>
              <p v-else-if="!masks.length" class="mask-note">
                A mask applies its own adjustments to part of the photograph. Masks stay with
                this photograph: presets, paste and Batch Grade never carry them.
              </p>

              <ul v-if="masks.length" class="mask-list" data-testid="mask-list">
                <li
                  v-for="m in masks"
                  :key="m.id"
                  class="mask-row"
                  :class="{ 'is-selected': m.id === selectedMaskId }"
                  :data-testid="`mask-row-${m.id}`"
                >
                  <button
                    type="button"
                    class="ghost mask-row__name"
                    :aria-pressed="m.id === selectedMaskId"
                    :data-testid="`mask-select-${m.id}`"
                    @click="selectedMaskId = m.id === selectedMaskId ? null : m.id"
                  >
                    {{ m.name }}
                  </button>
                  <button
                    type="button"
                    class="ghost mask-row__toggle"
                    :class="{ active: m.enabled }"
                    :aria-pressed="m.enabled"
                    :title="m.enabled ? 'Turn this mask off' : 'Turn this mask on'"
                    :data-testid="`mask-enable-${m.id}`"
                    @click="toggleMask(m.id, 'enabled')"
                  >
                    {{ m.enabled ? 'On' : 'Off' }}
                  </button>
                  <button
                    type="button"
                    class="ghost mask-row__toggle"
                    :class="{ active: m.invert }"
                    :aria-pressed="m.invert"
                    title="Apply outside the shape instead"
                    :data-testid="`mask-invert-${m.id}`"
                    @click="toggleMask(m.id, 'invert')"
                  >
                    Invert
                  </button>
                  <button
                    type="button"
                    class="ghost mask-row__delete"
                    :aria-label="`Delete mask ${m.name}`"
                    :data-testid="`mask-delete-${m.id}`"
                    @click="deleteMask(m.id)"
                  >
                    ×
                  </button>
                </li>
              </ul>

              <div v-if="selectedMask" class="effects-group" data-testid="mask-controls">
                <label class="mask-name">
                  <span class="effects-group-title">Name</span>
                  <input
                    type="text"
                    class="mask-name__input"
                    maxlength="40"
                    :value="selectedMask.name"
                    data-testid="mask-name-input"
                    @change="(e) => renameMask(selectedMask!.id, (e.target as HTMLInputElement).value)"
                  />
                </label>
                <button
                  type="button"
                  class="ghost mask-show"
                  :class="{ active: showMaskOverlay }"
                  :aria-pressed="showMaskOverlay"
                  data-testid="mask-show-overlay"
                  @click="showMaskOverlay = !showMaskOverlay"
                >
                  {{ showMaskOverlay ? 'Hide mask overlay' : 'Show mask overlay' }}
                </button>
                <AdjustmentSlider
                  label="Opacity"
                  :model-value="Math.round(selectedMask.opacity * 100)"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="100"
                  test-id="mask-opacity"
                  @update:model-value="(v) => setMaskField('opacity', v, false)"
                  @change="(v) => setMaskField('opacity', v, true)"
                />
                <AdjustmentSlider
                  v-if="selectedMask.kind.type === 'radial'"
                  label="Feather"
                  :model-value="Math.round(selectedMask.kind.feather * 100)"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="50"
                  test-id="mask-feather"
                  @update:model-value="(v) => setMaskField('feather', v, false)"
                  @change="(v) => setMaskField('feather', v, true)"
                />
                <AdjustmentSlider
                  v-for="s in MASK_SLIDERS"
                  :key="s.key"
                  :label="s.label"
                  :model-value="selectedMask.adjustments[s.key]"
                  :min="s.min"
                  :max="s.max"
                  :step="s.step"
                  :unit="s.unit"
                  :default-value="0"
                  :disabled="isReadOnly"
                  :test-id="`mask-${s.key}`"
                  @update:model-value="(v) => setMaskLocal(s.key, v, false)"
                  @change="(v) => setMaskLocal(s.key, v, true)"
                />
              </div>
            </div>
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
            <div class="grading-panel" data-testid="grading-panel">
              <!-- Wheel selector tabs -->
              <div class="grading-tabs" role="tablist" aria-label="Grading Wheels">
                <button
                  v-for="wheel in GRADING_WHEELS"
                  :key="wheel.key"
                  type="button"
                  class="grading-tab"
                  :class="{ active: selectedGradingWheel === wheel.key, modified: isGradingWheelModified(wheel.key) }"
                  :aria-label="wheel.label"
                  :aria-selected="selectedGradingWheel === wheel.key"
                  role="tab"
                  tabindex="0"
                  :data-testid="`grading-tab-${wheel.key}`"
                  @click="selectGradingWheel(wheel.key)"
                  @keydown.enter="selectGradingWheel(wheel.key)"
                  @keydown.space.prevent="selectGradingWheel(wheel.key)"
                >
                  <span class="grading-tab-label">{{ wheel.label }}</span>
                  <span
                    v-if="isGradingWheelModified(wheel.key)"
                    class="grading-tab-dot"
                    :data-testid="`grading-dot-${wheel.key}`"
                    aria-hidden="true"
                  />
                </button>
              </div>

              <!-- Active Wheel View -->
              <div class="grading-wheel-container">
                <div class="grading-wheel-header">
                  <span class="grading-wheel-title">{{ currentGradingWheelDef.label }} Wheel</span>
                  <button
                    type="button"
                    class="grading-reset-wheel-btn"
                    :disabled="!isGradingWheelModified(selectedGradingWheel) || loading || isReadOnly"
                    data-testid="grading-reset-wheel-btn"
                    @click="resetCurrentGradingWheel"
                  >
                    Reset {{ currentGradingWheelDef.label }}
                  </button>
                </div>

                <ColorWheel
                  :model-value="recipe.grading?.[selectedGradingWheel]"
                  :disabled="loading || isReadOnly"
                  :test-id="`grading-wheel-${selectedGradingWheel}`"
                  @update:model-value="onGradingWheelInput"
                  @change="onGradingWheelChange"
                />
              </div>

              <!-- Tonal Range Sliders: Blending and Balance -->
              <div class="grading-range-controls">
                <AdjustmentSlider
                  label="Blending"
                  :model-value="recipe.grading?.blending ?? 50"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :disabled="loading || isReadOnly"
                  test-id="grading-slider-blending"
                  @update:model-value="(v) => onGradingParamInput('blending', v)"
                  @change="(v) => onGradingParamChange('blending', v)"
                />

                <AdjustmentSlider
                  label="Balance"
                  :model-value="recipe.grading?.balance ?? 0"
                  :min="-100"
                  :max="100"
                  :step="1"
                  :disabled="loading || isReadOnly"
                  test-id="grading-slider-balance"
                  @update:model-value="(v) => onGradingParamInput('balance', v)"
                  @change="(v) => onGradingParamChange('balance', v)"
                />
              </div>
            </div>
          </CollapsibleSection>

          <!-- 5. Look -->
          <CollapsibleSection
            title="Look"
            test-id="section-look"
            :default-open="true"
            @reset="resetLook"
          >
            <div class="look-panel" data-testid="look-panel">
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

              <!-- Tone Mapper -->
              <div class="look-group">
                <div class="look-group-title">Tone Mapper</div>
                <div class="tone-mapper-toggle" data-testid="tone-mapper-toggle">
                  <button
                    type="button"
                    class="secondary tone-btn"
                    :class="{ active: !recipe.looks?.tone_mapper || recipe.looks?.tone_mapper === 'none' }"
                    data-testid="tone-mapper-plain"
                    :disabled="loading || isReadOnly"
                    @click="setToneMapper('none')"
                  >
                    Plain
                  </button>
                  <button
                    type="button"
                    class="secondary tone-btn"
                    :class="{ active: recipe.looks?.tone_mapper === 'aces' }"
                    data-testid="tone-mapper-filmic"
                    :disabled="loading || isReadOnly"
                    @click="setToneMapper('aces')"
                  >
                    Filmic
                  </button>
                </div>
                <div
                  v-if="recipe.looks?.tone_mapper === 'aces'"
                  class="tone-mapper-banner"
                  data-testid="tone-mapper-banner"
                >
                  Filmic tone curve on: highlights roll off and mid-tones shift. Turn it off to return to the plain curve.
                </div>
              </div>

              <!-- Glow -->
              <div class="look-group">
                <div class="look-group-title">Glow</div>
                <AdjustmentSlider
                  label="Amount"
                  :model-value="recipe.looks?.glow_amount ?? 0"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="glow-amount"
                  @update:model-value="(v) => onLooksSliderInput('glow_amount', v)"
                  @change="(v) => onLooksSliderChange('glow_amount', v)"
                />
                <AdjustmentSlider
                  label="Threshold"
                  :model-value="recipe.looks?.glow_threshold ?? 70"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="70"
                  :disabled="loading || isReadOnly"
                  test-id="glow-threshold"
                  @update:model-value="(v) => onLooksSliderInput('glow_threshold', v)"
                  @change="(v) => onLooksSliderChange('glow_threshold', v)"
                />
                <AdjustmentSlider
                  label="Radius"
                  :model-value="recipe.looks?.glow_radius ?? 30"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="30"
                  :disabled="loading || isReadOnly"
                  test-id="glow-radius"
                  @update:model-value="(v) => onLooksSliderInput('glow_radius', v)"
                  @change="(v) => onLooksSliderChange('glow_radius', v)"
                />
              </div>

              <!-- Halation -->
              <div class="look-group">
                <div class="look-group-title">Halation</div>
                <AdjustmentSlider
                  label="Amount"
                  :model-value="recipe.looks?.halation_amount ?? 0"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="halation-amount"
                  @update:model-value="(v) => onLooksSliderInput('halation_amount', v)"
                  @change="(v) => onLooksSliderChange('halation_amount', v)"
                />
                <AdjustmentSlider
                  label="Threshold"
                  :model-value="recipe.looks?.halation_threshold ?? 80"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="80"
                  :disabled="loading || isReadOnly"
                  test-id="halation-threshold"
                  @update:model-value="(v) => onLooksSliderInput('halation_threshold', v)"
                  @change="(v) => onLooksSliderChange('halation_threshold', v)"
                />
                <AdjustmentSlider
                  label="Radius"
                  :model-value="recipe.looks?.halation_radius ?? 20"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="20"
                  :disabled="loading || isReadOnly"
                  test-id="halation-radius"
                  @update:model-value="(v) => onLooksSliderInput('halation_radius', v)"
                  @change="(v) => onLooksSliderChange('halation_radius', v)"
                />
              </div>
            </div>
          </CollapsibleSection>

          <!-- 6. Geometry -->
          <CollapsibleSection
            title="Geometry"
            test-id="section-geometry"
            :default-open="false"
            @reset="resetGeometry"
          >
            <div class="geometry-panel" data-testid="geometry-panel">
              <!-- Crop mode toggle button -->
              <div class="geometry-toolbar">
                <button
                  type="button"
                  class="secondary geometry-btn geometry-crop-btn"
                  :class="{ active: isCropMode }"
                  data-testid="crop-mode-toggle-btn"
                  @click="toggleCropMode"
                >
                  {{ isCropMode ? 'Done Cropping' : 'Crop' }}
                </button>
              </div>

              <!-- Aspect ratio preset dropdown -->
              <div class="geometry-row">
                <label class="geometry-label" for="geometry-aspect-select">Aspect</label>
                <select
                  id="geometry-aspect-select"
                  v-model="selectedAspect"
                  class="geometry-select"
                  data-testid="geometry-aspect-select"
                  @change="onAspectChange"
                >
                  <option value="original">Original</option>
                  <option value="free">Free</option>
                  <option value="1:1">1:1 (Square)</option>
                  <option value="3:2">3:2 (35mm)</option>
                  <option value="2:3">2:3 (Portrait)</option>
                  <option value="4:3">4:3</option>
                  <option value="3:4">3:4</option>
                  <option value="16:9">16:9 (Widescreen)</option>
                  <option value="9:16">9:16 (Vertical)</option>
                  <option value="5:4">5:4</option>
                  <option value="4:5">4:5</option>
                </select>
              </div>

              <!-- Straighten slider -->
              <AdjustmentSlider
                v-model="straightenAngle"
                label="Straighten"
                :min="-45.0"
                :max="45.0"
                :step="0.1"
                unit="°"
                test-id="geometry-straighten"
                @update:model-value="onStraightenInput"
                @change="onStraightenChange"
              />

              <!-- Transform buttons: Rotate CCW, Rotate CW, Flip H, Flip V -->
              <div class="geometry-actions">
                <button
                  type="button"
                  class="secondary geometry-btn"
                  data-testid="rotate-ccw-btn"
                  title="Rotate 90° CCW"
                  @click="rotateCCW"
                >
                  ↺ -90°
                </button>
                <button
                  type="button"
                  class="secondary geometry-btn"
                  data-testid="rotate-cw-btn"
                  title="Rotate 90° CW"
                  @click="rotateCW"
                >
                  ↻ +90°
                </button>
                <button
                  type="button"
                  class="secondary geometry-btn"
                  :class="{ active: isFlipH }"
                  data-testid="flip-h-btn"
                  title="Flip Horizontal"
                  @click="toggleFlipH"
                >
                  ⇄ Flip H
                </button>
                <button
                  type="button"
                  class="secondary geometry-btn"
                  :class="{ active: isFlipV }"
                  data-testid="flip-v-btn"
                  title="Flip Vertical"
                  @click="toggleFlipV"
                >
                  ⇅ Flip V
                </button>
              </div>
            </div>
          </CollapsibleSection>

          <!-- 7. Effects -->
          <CollapsibleSection
            title="Effects"
            test-id="section-effects"
            :default-open="false"
            @reset="resetEffects"
          >
            <div class="effects-panel" data-testid="effects-panel">
              <div class="effects-group">
                <div class="effects-group-title">Vignette</div>
                <AdjustmentSlider
                  label="Amount"
                  :model-value="recipe.vignette?.amount ?? 0"
                  :min="-100"
                  :max="100"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="vignette-amount"
                  @update:model-value="(v) => onVignetteSliderInput('amount', v)"
                  @change="(v) => onVignetteSliderChange('amount', v)"
                />
                <AdjustmentSlider
                  label="Midpoint"
                  :model-value="recipe.vignette?.midpoint ?? 50"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="50"
                  :disabled="loading || isReadOnly"
                  test-id="vignette-midpoint"
                  @update:model-value="(v) => onVignetteSliderInput('midpoint', v)"
                  @change="(v) => onVignetteSliderChange('midpoint', v)"
                />
                <AdjustmentSlider
                  label="Roundness"
                  :model-value="recipe.vignette?.roundness ?? 0"
                  :min="-100"
                  :max="100"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="vignette-roundness"
                  @update:model-value="(v) => onVignetteSliderInput('roundness', v)"
                  @change="(v) => onVignetteSliderChange('roundness', v)"
                />
                <AdjustmentSlider
                  label="Feather"
                  :model-value="recipe.vignette?.feather ?? 50"
                  :min="0"
                  :max="100"
                  :step="1"
                  unit="%"
                  :default-value="50"
                  :disabled="loading || isReadOnly"
                  test-id="vignette-feather"
                  @update:model-value="(v) => onVignetteSliderInput('feather', v)"
                  @change="(v) => onVignetteSliderChange('feather', v)"
                />
              </div>

              <div class="effects-group">
                <div class="effects-group-title">Film Grain</div>
                <AdjustmentSlider
                  label="Amount"
                  :model-value="recipe.grain?.amount ?? 0"
                  :min="0"
                  :max="100"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="grain-amount"
                  @update:model-value="(v) => onGrainSliderInput('amount', v)"
                  @change="(v) => onGrainSliderChange('amount', v)"
                />
                <AdjustmentSlider
                  label="Size"
                  :model-value="recipe.grain?.size ?? 25"
                  :min="0"
                  :max="100"
                  :step="1"
                  :default-value="25"
                  :disabled="loading || isReadOnly"
                  test-id="grain-size"
                  @update:model-value="(v) => onGrainSliderInput('size', v)"
                  @change="(v) => onGrainSliderChange('size', v)"
                />
                <AdjustmentSlider
                  label="Roughness"
                  :model-value="recipe.grain?.roughness ?? 50"
                  :min="0"
                  :max="100"
                  :step="1"
                  :default-value="50"
                  :disabled="loading || isReadOnly"
                  test-id="grain-roughness"
                  @update:model-value="(v) => onGrainSliderInput('roughness', v)"
                  @change="(v) => onGrainSliderChange('roughness', v)"
                />
              </div>
            </div>
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
    <div v-if="notice" class="edit-notice" role="status" data-testid="edit-notice">
      <span class="edit-notice__text">{{ notice.text }}</span>
      <button
        v-if="notice.undo"
        type="button"
        class="ghost edit-notice__btn"
        data-testid="edit-notice-undo"
        @click="undoReplace"
      >
        Undo
      </button>
      <button
        type="button"
        class="ghost edit-notice__btn"
        aria-label="Dismiss"
        data-testid="edit-notice-dismiss"
        @click="dismissNotice"
      >
        ×
      </button>
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

/* Bottom right, clear of the viewport's controls, above everything but a modal. */
.edit-notice {
  position: fixed;
  right: var(--space-5);
  bottom: calc(var(--status-h) + var(--space-4));
  z-index: var(--z-toast);
  display: flex;
  align-items: center;
  gap: var(--space-2);
  max-width: min(520px, calc(100vw - 2 * var(--space-5)));
  padding: var(--space-2) var(--space-2) var(--space-2) var(--space-4);
  background: var(--bg-elevated);
  border: 1px solid var(--accent);
  border-radius: var(--radius-none);
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
}

.edit-notice__text {
  flex: 1;
}

.edit-notice__btn {
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-3);
  font-size: 13px;
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
  flex-wrap: wrap;
  justify-content: center;
  gap: var(--space-2);
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

.canvas-viewport__clip {
  top: 0;
  left: 0;
  pointer-events: none;
}

.canvas-viewport__clip-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-size: 12px;
}

.canvas-viewport__clip-btn.active {
  border: var(--border-active);
  color: var(--accent);
}

/* Hollow when the frame has no clipping of that kind, filled when it has: the
 * button says whether there is anything to see before it is pressed. */
.canvas-viewport__clip-swatch {
  width: 10px;
  height: 10px;
  border: 1px solid var(--text-muted);
  border-radius: var(--radius-none);
}

.canvas-viewport__clip-btn--shadow .canvas-viewport__clip-swatch.lit {
  background: var(--clip-shadow);
  border-color: var(--clip-shadow);
}

.canvas-viewport__clip-btn--highlight .canvas-viewport__clip-swatch.lit {
  background: var(--clip-highlight);
  border-color: var(--clip-highlight);
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

/* The histogram stays in view while the sliders below it scroll: it is read
 * while a slider moves, and one that scrolls away with the Basic section is not
 * there when Curves or Colour is being adjusted. The negative offset cancels the
 * panel's padding, so nothing shows through above it. */
.adjustment-panel__histogram {
  position: sticky;
  top: calc(-1 * var(--space-3));
  z-index: var(--z-content);
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

.grading-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.grading-tabs {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-1);
}

.grading-tab {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  min-height: 40px;
  height: 40px;
  padding: 0 var(--space-2);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.grading-tab:hover {
  color: var(--text-heading);
  border-color: var(--border-strong);
}

.grading-tab.active {
  color: var(--text-heading);
  border: var(--border-active);
  font-weight: 600;
}

.grading-tab-label {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.grading-tab-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-none);
  background-color: var(--text);
  border: 1px solid var(--bg);
  flex-shrink: 0;
}

.grading-wheel-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) 0;
  width: 100%;
}

.grading-wheel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  min-height: 40px;
  height: 40px;
}

.grading-wheel-title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.grading-reset-wheel-btn {
  font-family: var(--font-label);
  font-size: 12px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: var(--space-1) var(--space-2);
  min-height: 40px;
  height: 40px;
  border-radius: var(--radius-none);
  transition: color var(--dur-fast) var(--ease);
}

.grading-reset-wheel-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.grading-reset-wheel-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.grading-range-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-2);
  border-top: 1px solid var(--border);
  width: 100%;
}

/* --- Look Section (ED-16) ----------------------------------------------- */

.look-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.look-group {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.look-group-title {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.tone-mapper-toggle {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2);
}

.tone-btn {
  min-height: 40px;
  padding: var(--space-2);
  font-size: 13px;
  border-radius: var(--radius-pill);
}

.tone-btn.active {
  background: var(--accent);
  color: var(--void);
}

.tone-mapper-banner {
  padding: var(--space-2) var(--space-3);
  background: var(--bg-elevated);
  border-left: 2px solid var(--accent-warm);
  color: var(--text);
  font-family: var(--font-body);
  font-size: 11px;
  line-height: 1.4;
}

/* --- Geometry Section (ED-13) ------------------------------------------- */

.geometry-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}

.geometry-toolbar {
  display: flex;
  gap: var(--space-2);
}

.geometry-crop-btn {
  flex: 1;
  min-height: 40px;
  font-size: 13px;
  font-weight: 600;
  border-radius: 2px;
}

.geometry-crop-btn.active {
  background: var(--accent);
  color: var(--void);
}

.geometry-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.geometry-label {
  font-size: 13px;
  color: var(--text-muted);
}

.geometry-select {
  flex: 1;
  min-height: 40px;
  background: var(--bg-panel);
  border: var(--border-hair);
  color: var(--text-heading);
  padding: 0 var(--space-2);
  border-radius: 2px;
  font-size: 13px;
  outline: none;
}

.geometry-select:focus {
  border-color: var(--focus-ring);
}

.geometry-actions {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2);
}

.geometry-btn {
  min-height: 40px;
  padding: var(--space-2);
  font-size: 13px;
  border-radius: 2px;
}

.geometry-btn.active {
  background: var(--accent);
  color: var(--void);
}

.effects-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-1) 0;
}

.effects-group {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.mask-add {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-2);
}

.mask-add button {
  min-height: 40px;
  font-size: 13px;
}

.mask-note {
  margin: 0;
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
}

.mask-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: var(--space-1);
}

.mask-row {
  display: grid;
  grid-template-columns: 1fr auto auto 40px;
  gap: var(--space-1);
  border: var(--border-hair);
  background: var(--bg-panel);
}

.mask-row.is-selected {
  border: var(--border-active);
}

.mask-row button {
  min-height: 40px;
  padding: 0 var(--space-2);
  font-size: 12px;
}

.mask-row__name {
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mask-row__toggle.active {
  color: var(--accent);
}

.mask-name {
  display: grid;
  gap: var(--space-1);
}

.mask-name__input {
  min-height: 40px;
  font-size: 16px;
}

.mask-show {
  min-height: 40px;
  font-size: 12px;
}

.mask-show.active {
  color: var(--accent);
  border: var(--border-active);
}

.effects-group-title {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}
</style>
