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
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type {
  AdjustmentRecipe,
  AutoTarget,
  BlackAndWhite,
  ColorWheel as ColorWheelType,
  Detail,
  FilmGrain,
  Geometry,
  HslAdjustments,
  LutLibraryList,
  LocalAdjustments,
  LutRef,
  Mask,
  MaskComponent,
  MaskKind,
  MaskOp,
  NormalizedCrop,
  PointColour,
  PresetList,
  OpenPreviewResult,
  PreviewHistogram,
  PreviewStage,
  ToneCurves,
  Vignette,
  LookEffects,
  Snapshot,
  Version,
} from '@host/api';
import { desktop } from '@host/api';
import AdjustmentSlider from '../components/AdjustmentSlider.vue';
import CollapsibleSection from '../components/CollapsibleSection.vue';
import ColorWheel from '../components/ColorWheel.vue';
import CropOverlay from '../components/CropOverlay.vue';
import CurveEditor from '../components/CurveEditor.vue';
import Histogram, { type HistogramMode } from '../components/Histogram.vue';
import JobProgress from '@ui/components/JobProgress.vue';
import MaskOverlay, { type OverlayMask } from '../components/MaskOverlay.vue';
import PresetBar from '../components/PresetBar.vue';
import { copiedSettings, copyToClipboard } from '../recipeClipboard';
import { exportState, buildExportOptions, isResizeValid, resizeInvalidReason } from '../exportState';
import { EditHistory } from '../editHistory';
import LutPicker from '../components/LutPicker.vue';
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

// ED-21: the brush. Its settings belong to the tool, not to a mask, so they carry over
// from one mask to the next as a brush does in any paint program.
const painting = ref(false);
/** Radius as a percentage of the long edge. */
const brushSize = ref(4);
const brushFeather = ref(50);
const brushFlow = ref(100);
const brushErase = ref(false);
/** ⌥ held: erase while held, as in every darkroom application. */
const altHeld = ref(false);
const BRUSH_MIN = 0.5;

// ED-23: automatic masks. A model is downloaded once, with the person's say-so; a mask is
// a few seconds' work, which can be stopped (its result is then discarded).
const autoBusy = ref<AutoTarget | null>(null);
const autoDownload = ref<{ target: AutoTarget; name: string; jobId: string | null; asBackground?: boolean; componentForMaskId?: string } | null>(null);
let autoSeq = 0;
const AUTO_LABEL: Record<AutoTarget, string> = { subject: 'Subject', sky: 'Sky' };
const BRUSH_MAX = 25;
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

function createDefaultDetail(): Detail {
  return {
    sharpen_amount: 0,
    sharpen_radius: 1.0,
    sharpen_masking: 0,
    noise_luminance: 0,
    noise_colour: 0,
    hot_pixels: false,
    clarity: 0,
    texture: 0,
    dehaze: 0,
  };
}

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
    detail: createDefaultDetail(),
    black_and_white: null,
    point_colours: [],
    masks: [],
    snapshots: [],
    versions: [],
    version_name: null,
  };
}

const recipe = ref<AdjustmentRecipe>(createIdentityRecipe());

// ---------------------------------------------------------------------------
// History and Snapshots state (RP-5)
// ---------------------------------------------------------------------------

const history = new EditHistory();
const historyCursor = ref(-1);
const historyEntries = ref<Array<{ label: string }>>([]);
let lastCommittedRecipe: AdjustmentRecipe | null = null;

const reversedHistory = computed(() => {
  return historyEntries.value
    .map((item, index) => ({ label: item.label, index }))
    .reverse();
});

function syncHistoryState() {
  historyCursor.value = history.cursor;
  historyEntries.value = [...history.entries];
}

const newSnapshotName = ref('');
const snapshotMessage = ref('');

// ---------------------------------------------------------------------------
// Versions state (RP-6)
// ---------------------------------------------------------------------------

const currentVersionName = computed(() => recipe.value.version_name ?? 'Main');
const hasOtherVersions = computed(() => (recipe.value.versions?.length ?? 0) > 0);
const newVersionInput = ref('');
const versionMessage = ref('');
const editingCurrentVersion = ref(false);
const renameCurrentInput = ref('');
const editingVersionName = ref<string | null>(null);
const renameOtherInput = ref('');
const isExportingAll = ref(false);
const exportAllProgressText = ref('');
const exportAllSummary = ref('');

function suggestVersionName(): string {
  const existingNames = new Set<string>();
  if (recipe.value.version_name) {
    existingNames.add(recipe.value.version_name);
  } else {
    existingNames.add('Main');
  }
  for (const v of recipe.value.versions ?? []) {
    existingNames.add(v.name);
  }
  let i = 1;
  while (existingNames.has(`Copy ${i}`)) {
    i++;
  }
  return `Copy ${i}`;
}

// ---------------------------------------------------------------------------
// Colour tools: Eyedropper, Auto Tone, Black & White (RP-4), Point Colour (RP-10)
// ---------------------------------------------------------------------------

type EyedropperTarget =
  | 'white-balance'
  | 'point-colour'
  | 'colour-range-mask'
  | 'colour-range-repick'
  | 'colour-range-component'
  | null;
const eyedropperTarget = ref<EyedropperTarget>(null);
const isEyedropperActive = computed(() => eyedropperTarget.value !== null);

function toggleEyedropper() {
  if (isCropMode.value || isBefore.value || compareMode.value !== 'off' || !sessionId.value) return;
  eyedropperTarget.value = eyedropperTarget.value === 'white-balance' ? null : 'white-balance';
}

function togglePointColourEyedropper() {
  if (isCropMode.value || isBefore.value || compareMode.value !== 'off' || !sessionId.value) return;
  if ((recipe.value.point_colours?.length ?? 0) >= 8) return;
  eyedropperTarget.value = eyedropperTarget.value === 'point-colour' ? null : 'point-colour';
}

function startColourRangeEyedropper() {
  if (isCropMode.value || isBefore.value || compareMode.value !== 'off' || !sessionId.value) return;
  eyedropperTarget.value = 'colour-range-mask';
}

function startColourRangeRepick() {
  if (isCropMode.value || isBefore.value || compareMode.value !== 'off' || !sessionId.value) return;
  eyedropperTarget.value = 'colour-range-repick';
}

function startColourRangeComponentEyedropper() {
  if (isCropMode.value || isBefore.value || compareMode.value !== 'off' || !sessionId.value) return;
  eyedropperTarget.value = 'colour-range-component';
}

function exitEyedropper() {
  eyedropperTarget.value = null;
}

const selectedPointIndex = ref<number>(-1);

const selectedPoint = computed(() => {
  const points = recipe.value.point_colours;
  if (!points || selectedPointIndex.value < 0 || selectedPointIndex.value >= points.length) {
    return null;
  }
  return points[selectedPointIndex.value];
});

function selectPoint(idx: number) {
  selectedPointIndex.value = idx;
}

function deleteSelectedPoint() {
  const points = recipe.value.point_colours;
  if (!points || selectedPointIndex.value < 0 || selectedPointIndex.value >= points.length) return;
  points.splice(selectedPointIndex.value, 1);
  if (points.length === 0) {
    selectedPointIndex.value = -1;
  } else if (selectedPointIndex.value >= points.length) {
    selectedPointIndex.value = points.length - 1;
  }
  commitRecipeChange('Point colour');
}

function onPointColourSliderInput(prop: 'hue_shift' | 'saturation' | 'luminance' | 'range', val: number) {
  if (!selectedPoint.value) return;
  selectedPoint.value[prop] = val;
  onRecipeValueInput();
}

function onPointColourSliderChange(prop: 'hue_shift' | 'saturation' | 'luminance' | 'range', val: number) {
  if (!selectedPoint.value) return;
  selectedPoint.value[prop] = val;
  onRecipeValueChange();
}

function displayToFrameCoords(
  uDisp: number,
  vDisp: number,
  orientation: number,
  hasGeometry: boolean
): [number, number] {
  if (hasGeometry || orientation === 1) {
    return [uDisp, vDisp];
  }
  switch (orientation) {
    case 2:
      return [1.0 - uDisp, vDisp];
    case 3:
      return [1.0 - uDisp, 1.0 - vDisp];
    case 4:
      return [uDisp, 1.0 - vDisp];
    case 5:
      return [vDisp, uDisp];
    case 6:
      return [vDisp, 1.0 - uDisp];
    case 7:
      return [1.0 - vDisp, 1.0 - uDisp];
    case 8:
      return [1.0 - vDisp, uDisp];
    default:
      return [uDisp, vDisp];
  }
}

async function onEyedropperSample(e: MouseEvent | PointerEvent) {
  if (!eyedropperTarget.value || !sessionId.value) return;
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return;
  const uDisp = (e.clientX - rect.left) / rect.width;
  const vDisp = (e.clientY - rect.top) / rect.height;
  if (uDisp < 0 || uDisp > 1 || vDisp < 0 || vDisp > 1) {
    exitEyedropper();
    return;
  }
  const hasGeometry = Boolean(recipe.value.geometry);
  const [uFrame, vFrame] = displayToFrameCoords(
    uDisp,
    vDisp,
    effectiveOrientation.value,
    hasGeometry
  );
  const [a, b, c, d, ee, f] = frameToStored.value;
  const storedX = a * uFrame + b * vFrame + c;
  const storedY = d * uFrame + ee * vFrame + f;
  const target = eyedropperTarget.value;
  exitEyedropper();

  if (target === 'white-balance') {
    try {
      const pick = await desktop.whiteBalanceAt(sessionId.value, storedX, storedY);
      const before = JSON.parse(JSON.stringify(recipe.value));
      recipe.value.temperature = Math.round(pick.temperature);
      recipe.value.tint = Math.round(pick.tint);
      commitRecipeChange('White balance');
      const tempSign = pick.temperature > 0 ? '+' : '';
      const tintSign = pick.tint > 0 ? '+' : '';
      const clampedNote = pick.clamped ? ' (slider range reached)' : '';
      showNotice(
        `WB: Temp ${tempSign}${Math.round(pick.temperature)}, Tint ${tintSign}${Math.round(pick.tint)}${clampedNote}`,
        before,
      );
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showNotice(`Cannot balance here: ${msg}`);
    }
  } else if (target === 'point-colour') {
    try {
      const pick = await desktop.colourAt(sessionId.value, storedX, storedY);
      if (!recipe.value.point_colours) {
        recipe.value.point_colours = [];
      }
      if (recipe.value.point_colours.length >= 8) {
        showNotice('Cannot add more than 8 point colours');
        return;
      }
      const newPoint: PointColour = {
        hue: pick.hue,
        chroma: pick.chroma,
        lightness: pick.lightness,
        range: 50,
        hue_shift: 0,
        saturation: 0,
        luminance: 0,
      };
      recipe.value.point_colours.push(newPoint);
      selectedPointIndex.value = recipe.value.point_colours.length - 1;
      commitRecipeChange('Point colour');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showNotice(`Cannot pick colour: ${msg}`);
    }
  } else if (target === 'colour-range-mask') {
    try {
      const pick = await desktop.colourAt(sessionId.value, storedX, storedY);
      if (masks.value.length >= MASK_LIMIT) {
        showNotice(`Cannot add more than ${MASK_LIMIT} masks`);
        return;
      }
      maskSeq += 1;
      const count = masks.value.filter((m) => m.kind.type === 'colour_range').length + 1;
      const mask: Mask = {
        id: `m${Date.now().toString(36)}${maskSeq}`,
        name: `Colour Range ${count}`,
        kind: {
          type: 'colour_range',
          hue: pick.hue,
          chroma: pick.chroma,
          lightness: pick.lightness,
          range: 50,
        },
        invert: false,
        opacity: 1,
        enabled: true,
        adjustments: { ...ZERO_LOCAL },
      };
      recipe.value.masks = [...masks.value, mask];
      selectedMaskId.value = mask.id;
      selectedComponentIndex.value = null;
      showMaskOverlay.value = true;
      commitRecipeChange('Add mask');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showNotice(`Cannot pick colour: ${msg}`);
    }
  } else if (target === 'colour-range-repick') {
    try {
      const pick = await desktop.colourAt(sessionId.value, storedX, storedY);
      const m = selectedMask.value;
      if (!m) return;
      if (selectedComponentIndex.value !== null && m.components?.[selectedComponentIndex.value]) {
        const comp = m.components[selectedComponentIndex.value];
        if (comp.kind.type === 'colour_range') {
          comp.kind.hue = pick.hue;
          comp.kind.chroma = pick.chroma;
          comp.kind.lightness = pick.lightness;
          onRecipeValueChange();
        }
      } else if (m.kind.type === 'colour_range') {
        m.kind.hue = pick.hue;
        m.kind.chroma = pick.chroma;
        m.kind.lightness = pick.lightness;
        onRecipeValueChange();
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showNotice(`Cannot pick colour: ${msg}`);
    }
  } else if (target === 'colour-range-component') {
    try {
      const pick = await desktop.colourAt(sessionId.value, storedX, storedY);
      const m = selectedMask.value;
      if (!m) return;
      if (!m.components) {
        m.components = [];
      }
      m.components.push({
        op: 'intersect',
        kind: {
          type: 'colour_range',
          hue: pick.hue,
          chroma: pick.chroma,
          lightness: pick.lightness,
          range: 50,
        },
        invert: false,
      });
      selectedComponentIndex.value = m.components.length - 1;
      onRecipeValueChange();
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      showNotice(`Cannot pick colour: ${msg}`);
    }
  }
}

async function triggerAutoTone() {
  if (!sessionId.value || loading.value) return;
  try {
    const res = await desktop.autoTone(sessionId.value);
    const before = JSON.parse(JSON.stringify(recipe.value));
    recipe.value.exposure = res.exposure;
    recipe.value.whites = res.whites;
    recipe.value.blacks = res.blacks;
    commitRecipeChange('Auto');
    const expSign = res.exposure > 0 ? '+' : '';
    const wSign = res.whites > 0 ? '+' : '';
    const bSign = res.blacks > 0 ? '+' : '';
    showNotice(
      `Auto Tone: Exp ${expSign}${res.exposure.toFixed(2)} EV, Whites ${wSign}${res.whites.toFixed(1)}, Blacks ${bSign}${res.blacks.toFixed(1)}`,
      before,
    );
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    showNotice(`Auto Tone failed: ${msg}`);
  }
}

const BW_BANDS = [
  { key: 'red', label: 'Red' },
  { key: 'orange', label: 'Orange' },
  { key: 'yellow', label: 'Yellow' },
  { key: 'green', label: 'Green' },
  { key: 'aqua', label: 'Aqua' },
  { key: 'blue', label: 'Blue' },
  { key: 'purple', label: 'Purple' },
  { key: 'magenta', label: 'Magenta' },
] as const;

type BwBandKey = (typeof BW_BANDS)[number]['key'];

const isBwActive = computed(() => !!recipe.value.black_and_white);

const lastBwMix = ref<BlackAndWhite>({
  red: 0,
  orange: 0,
  yellow: 0,
  green: 0,
  aqua: 0,
  blue: 0,
  purple: 0,
  magenta: 0,
});

function toggleBlackAndWhite() {
  if (recipe.value.black_and_white) {
    lastBwMix.value = { ...recipe.value.black_and_white };
    recipe.value.black_and_white = null;
  } else {
    recipe.value.black_and_white = { ...lastBwMix.value };
  }
  commitRecipeChange('Black & white');
}

function getBwBand(key: BwBandKey): number {
  return recipe.value.black_and_white?.[key] ?? 0;
}

function onBwSliderInput(key: BwBandKey, val: number) {
  if (!recipe.value.black_and_white) {
    recipe.value.black_and_white = { ...lastBwMix.value };
  }
  recipe.value.black_and_white[key] = val;
  lastBwMix.value[key] = val;
  onRecipeValueInput();
}

function onBwSliderChange(key: BwBandKey, val: number) {
  if (!recipe.value.black_and_white) {
    recipe.value.black_and_white = { ...lastBwMix.value };
  }
  recipe.value.black_and_white[key] = val;
  lastBwMix.value[key] = val;
  onRecipeValueChange();
}

const isBwMixDefault = computed(() => {
  if (!recipe.value.black_and_white) return true;
  const bw = recipe.value.black_and_white;
  return (
    bw.red === 0 &&
    bw.orange === 0 &&
    bw.yellow === 0 &&
    bw.green === 0 &&
    bw.aqua === 0 &&
    bw.blue === 0 &&
    bw.purple === 0 &&
    bw.magenta === 0
  );
});

function resetBwMix() {
  if (recipe.value.black_and_white) {
    recipe.value.black_and_white = {
      red: 0,
      orange: 0,
      yellow: 0,
      green: 0,
      aqua: 0,
      blue: 0,
      purple: 0,
      magenta: 0,
    };
    lastBwMix.value = { ...recipe.value.black_and_white };
    onRecipeValueChange();
  }
}

// Viewing & zoom state (RP-3)
const zoom = ref<'fit' | number>('fit');
const pan = ref<{ x: number; y: number }>({ x: 0, y: 0 });
const fullWidth = ref(0);
const fullHeight = ref(0);
const dragWidth = ref(1280);
const dragHeight = ref(854);
const settleWidth = ref(2560);
const settleHeight = ref(1707);
const compareMode = ref<'off' | 'side-by-side' | 'split'>('off');
const splitDivider = ref(50);
const loadingFull = ref(false);
const spaceHeld = ref(false);
const isPanning = ref(false);
const paintedStage = ref<PreviewStage>('Settle');
const beforeCanvasRef = ref<HTMLCanvasElement | null>(null);
const sideBySideBeforeCanvasRef = ref<HTMLCanvasElement | null>(null);

// Rendering discipline state (RP-3 single queue for after and before)
let inFlightTask: 'after' | 'before' | null = null;
let pendingAfter: { recipe: AdjustmentRecipe; stage: PreviewStage } | null = null;
let pendingBefore: { recipe: AdjustmentRecipe; stage: PreviewStage } | null = null;
let lastBeforeRendered: { geometry: any; stage: PreviewStage } | null = null;
let lastBeforeFrame: {
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
  orientation?: number;
} | null = null;
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

function getProxyLongEdge(stage: PreviewStage): number {
  if (stage === 'Full') {
    return Math.max(fullWidth.value, fullHeight.value) || 6000;
  }
  if (stage === 'Drag') {
    return Math.max(dragWidth.value, dragHeight.value) || 1280;
  }
  return Math.max(settleWidth.value, settleHeight.value) || 2560;
}

function getVisualFullContentDimensions(): { width: number; height: number } {
  const fw = frameWidth.value || 1280;
  const fh = frameHeight.value || 854;
  const fullLongEdge = Math.max(fullWidth.value, fullHeight.value) || 6000;
  const proxyLongEdge = getProxyLongEdge(paintedStage.value);
  const multiplier = paintedStage.value === 'Full' ? 1.0 : fullLongEdge / proxyLongEdge;
  const fullContentW = fw * multiplier;
  const fullContentH = fh * multiplier;

  if (swapsAxes.value) {
    return {
      width: fullContentH,
      height: fullContentW,
    };
  }
  return {
    width: fullContentW,
    height: fullContentH,
  };
}

// Visual dimensions at 100% zoom (1 source px per device px, upright)
const visual100Dimensions = computed(() => {
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
  const { width, height } = getVisualFullContentDimensions();
  return {
    width: Math.round(width / dpr),
    height: Math.round(height / dpr),
  };
});

// Sizing so upright visual photograph fits inside viewport container without overflowing or clipping
const visualFitDimensions = computed(() => {
  const cw = compareMode.value === 'side-by-side'
    ? Math.floor(containerWidth.value / 2)
    : containerWidth.value;
  const ch = containerHeight.value;

  const vis100W = visual100Dimensions.value.width;
  const vis100H = visual100Dimensions.value.height;

  if (cw <= 0 || ch <= 0 || vis100W <= 0 || vis100H <= 0) {
    return { width: vis100W || 1280, height: vis100H || 854 };
  }

  const aVis = vis100W / vis100H;
  let vW: number;
  let vH: number;
  if (cw / ch > aVis) {
    vH = ch;
    vW = ch * aVis;
  } else {
    vW = cw;
    vH = cw / aVis;
  }
  return { width: Math.round(vW), height: Math.round(vH) };
});

// Stage dimensions multiplied directly (no CSS scale) so 40px overlay handles remain 40px
const stageDimensions = computed(() => {
  if (zoom.value === 'fit') {
    return visualFitDimensions.value;
  }
  const z = typeof zoom.value === 'number' ? zoom.value : 1.0;
  return {
    width: Math.round(visual100Dimensions.value.width * z),
    height: Math.round(visual100Dimensions.value.height * z),
  };
});

const displayedDimensions = stageDimensions;

function clampPan(
  x: number,
  y: number,
  stageW: number,
  stageH: number,
  contW: number,
  contH: number,
): { x: number; y: number } {
  let clampedX: number;
  let clampedY: number;

  if (stageW <= contW) {
    clampedX = Math.round((contW - stageW) / 2);
  } else {
    clampedX = Math.min(0, Math.max(contW - stageW, x));
  }

  if (stageH <= contH) {
    clampedY = Math.round((contH - stageH) / 2);
  } else {
    clampedY = Math.min(0, Math.max(contH - stageH, y));
  }

  return { x: clampedX, y: clampedY };
}

const clampedPan = computed(() => {
  const cw = compareMode.value === 'side-by-side'
    ? Math.floor(containerWidth.value / 2)
    : containerWidth.value;
  const ch = containerHeight.value;
  const sw = stageDimensions.value.width;
  const sh = stageDimensions.value.height;

  if (cw <= 0 || ch <= 0) return { x: 0, y: 0 };
  return clampPan(pan.value.x, pan.value.y, sw, sh, cw, ch);
});

const stageStyle = computed(() => {
  const { width, height } = stageDimensions.value;
  const { x, y } = clampedPan.value;
  return {
    width: `${width}px`,
    height: `${height}px`,
    left: `${x}px`,
    top: `${y}px`,
    position: 'absolute' as const,
  };
});

const canvasStyle = computed(() => {
  const { width: sw, height: sh } = stageDimensions.value;
  if (swapsAxes.value) {
    const left = (sw - sh) / 2;
    const top = (sh - sw) / 2;
    return {
      width: `${sh}px`,
      height: `${sw}px`,
      position: 'absolute' as const,
      left: `${left}px`,
      top: `${top}px`,
      transform: canvasTransform.value,
      transformOrigin: 'center center',
      zIndex: 'var(--z-canvas)',
    };
  }
  return {
    width: `${sw}px`,
    height: `${sh}px`,
    position: 'absolute' as const,
    left: '0px',
    top: '0px',
    transform: canvasTransform.value,
    transformOrigin: 'center center',
    zIndex: 'var(--z-canvas)',
  };
});

const zoomReadoutText = computed(() => {
  if (zoom.value === 'fit') {
    const sw = visual100Dimensions.value.width;
    if (sw <= 0) return 'Fit';
    const pct = Math.round((stageDimensions.value.width / sw) * 100);
    return `${pct}%`;
  }
  return `${Math.round((zoom.value as number) * 100)}%`;
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

  paintedStage.value = stage;
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

  if (stage === 'Full') {
    loadingFull.value = false;
  }

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

function paintBeforePixels(frame: {
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
  orientation?: number;
}) {
  const targets = [beforeCanvasRef.value, sideBySideBeforeCanvasRef.value].filter(
    Boolean,
  ) as HTMLCanvasElement[];
  for (const canvas of targets) {
    if (canvas.width !== frame.width || canvas.height !== frame.height) {
      canvas.width = frame.width;
      canvas.height = frame.height;
    }
    const ctx = canvas.getContext('2d');
    if (!ctx) continue;
    const imgData = new ImageData(frame.pixels, frame.width, frame.height);
    ctx.putImageData(imgData, 0, 0);
  }
}

function settleStageForCurrentZoom(): PreviewStage {
  if (zoom.value === 'fit') return 'Settle';
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
  const neededW = stageDimensions.value.width * dpr;
  const neededH = stageDimensions.value.height * dpr;

  const fullLongEdge = Math.max(fullWidth.value, fullHeight.value) || 6000;
  const settleLongEdge = Math.max(settleWidth.value, settleHeight.value) || 2560;
  const settleScale = settleLongEdge / fullLongEdge;

  const { width: visualFullW, height: visualFullH } = getVisualFullContentDimensions();
  const visualSettleW = visualFullW * settleScale;
  const visualSettleH = visualFullH * settleScale;

  if (neededW > visualSettleW || neededH > visualSettleH) {
    return 'Full';
  }
  return 'Settle';
}

function beforeRecipe(): AdjustmentRecipe {
  const id = createIdentityRecipe(recipe.value.source_sha256);
  if (recipe.value.geometry) {
    id.geometry = JSON.parse(JSON.stringify(recipe.value.geometry));
  }
  return id;
}

async function executeRender(r: AdjustmentRecipe, stage: PreviewStage) {
  if (!sessionId.value) return;

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
  }
}

async function executeBeforeRender(r: AdjustmentRecipe, stage: PreviewStage) {
  if (!sessionId.value) return;
  try {
    const frame = await desktop.renderPreview(sessionId.value, r, stage);
    lastBeforeRendered = {
      geometry: r.geometry ? JSON.parse(JSON.stringify(r.geometry)) : null,
      stage,
    };
    lastBeforeFrame = {
      width: frame.width,
      height: frame.height,
      pixels: frame.pixels,
      orientation: frame.orientation,
    };
    paintBeforePixels(frame);
  } catch (err: unknown) {
    console.error('Before render error:', err);
  }
}

async function processRenderQueue() {
  if (inFlightTask !== null || !sessionId.value) return;

  if (pendingBefore) {
    const task = pendingBefore;
    pendingBefore = null;
    inFlightTask = 'before';
    await executeBeforeRender(task.recipe, task.stage);
    inFlightTask = null;
    processRenderQueue();
    return;
  }

  if (pendingAfter) {
    const task = pendingAfter;
    pendingAfter = null;
    inFlightTask = 'after';
    await executeRender(task.recipe, task.stage);
    inFlightTask = null;
    processRenderQueue();
  }
}

function requestRender(stage: PreviewStage) {
  if (!sessionId.value) return;
  const effectiveStage = stage === 'Settle' ? settleStageForCurrentZoom() : stage;
  const current = isBefore.value
    ? createIdentityRecipe(recipe.value.source_sha256)
    : { ...recipe.value };
  pendingAfter = { recipe: current, stage: effectiveStage };

  if (compareMode.value !== 'off') {
    const geomStr = recipe.value.geometry ? JSON.stringify(recipe.value.geometry) : null;
    const lastGeomStr = lastBeforeRendered?.geometry
      ? JSON.stringify(lastBeforeRendered.geometry)
      : null;
    const stageDowngrade =
      effectiveStage === 'Drag' &&
      (lastBeforeRendered?.stage === 'Settle' || lastBeforeRendered?.stage === 'Full');
    const needsBefore =
      !lastBeforeRendered ||
      (!stageDowngrade && lastBeforeRendered.stage !== effectiveStage) ||
      geomStr !== lastGeomStr;

    if (needsBefore) {
      pendingBefore = { recipe: beforeRecipe(), stage: effectiveStage };
    }
  }

  if (effectiveStage === 'Full') {
    loadingFull.value = true;
  }

  processRenderQueue();
}

const ZOOM_STEPS = [0.25, 0.5, 1.0, 2.0, 4.0];

function checkStageOnZoomChange() {
  const targetStage = settleStageForCurrentZoom();
  if (targetStage !== lastRendered?.stage) {
    requestRender(targetStage);
  }
}

function setZoom(newZoom: 'fit' | number) {
  if (isCropMode.value) return;
  if (newZoom === 'fit') {
    zoom.value = 'fit';
    pan.value = { x: 0, y: 0 };
  } else {
    zoom.value = Math.max(0.25, Math.min(4.0, newZoom));
    const cw = compareMode.value === 'side-by-side'
      ? Math.floor(containerWidth.value / 2)
      : containerWidth.value;
    const ch = containerHeight.value;
    const sw = Math.round(visual100Dimensions.value.width * zoom.value);
    const sh = Math.round(visual100Dimensions.value.height * zoom.value);
    pan.value = clampPan(Math.round((cw - sw) / 2), Math.round((ch - sh) / 2), sw, sh, cw, ch);
  }
  checkStageOnZoomChange();
}

function stepZoom(direction: 1 | -1) {
  if (isCropMode.value) return;
  const fitScale = visualFitDimensions.value.width / visual100Dimensions.value.width;
  const currentScale = typeof zoom.value === 'number' ? zoom.value : fitScale;

  if (direction === 1) {
    const next = ZOOM_STEPS.find((s) => s > currentScale + 0.01);
    setZoom(next ?? 4.0);
  } else {
    const prev = [...ZOOM_STEPS].reverse().find((s) => s < currentScale - 0.01);
    if (!prev || prev <= fitScale) {
      setZoom('fit');
    } else {
      setZoom(prev);
    }
  }
}

function zoomAboutPoint(targetScale: number, cx: number, cy: number) {
  if (isCropMode.value) return;
  const oldW = stageDimensions.value.width;
  const oldH = stageDimensions.value.height;
  const oldPan = clampedPan.value;

  const u = (cx - oldPan.x) / oldW;
  const v = (cy - oldPan.y) / oldH;

  const newW = Math.round(visual100Dimensions.value.width * targetScale);
  const newH = Math.round(visual100Dimensions.value.height * targetScale);

  const desiredPanX = cx - u * newW;
  const desiredPanY = cy - v * newH;

  const cw = compareMode.value === 'side-by-side'
    ? Math.floor(containerWidth.value / 2)
    : containerWidth.value;
  const ch = containerHeight.value;

  pan.value = clampPan(desiredPanX, desiredPanY, newW, newH, cw, ch);
  zoom.value = targetScale;
  checkStageOnZoomChange();
}

function toggleCompare(mode: 'side-by-side' | 'split') {
  if (isCropMode.value) return;
  if (compareMode.value === mode) {
    compareMode.value = 'off';
  } else {
    compareMode.value = mode;
    requestRender(settleStageForCurrentZoom());
  }
}

let draggingDivider = false;

function startDividerDrag(e: PointerEvent) {
  e.preventDefault();
  draggingDivider = true;
  try {
    (e.currentTarget as HTMLElement)?.setPointerCapture?.(e.pointerId);
  } catch {}
  window.addEventListener('pointermove', onDividerPointerMove);
  window.addEventListener('pointerup', onDividerPointerUp);
  window.addEventListener('pointercancel', onDividerPointerUp);
}

function onDividerPointerMove(e: PointerEvent) {
  if (!draggingDivider) return;
  const stageEl = viewportContainerRef.value?.querySelector('.canvas-viewport__stage');
  if (!stageEl) return;
  const rect = stageEl.getBoundingClientRect();
  if (rect.width <= 0) return;
  const pct = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
  splitDivider.value = Math.round(pct * 10) / 10;
}

function onDividerPointerUp() {
  draggingDivider = false;
  window.removeEventListener('pointermove', onDividerPointerMove);
  window.removeEventListener('pointerup', onDividerPointerUp);
  window.removeEventListener('pointercancel', onDividerPointerUp);
}

let isDraggingPan = false;
let panStartPointer = { x: 0, y: 0 };
let panStartOffset = { x: 0, y: 0 };

function onViewportPointerDown(e: PointerEvent) {
  if (isEyedropperActive.value) {
    e.preventDefault();
    e.stopPropagation();
    onEyedropperSample(e);
    return;
  }
  if ((e.target as HTMLElement)?.closest('button, .canvas-viewport__split-divider, input')) {
    return;
  }
  const canPan = spaceHeld.value || (zoom.value !== 'fit' && !selectedMaskId.value && !painting.value);
  if (!canPan) return;

  isDraggingPan = true;
  isPanning.value = true;
  panStartPointer = { x: e.clientX, y: e.clientY };
  panStartOffset = { ...clampedPan.value };

  try {
    (e.currentTarget as HTMLElement)?.setPointerCapture?.(e.pointerId);
  } catch {}
  window.addEventListener('pointermove', onViewportPointerMove);
  window.addEventListener('pointerup', onViewportPointerUp);
  window.addEventListener('pointercancel', onViewportPointerUp);
}

function onViewportPointerMove(e: PointerEvent) {
  if (!isDraggingPan) return;
  const dx = e.clientX - panStartPointer.x;
  const dy = e.clientY - panStartPointer.y;
  const rawX = panStartOffset.x + dx;
  const rawY = panStartOffset.y + dy;

  const cw = compareMode.value === 'side-by-side'
    ? Math.floor(containerWidth.value / 2)
    : containerWidth.value;
  const ch = containerHeight.value;
  const sw = stageDimensions.value.width;
  const sh = stageDimensions.value.height;

  pan.value = clampPan(rawX, rawY, sw, sh, cw, ch);
}

function onViewportPointerUp() {
  if (!isDraggingPan) return;
  isDraggingPan = false;
  isPanning.value = false;
  window.removeEventListener('pointermove', onViewportPointerMove);
  window.removeEventListener('pointerup', onViewportPointerUp);
  window.removeEventListener('pointercancel', onViewportPointerUp);
}

function onViewportWheel(e: WheelEvent) {
  if (e.ctrlKey || e.metaKey) {
    e.preventDefault();
    const container = viewportContainerRef.value;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;

    const fitScale = visualFitDimensions.value.width / visual100Dimensions.value.width;
    const curScale = typeof zoom.value === 'number' ? zoom.value : fitScale;
    const factor = Math.exp(-e.deltaY * 0.005);
    const targetScale = Math.max(fitScale, Math.min(4.0, curScale * factor));

    if (Math.abs(targetScale - fitScale) < 0.001) {
      setZoom('fit');
    } else {
      zoomAboutPoint(targetScale, cx, cy);
    }
    return;
  }

  if (zoom.value !== 'fit') {
    e.preventDefault();
    const cw = compareMode.value === 'side-by-side'
      ? Math.floor(containerWidth.value / 2)
      : containerWidth.value;
    const ch = containerHeight.value;
    const sw = stageDimensions.value.width;
    const sh = stageDimensions.value.height;

    pan.value = clampPan(pan.value.x - e.deltaX, pan.value.y - e.deltaY, sw, sh, cw, ch);
  }
}

function onStageDoubleClick(e: MouseEvent) {
  if (isCropMode.value || selectedMaskId.value || painting.value) return;
  if (zoom.value === 'fit') {
    const container = viewportContainerRef.value;
    if (!container) {
      setZoom(1.0);
      return;
    }
    const rect = container.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    zoomAboutPoint(1.0, cx, cy);
  } else {
    setZoom('fit');
  }
}

watch(compareMode, async () => {
  await nextTick();
  if (lastFrame) paintPixels(lastFrame, lastRendered?.stage ?? 'Settle');
  if (lastBeforeFrame) paintBeforePixels(lastBeforeFrame);
});

function deriveLabel(before: AdjustmentRecipe | null, after: AdjustmentRecipe): string {
  if (!before) return 'Edit';
  const changed: string[] = [];

  if (before.exposure !== after.exposure) changed.push('Exposure');
  if (before.temperature !== after.temperature) changed.push('Temperature');
  if (before.tint !== after.tint) changed.push('Tint');
  if (before.contrast !== after.contrast) changed.push('Contrast');
  if (before.highlights !== after.highlights) changed.push('Highlights');
  if (before.shadows !== after.shadows) changed.push('Shadows');
  if (before.whites !== after.whites) changed.push('Whites');
  if (before.blacks !== after.blacks) changed.push('Blacks');
  if (before.vibrance !== after.vibrance) changed.push('Vibrance');
  if (before.saturation !== after.saturation) changed.push('Saturation');
  if (before.brightness !== after.brightness) changed.push('Brightness');
  if (before.hue !== after.hue) changed.push('Hue');

  if (JSON.stringify(before.lut) !== JSON.stringify(after.lut) || before.lut_intensity !== after.lut_intensity) {
    changed.push('LUT');
  }
  if (JSON.stringify(before.black_and_white) !== JSON.stringify(after.black_and_white)) {
    changed.push('Black & white');
  }
  if (JSON.stringify(before.curves) !== JSON.stringify(after.curves)) {
    changed.push('Tone curve');
  }
  if (JSON.stringify(before.hsl) !== JSON.stringify(after.hsl)) {
    changed.push('HSL');
  }
  if (JSON.stringify(before.grading) !== JSON.stringify(after.grading)) {
    changed.push('Color grading');
  }
  if (JSON.stringify(before.geometry) !== JSON.stringify(after.geometry)) {
    changed.push('Crop');
  }
  if (JSON.stringify(before.vignette) !== JSON.stringify(after.vignette)) {
    changed.push('Vignette');
  }
  if (JSON.stringify(before.grain) !== JSON.stringify(after.grain)) {
    changed.push('Grain');
  }
  if (JSON.stringify(before.looks) !== JSON.stringify(after.looks)) {
    changed.push('Looks');
  }
  if (JSON.stringify(before.detail) !== JSON.stringify(after.detail)) {
    const bd = before.detail ?? createDefaultDetail();
    const ad = after.detail ?? createDefaultDetail();
    const presenceChanged: string[] = [];
    if ((bd.clarity ?? 0) !== (ad.clarity ?? 0)) presenceChanged.push('Clarity');
    if ((bd.texture ?? 0) !== (ad.texture ?? 0)) presenceChanged.push('Texture');
    if ((bd.dehaze ?? 0) !== (ad.dehaze ?? 0)) presenceChanged.push('Dehaze');

    const detail6Changed =
      (bd.sharpen_amount ?? 0) !== (ad.sharpen_amount ?? 0) ||
      (bd.sharpen_radius ?? 1.0) !== (ad.sharpen_radius ?? 1.0) ||
      (bd.sharpen_masking ?? 0) !== (ad.sharpen_masking ?? 0) ||
      (bd.noise_luminance ?? 0) !== (ad.noise_luminance ?? 0) ||
      (bd.noise_colour ?? 0) !== (ad.noise_colour ?? 0) ||
      Boolean(bd.hot_pixels) !== Boolean(ad.hot_pixels);

    if (!detail6Changed && presenceChanged.length === 1) {
      changed.push(presenceChanged[0]);
    } else {
      changed.push('Detail');
    }
  }
  if (JSON.stringify(before.masks) !== JSON.stringify(after.masks)) {
    if ((after.masks?.length ?? 0) > (before.masks?.length ?? 0)) {
      changed.push('Add mask');
    } else {
      changed.push('Masks');
    }
  }
  if (JSON.stringify(before.point_colours ?? []) !== JSON.stringify(after.point_colours ?? [])) {
    changed.push('Point colour');
  }

  if (changed.length === 0) return 'Edit';
  if (changed.length === 1) return changed[0];
  return 'Several settings';
}

function triggerSaveWithoutHistory() {
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
      await desktop.saveRecipe(sourcePath.value, cleanRecipeForSave(recipe.value));
      saveStatus.value = 'Saved';
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      saveStatus.value = `Couldn't save: ${msg}`;
    }
  }, 300);
}

function scheduleSave(explicitLabel?: string) {
  const label = explicitLabel || deriveLabel(lastCommittedRecipe, recipe.value);
  const committed = history.commit(recipe.value, label);
  if (committed) {
    lastCommittedRecipe = cloneRecipe(recipe.value);
    syncHistoryState();
    dismissNotice();
  }

  triggerSaveWithoutHistory();
}

function onRecipeValueInput() {
  requestRender('Drag');
}

function onRecipeValueChange() {
  requestRender('Settle');
  scheduleSave();
}

function commitRecipeChange(explicitLabel?: string) {
  requestRender('Settle');
  scheduleSave(explicitLabel);
}

function undoStep() {
  const restored = history.undo();
  if (!restored) return;
  applyRestoredRecipe(restored);
}

function redoStep() {
  const restored = history.redo();
  if (!restored) return;
  applyRestoredRecipe(restored);
}

function jumpToHistory(index: number) {
  const restored = history.jump(index);
  if (!restored) return;
  applyRestoredRecipe(restored);
}

function applyRestoredRecipe(restored: AdjustmentRecipe) {
  const curSnapshots = recipe.value.snapshots ? cloneRecipe({ snapshots: recipe.value.snapshots } as any).snapshots : [];
  const curVersions = recipe.value.versions ? cloneRecipe({ versions: recipe.value.versions } as any).versions : [];
  const curVersionName = recipe.value.version_name ?? null;

  const next = cloneRecipe(restored);
  if (curSnapshots && curSnapshots.length > 0) {
    next.snapshots = curSnapshots;
  } else {
    delete next.snapshots;
  }
  if (curVersions && curVersions.length > 0) {
    next.versions = curVersions;
  } else {
    delete next.versions;
  }
  if (curVersionName) {
    next.version_name = curVersionName;
  } else {
    delete next.version_name;
  }

  recipe.value = next;
  lastCommittedRecipe = cloneRecipe(next);
  syncHistoryState();

  if (selectedMaskId.value && !recipe.value.masks?.some((m) => m.id === selectedMaskId.value)) {
    selectedMaskId.value = null;
    painting.value = false;
  }

  requestRender('Settle');
  triggerSaveWithoutHistory();
}

function saveSnapshot() {
  const name = newSnapshotName.value.trim();
  if (!name) {
    snapshotMessage.value = 'Snapshot name cannot be empty.';
    return;
  }
  if (recipe.value.snapshots?.some((s) => s.name === name)) {
    snapshotMessage.value = `A snapshot named "${name}" already exists.`;
    return;
  }
  snapshotMessage.value = '';
  const snapRecipe = cloneRecipe(recipe.value);
  delete snapRecipe.snapshots;
  delete snapRecipe.versions;
  delete snapRecipe.version_name;
  const snap: Snapshot = {
    name,
    recipe: snapRecipe,
  };
  if (!recipe.value.snapshots) {
    recipe.value.snapshots = [];
  }
  recipe.value.snapshots.push(snap);
  newSnapshotName.value = '';
  triggerSaveWithoutHistory();
}

function restoreSnapshot(snap: Snapshot) {
  const curSnapshots = recipe.value.snapshots ? cloneRecipe({ snapshots: recipe.value.snapshots } as any).snapshots : [];
  const curVersions = recipe.value.versions ? cloneRecipe({ versions: recipe.value.versions } as any).versions : [];
  const curVersionName = recipe.value.version_name ?? null;

  const next = cloneRecipe(snap.recipe);
  if (curSnapshots && curSnapshots.length > 0) {
    next.snapshots = curSnapshots;
  } else {
    delete next.snapshots;
  }
  if (curVersions && curVersions.length > 0) {
    next.versions = curVersions;
  } else {
    delete next.versions;
  }
  if (curVersionName) {
    next.version_name = curVersionName;
  } else {
    delete next.version_name;
  }

  recipe.value = next;
  requestRender('Settle');
  scheduleSave(`Snapshot: ${snap.name}`);
}

function deleteSnapshot(name: string) {
  if (!recipe.value.snapshots) return;
  recipe.value.snapshots = recipe.value.snapshots.filter((s) => s.name !== name);
  triggerSaveWithoutHistory();
}

// ---------------------------------------------------------------------------
// Versions operations (RP-6)
// ---------------------------------------------------------------------------

function createVersion() {
  const name = newVersionInput.value.trim();
  if (!name) {
    versionMessage.value = 'Version name cannot be empty.';
    return;
  }
  const isMainTaken = !recipe.value.version_name || recipe.value.versions?.some((v) => v.name === 'Main');
  if (name === 'Main' && isMainTaken) {
    versionMessage.value = 'A version named "Main" already exists.';
    return;
  }
  if (name === currentVersionName.value || recipe.value.versions?.some((v) => v.name === name)) {
    versionMessage.value = `A version named "${name}" already exists.`;
    return;
  }
  versionMessage.value = '';

  // The current recipe (excluding versions, snapshots, version_name) goes into the list under its name
  const oldCurrentStripped = cloneRecipe(recipe.value);
  delete oldCurrentStripped.versions;
  delete oldCurrentStripped.snapshots;
  delete oldCurrentStripped.version_name;

  const oldName = currentVersionName.value;
  const currentVersions: Version[] = recipe.value.versions ? [...recipe.value.versions] : [];
  currentVersions.push({
    name: oldName,
    recipe: oldCurrentStripped,
  });

  // Duplicates the current edit, and the new one becomes current
  recipe.value.versions = currentVersions;
  recipe.value.version_name = name;

  // Reset history to the new current recipe
  history.reset(recipe.value);
  lastCommittedRecipe = cloneRecipe(recipe.value);
  syncHistoryState();

  triggerSaveWithoutHistory();
  newVersionInput.value = suggestVersionName();
}

function switchVersion(targetName: string) {
  if (!recipe.value.versions) return;
  const targetIndex = recipe.value.versions.findIndex((v) => v.name === targetName);
  if (targetIndex === -1) return;

  const target = recipe.value.versions[targetIndex];

  // The current recipe, without versions and snapshots, goes into the list under its name
  const currentToStore = cloneRecipe(recipe.value);
  delete currentToStore.versions;
  delete currentToStore.snapshots;
  delete currentToStore.version_name;

  const currentStoredName = currentVersionName.value;

  const nextVersions = recipe.value.versions.map((v, i) => {
    if (i === targetIndex) {
      return {
        name: currentStoredName,
        recipe: currentToStore,
      };
    }
    return v;
  });

  // Snapshots stay on the top level throughout
  const currentSnapshots = recipe.value.snapshots ? cloneRecipe({ snapshots: recipe.value.snapshots } as any).snapshots : [];

  // The chosen one comes out of the list and becomes the top level
  const nextRecipe = cloneRecipe(target.recipe);
  delete nextRecipe.versions;
  delete nextRecipe.snapshots;
  delete nextRecipe.version_name;

  if (targetName !== 'Main') {
    nextRecipe.version_name = targetName;
  } else {
    delete nextRecipe.version_name;
  }
  nextRecipe.versions = nextVersions;
  if (currentSnapshots && currentSnapshots.length > 0) {
    nextRecipe.snapshots = currentSnapshots;
  }

  recipe.value = nextRecipe;
  selectedMaskId.value = null;
  painting.value = false;
  requestRender('Settle');

  // Reset the history to the new current recipe
  history.reset(recipe.value);
  lastCommittedRecipe = cloneRecipe(recipe.value);
  syncHistoryState();

  triggerSaveWithoutHistory();
  newVersionInput.value = suggestVersionName();
}

function startRenameCurrent() {
  renameCurrentInput.value = currentVersionName.value;
  editingCurrentVersion.value = true;
  versionMessage.value = '';
}

function submitRenameCurrent() {
  const name = renameCurrentInput.value.trim();
  if (!name) {
    versionMessage.value = 'Version name cannot be empty.';
    return;
  }
  if (name === currentVersionName.value) {
    editingCurrentVersion.value = false;
    return;
  }
  const isMainTaken = recipe.value.versions?.some((v) => v.name === 'Main');
  if (name === 'Main' && isMainTaken) {
    versionMessage.value = 'A version named "Main" already exists.';
    return;
  }
  if (recipe.value.versions?.some((v) => v.name === name)) {
    versionMessage.value = `A version named "${name}" already exists.`;
    return;
  }
  versionMessage.value = '';
  recipe.value.version_name = name === 'Main' ? null : name;
  editingCurrentVersion.value = false;
  triggerSaveWithoutHistory();
  newVersionInput.value = suggestVersionName();
}

function startRenameOther(oldName: string) {
  renameOtherInput.value = oldName;
  editingVersionName.value = oldName;
  versionMessage.value = '';
}

function submitRenameOther(oldName: string) {
  const name = renameOtherInput.value.trim();
  if (!name) {
    versionMessage.value = 'Version name cannot be empty.';
    return;
  }
  if (name === oldName) {
    editingVersionName.value = null;
    return;
  }
  const isMainTaken = !recipe.value.version_name || recipe.value.versions?.some((v) => v.name === 'Main' && v.name !== oldName);
  if (name === 'Main' && isMainTaken) {
    versionMessage.value = 'A version named "Main" already exists.';
    return;
  }
  if (name === currentVersionName.value || recipe.value.versions?.some((v) => v.name === name && v.name !== oldName)) {
    versionMessage.value = `A version named "${name}" already exists.`;
    return;
  }
  versionMessage.value = '';
  const target = recipe.value.versions?.find((v) => v.name === oldName);
  if (target) {
    target.name = name;
  }
  editingVersionName.value = null;
  triggerSaveWithoutHistory();
  newVersionInput.value = suggestVersionName();
}

function deleteVersion(name: string) {
  if (!recipe.value.versions) return;
  recipe.value.versions = recipe.value.versions.filter((v) => v.name !== name);
  triggerSaveWithoutHistory();
  newVersionInput.value = suggestVersionName();
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
  if (e.key === 'Alt') altHeld.value = true;
  if (e.code === 'Space' && !textIsInPlay() && !e.repeat) {
    spaceHeld.value = true;
    if (e.target === document.body || (e.target as HTMLElement)?.closest('.canvas-viewport')) {
      e.preventDefault();
    }
  }

  if (textIsInPlay()) {
    if (e.key === '\\' && !isTextInputFocused()) {
      e.preventDefault();
      setBefore(true);
    }
    return;
  }

  // Undo and Redo: Cmd-Z and Cmd-Shift-Z (RP-5)
  const isMod = e.metaKey || e.ctrlKey;
  if (isMod && (e.key === 'z' || e.key === 'Z')) {
    e.preventDefault();
    if (isCropMode.value) return;
    if (e.shiftKey) {
      redoStep();
    } else {
      undoStep();
    }
    return;
  }

  // Z toggles fit and 100%
  if ((e.key === 'z' || e.key === 'Z') && !e.metaKey && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    if (zoom.value === 'fit') {
      setZoom(1.0);
    } else {
      setZoom('fit');
    }
    return;
  }

  // Y toggles side-by-side (plain) or split (Shift-Y)
  if ((e.key === 'y' || e.key === 'Y') && !e.metaKey && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    if (e.shiftKey) {
      toggleCompare('split');
    } else {
      toggleCompare('side-by-side');
    }
    return;
  }

  // W toggles white balance eyedropper (RP-4)
  if ((e.key === 'w' || e.key === 'W') && !e.metaKey && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    toggleEyedropper();
    return;
  }

  // V toggles Black & White (RP-4)
  if ((e.key === 'v' || e.key === 'V') && !e.metaKey && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    toggleBlackAndWhite();
    return;
  }

  // Esc exits eyedropper mode
  if (e.key === 'Escape' && isEyedropperActive.value) {
    e.preventDefault();
    exitEyedropper();
    return;
  }

  if (painting.value && !e.metaKey && !e.ctrlKey) {
    if (e.key === '[' || e.key === ']') {
      e.preventDefault();
      nudgeBrush(e.key === ']' ? 1 : -1);
      return;
    }
    if (e.key === 'Escape') {
      painting.value = false;
      return;
    }
  }
  // Esc leaves a mask's editing once painting is not in the way (painting stops first).
  if (e.key === 'Escape' && selectedMaskId.value && !document.querySelector('[role="dialog"]')) {
    finishMaskEditing();
    return;
  }
  const mod = e.metaKey || e.ctrlKey;
  if (mod && !e.shiftKey && !e.altKey) {
    if (e.key === 'c' || e.key === 'v') {
      if (!sessionId.value || document.querySelector('[role="dialog"]')) return;
      e.preventDefault();
      if (e.key === 'c') copySettings();
      else pasteSettings();
      return;
    }
    if (e.key === '1') {
      e.preventDefault();
      setZoom(1.0);
      return;
    }
    if (e.key === '0') {
      e.preventDefault();
      setZoom('fit');
      return;
    }
    if (e.key === '=' || e.key === '+') {
      e.preventDefault();
      stepZoom(1);
      return;
    }
    if (e.key === '-') {
      e.preventDefault();
      stepZoom(-1);
      return;
    }
  }
  if (e.key === '\\') {
    if (isTextInputFocused()) return;
    e.preventDefault();
    setBefore(true);
  }
}

function handleKeyUp(e: KeyboardEvent) {
  if (e.key === 'Alt') altHeld.value = false;
  if (e.code === 'Space') spaceHeld.value = false;
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
    detail: existing.detail
      ? {
          sharpen_amount: existing.detail.sharpen_amount ?? 0,
          sharpen_radius: existing.detail.sharpen_radius ?? 1.0,
          sharpen_masking: existing.detail.sharpen_masking ?? 0,
          noise_luminance: existing.detail.noise_luminance ?? 0,
          noise_colour: existing.detail.noise_colour ?? 0,
          hot_pixels: existing.detail.hot_pixels ?? false,
          clarity: existing.detail.clarity ?? 0,
          texture: existing.detail.texture ?? 0,
          dehaze: existing.detail.dehaze ?? 0,
        }
      : createDefaultDetail(),
    point_colours: (existing.point_colours ?? []).map((p) => ({ ...p })),
    masks: (existing.masks ?? []).map(normaliseMask),
    snapshots: (existing.snapshots ?? []).map((s) => ({
      name: s.name,
      recipe: cloneRecipe(s.recipe),
    })),
    version_name: existing.version_name ?? null,
    versions: (existing.versions ?? []).map((v) => ({
      name: v.name,
      recipe: cloneRecipe(v.recipe),
    })),
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
  // core writes nothing for an empty stroke list (`skip_serializing_if`).
  const res: Mask = {
    id: m.id,
    name: m.name ?? '',
    kind,
    invert: m.invert ?? false,
    opacity: m.opacity ?? 1,
    enabled: m.enabled ?? true,
    adjustments: { ...ZERO_LOCAL, ...(m.adjustments ?? {}) },
    strokes: (m.strokes ?? []).map((st) => ({
      points: st.points.map((pt) => [pt[0], pt[1]] as [number, number]),
      radius: st.radius,
      feather: st.feather ?? 0.5,
      flow: st.flow ?? 1,
      erase: st.erase ?? false,
    })),
  };
  if (m.components && m.components.length > 0) {
    res.components = m.components.map((c) => ({
      op: c.op ?? 'intersect',
      kind:
        c.kind.type === 'radial'
          ? { ...c.kind, angle: c.kind.angle ?? 0, feather: c.kind.feather ?? 0.5 }
          : { ...c.kind },
      invert: c.invert ?? false,
    }));
  }
  return res;
}

function cleanRecipeForSave(r: AdjustmentRecipe): AdjustmentRecipe {
  const copy = cloneRecipe(r);
  if (copy.masks) {
    for (const m of copy.masks) {
      if (!m.components || m.components.length === 0) {
        delete m.components;
      }
    }
  }
  return copy;
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
function replaceLook(incoming: AdjustmentRecipe, text: string, explicitLabel?: string) {
  const before = cloneRecipe(recipe.value);
  const next = normaliseRecipe(cloneRecipe(incoming), before.source_sha256 ?? '');
  next.geometry = before.geometry;
  // Masks are this photograph's too, placed on its content (ED-19): never replaced by
  // another photograph's settings or a preset's.
  next.masks = before.masks;
  next.source_sha256 = before.source_sha256;
  next.snapshots = before.snapshots;
  next.version_name = before.version_name;
  next.versions = before.versions;
  recipe.value = next;
  commitRecipeChange(explicitLabel);
  showNotice(text, before);
}

function undoReplace() {
  dismissNotice();
  undoStep();
}

async function applyPreset(name: string) {
  if (!sessionId.value) return;
  try {
    const preset = await desktop.loadPreset(name);
    selectedPreset.value = name;
    replaceLook(preset, `Applied preset ${name}. Crop, rotation and masks kept.`, name);
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
  copyToClipboard(recipe.value, fileName(sourcePath.value));
  showNotice(`Copied settings from ${fileName(sourcePath.value)}.`);
}

function pasteSettings() {
  const copied = copiedSettings.value;
  if (!copied || !sessionId.value) return;
  selectedPreset.value = null;
  replaceLook(copied.recipe, `Pasted settings from ${copied.from}. Crop, rotation and masks kept.`, 'Paste settings');
}

async function openImage(path: string) {
  if (!path.trim()) return;
  // Undo belongs to the photograph it was offered on.
  dismissNotice();
  selectedPreset.value = null;
  selectedMaskId.value = null;
  stopAutoMask();
  autoDownload.value = null;
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
    if (info.drag) {
      dragWidth.value = info.drag[0];
      dragHeight.value = info.drag[1];
    }
    settleWidth.value = info.settle_width ?? info.settle[0];
    settleHeight.value = info.settle_height ?? info.settle[1];
    fullWidth.value = info.full_width ?? info.settle[0];
    fullHeight.value = info.full_height ?? info.settle[1];
    zoom.value = 'fit';
    pan.value = { x: 0, y: 0 };
    compareMode.value = 'off';
    lastBeforeRendered = null;
    lastBeforeFrame = null;

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

    history.reset(recipe.value);
    lastCommittedRecipe = cloneRecipe(recipe.value);
    syncHistoryState();

    editingCurrentVersion.value = false;
    editingVersionName.value = null;
    versionMessage.value = '';
    newVersionInput.value = suggestVersionName();
    exportAllSummary.value = '';

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

const liveExampleName = computed(() => {
  const stem = sourcePath.value
    ? sourcePath.value.split('/').pop()?.replace(/\.[^/.]+$/, '') || 'IMG_0001'
    : 'IMG_0001';
  const verName = currentVersionName.value === 'Main' ? '' : currentVersionName.value;
  const editSuffix = verName ? `_edit_${verName.replace(/[^a-zA-Z0-9]/g, '-')}` : '_edit';
  const dateStr = '2024-08-15';

  let ext = 'jpg';
  switch (exportState.formatKind) {
    case 'AsSource': {
      const srcExt = sourcePath.value?.split('.').pop()?.toLowerCase();
      ext = srcExt === 'tiff' || srcExt === 'tif' ? 'tiff' : 'jpg';
      break;
    }
    case 'Jpeg':
      ext = 'jpg';
      break;
    case 'Tiff':
      ext = 'tiff';
      break;
    case 'Png':
      ext = 'png';
      break;
    case 'Webp':
      ext = 'webp';
      break;
    case 'Avif':
      ext = 'avif';
      break;
  }

  if (!exportState.nameTemplate.trim()) {
    return `${stem}${editSuffix}.${ext}`;
  }

  const substituted = exportState.nameTemplate
    .replace(/\{name\}/g, stem)
    .replace(/\{version\}/g, verName)
    .replace(/\{date\}/g, dateStr)
    .replace(/\{edit\}/g, editSuffix);

  let sanitized = '';
  for (const c of substituted) {
    if (/[a-zA-Z0-9\-_. ]/.test(c)) {
      sanitized += c;
    } else {
      sanitized += '-';
    }
  }
  return `${sanitized}.${ext}`;
});

async function handleExport() {
  if (!sourcePath.value || !exportDir.value || !isResizeValid.value) return;
  exportStatus.value = null;
  exportError.value = null;

  try {
    const options = buildExportOptions();
    const res = await desktop.exportEditedImage(
      sourcePath.value.trim(),
      recipe.value,
      exportDir.value.trim(),
      options,
    );
    exportStatus.value = res.path;
    if (res.metadata_skipped) {
      exportStatus.value += ` (Metadata skipped: ${res.metadata_skipped.reason})`;
    }
  } catch (err: unknown) {
    exportError.value = err instanceof Error ? err.message : String(err);
  }
}

async function exportAllVersions() {
  if (!sourcePath.value || !exportDir.value || isExportingAll.value || !isResizeValid.value) return;

  const currentExportRecipe = cloneRecipe(recipe.value);
  delete currentExportRecipe.versions;
  delete currentExportRecipe.snapshots;
  currentExportRecipe.version_name = recipe.value.version_name ?? null;

  interface ExportItem {
    name: string;
    recipe: AdjustmentRecipe;
  }

  const items: ExportItem[] = [
    {
      name: currentVersionName.value,
      recipe: currentExportRecipe,
    },
  ];

  for (const v of recipe.value.versions ?? []) {
    const r = cloneRecipe(v.recipe);
    delete r.versions;
    delete r.snapshots;
    r.version_name = v.name === 'Main' ? null : v.name;
    items.push({
      name: v.name,
      recipe: r,
    });
  }

  isExportingAll.value = true;
  exportAllSummary.value = '';
  const written: Array<{ name: string; path: string }> = [];
  const failures: Array<{ name: string; reason: string }> = [];
  const options = buildExportOptions();

  try {
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      exportAllProgressText.value = `Exporting ${i + 1}/${items.length}…`;
      try {
        const res = await desktop.exportEditedImage(
          sourcePath.value.trim(),
          item.recipe,
          exportDir.value.trim(),
          options,
        );
        written.push({ name: item.name, path: res.path });
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        failures.push({ name: item.name, reason: msg });
      }
    }
  } finally {
    isExportingAll.value = false;
    exportAllProgressText.value = '';
  }

  const parts: string[] = [];
  if (written.length > 0) {
    parts.push(`${written.length} written`);
  }
  if (failures.length > 0) {
    const reasons = failures.map((f) => `${f.name}: ${f.reason}`).join('; ');
    parts.push(`${failures.length} failed (${reasons})`);
  }
  exportAllSummary.value = parts.join(', ');
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

function addMask(type: 'linear' | 'radial' | 'brush') {
  if (!sessionId.value || masks.value.length >= MASK_LIMIT) return;
  const count = masks.value.filter((m) => m.kind.type === type).length + 1;
  // Placed by where they appear on screen, whatever the orientation or crop: a graduated
  // filter coming down from the top, a radial in the middle.
  const kind: MaskKind =
    type === 'brush'
      ? { type }
      : type === 'linear'
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
    name: `${{ linear: 'Linear', radial: 'Radial', brush: 'Brush' }[type]} ${count}`,
    kind,
    invert: false,
    opacity: 1,
    enabled: true,
    adjustments: { ...ZERO_LOCAL },
  };
  recipe.value.masks = [...masks.value, mask];
  selectedMaskId.value = mask.id;
  selectedComponentIndex.value = null;
  // A brush mask is nothing until painted: start painting at once.
  painting.value = type === 'brush';
  commitRecipeChange('Add mask');
}

function addLuminanceMask() {
  if (!sessionId.value || masks.value.length >= MASK_LIMIT) return;
  const count = masks.value.filter((m) => m.kind.type === 'luminance_range').length + 1;
  maskSeq += 1;
  const mask: Mask = {
    id: `m${Date.now().toString(36)}${maskSeq}`,
    name: `Luminance ${count}`,
    kind: {
      type: 'luminance_range',
      low: 0.25,
      high: 0.75,
      feather: 0.2,
    },
    invert: false,
    opacity: 1,
    enabled: true,
    adjustments: { ...ZERO_LOCAL },
  };
  recipe.value.masks = [...masks.value, mask];
  selectedMaskId.value = mask.id;
  selectedComponentIndex.value = null;
  showMaskOverlay.value = true;
  commitRecipeChange('Add mask');
}

async function addBackgroundMask() {
  if (!sessionId.value || masks.value.length >= MASK_LIMIT || autoBusy.value) return;
  error.value = null;
  try {
    const model = (await desktop.autoMaskModels()).find((m) => m.target === 'subject');
    if (!model) throw new Error('No model is known for Subject');
    if (!model.present) {
      autoDownload.value = { target: 'subject', name: model.name, jobId: null, asBackground: true };
      return;
    }
    await runAutoMask('subject', true);
  } catch (err: unknown) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

/** The brush radius the overlay paints with, or null when not painting. */
const paintRadius = computed(() =>
  painting.value && selectedMask.value ? brushSize.value / 100 : null,
);

function onStrokeStart(point: [number, number]) {
  const m = selectedMask.value;
  if (!m) return;
  m.strokes = [
    ...(m.strokes ?? []),
    {
      points: [point],
      radius: brushSize.value / 100,
      feather: brushFeather.value / 100,
      flow: brushFlow.value / 100,
      erase: brushErase.value !== altHeld.value,
    },
  ];
  requestRender('Drag');
}

function onStrokePoint(point: [number, number]) {
  const strokes = selectedMask.value?.strokes;
  const stroke = strokes?.[strokes.length - 1];
  if (!stroke) return;
  stroke.points.push(point);
  requestRender('Drag');
}

function onStrokeEnd() {
  onRecipeValueChange();
}

/** Back to the whole photograph: the mask stays, only its editing ends. */
function finishMaskEditing() {
  painting.value = false;
  selectedComponentIndex.value = null;
  selectedMaskId.value = null;
}

function undoStroke() {
  const m = selectedMask.value;
  if (!m?.strokes?.length) return;
  m.strokes = m.strokes.slice(0, -1);
  onRecipeValueChange();
}

function clearStrokes() {
  const m = selectedMask.value;
  if (!m?.strokes?.length) return;
  const before = cloneRecipe(recipe.value);
  m.strokes = [];
  onRecipeValueChange();
  showNotice(`Cleared the painting on ${m.name}.`, before);
}

function nudgeBrush(delta: number) {
  brushSize.value = Math.min(BRUSH_MAX, Math.max(BRUSH_MIN, +(brushSize.value + delta).toFixed(1)));
}

watch(selectedMaskId, async (id, previous) => {
  if (!id) {
    painting.value = false;
    return;
  }
  // The mask's controls appear at the top of the panel; when it was chosen from further down
  // (the list, a pin on the photograph, + Radial), bring them into view just below the pinned
  // histogram, or the person would not see them arrive.
  if (id === previous) return;
  await nextTick();
  const panel = document.querySelector<HTMLElement>('.adjustment-panel');
  const block = document.querySelector<HTMLElement>('[data-testid="mask-editing"]');
  const hist = document.querySelector<HTMLElement>('.adjustment-panel__histogram');
  if (!panel || !block) return;
  const gap = 8;
  const offset =
    block.getBoundingClientRect().top - panel.getBoundingClientRect().top -
    (hist?.getBoundingClientRect().height ?? 0) - gap;
  panel.scrollTop += offset;
});

async function addAutoMask(target: AutoTarget) {
  if (!sessionId.value || masks.value.length >= MASK_LIMIT || autoBusy.value) return;
  error.value = null;
  try {
    const model = (await desktop.autoMaskModels()).find((m) => m.target === target);
    if (!model) throw new Error(`No model is known for ${AUTO_LABEL[target]}`);
    if (!model.present) {
      // Ask first: it is a few hundred megabytes, kept for every later photograph.
      autoDownload.value = { target, name: model.name, jobId: null };
      return;
    }
    await runAutoMask(target);
  } catch (err: unknown) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

async function startModelDownload() {
  const d = autoDownload.value;
  if (!d) return;
  try {
    d.jobId = await desktop.downloadMaskModel(d.target);
  } catch (err: unknown) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

async function cancelModelDownload() {
  const id = autoDownload.value?.jobId;
  if (id) await desktop.cancelJob(id).catch(() => false);
}

/** The job ended: carry on if the model is now on disk; otherwise the job says why. */
async function onModelDownloaded() {
  const d = autoDownload.value;
  if (!d) return;
  const model = (await desktop.autoMaskModels()).find((m) => m.target === d.target);
  if (model?.present) {
    autoDownload.value = null;
    if (d.componentForMaskId) {
      await finishAddAutoComponent(d.target, d.componentForMaskId);
    } else if (d.asBackground) {
      await runAutoMask('subject', true);
    } else {
      await runAutoMask(d.target);
    }
  }
}

async function runAutoMask(target: AutoTarget, asBackground = false) {
  if (!sessionId.value) return;
  const seq = ++autoSeq;
  const session = sessionId.value;
  autoBusy.value = target;
  try {
    const raster = await desktop.makeAutoMask(session, target);
    // Stopped, or another photograph opened meanwhile: the result is not this one's.
    if (seq !== autoSeq || session !== sessionId.value) return;
    const count = masks.value.filter((m) => m.kind.type === 'auto' && m.kind.target === target).length + 1;
    maskSeq += 1;
    const mask: Mask = {
      id: `m${Date.now().toString(36)}${maskSeq}`,
      name: asBackground ? 'Background' : `${AUTO_LABEL[target]} ${count}`,
      kind: { type: 'auto', target, mask: raster },
      invert: asBackground,
      opacity: 1,
      enabled: true,
      adjustments: { ...ZERO_LOCAL },
      strokes: [],
    };
    recipe.value.masks = [...masks.value, mask];
    selectedMaskId.value = mask.id;
    selectedComponentIndex.value = null;
    // Show what was found: the person judges it before adjusting through it.
    showMaskOverlay.value = true;
    onRecipeValueChange();
  } catch (err: unknown) {
    if (seq === autoSeq) error.value = err instanceof Error ? err.message : String(err);
  } finally {
    if (seq === autoSeq) autoBusy.value = null;
  }
}

function stopAutoMask() {
  autoSeq += 1;
  autoBusy.value = null;
}

const selectedComponentIndex = ref<number | null>(null);

watch(selectedMaskId, () => {
  selectedComponentIndex.value = null;
});

const activeEditingKind = computed(() => {
  if (!selectedMask.value) return null;
  if (
    selectedComponentIndex.value !== null &&
    selectedMask.value.components?.[selectedComponentIndex.value]
  ) {
    return selectedMask.value.components[selectedComponentIndex.value].kind;
  }
  return selectedMask.value.kind;
});

const overlayMasks = computed<OverlayMask[]>(() => {
  return masks.value.map((m) => {
    if (
      m.id === selectedMaskId.value &&
      selectedComponentIndex.value !== null &&
      m.components?.[selectedComponentIndex.value]
    ) {
      const comp = m.components[selectedComponentIndex.value];
      return {
        id: m.id,
        name: m.name,
        kind: comp.kind,
        strokes: m.strokes,
      };
    }
    return {
      id: m.id,
      name: m.name,
      kind: m.kind,
      strokes: m.strokes,
    };
  });
});

function selectComponent(index: number) {
  selectedComponentIndex.value = index;
}

function selectBaseMask() {
  selectedComponentIndex.value = null;
}

function setComponentOp(index: number, op: MaskOp) {
  const m = selectedMask.value;
  if (!m?.components?.[index]) return;
  m.components[index].op = op;
  commitRecipeChange('Masks');
}

function toggleComponentInvert(index: number) {
  const m = selectedMask.value;
  if (!m?.components?.[index]) return;
  m.components[index].invert = !m.components[index].invert;
  commitRecipeChange('Masks');
}

function deleteComponent(index: number) {
  const m = selectedMask.value;
  if (!m || !m.components) return;
  m.components.splice(index, 1);
  if (m.components.length === 0) {
    delete m.components;
  }
  if (selectedComponentIndex.value === index) {
    selectedComponentIndex.value = null;
  } else if (selectedComponentIndex.value !== null && selectedComponentIndex.value > index) {
    selectedComponentIndex.value -= 1;
  }
  commitRecipeChange('Masks');
}

function addComponent(type: 'linear' | 'radial' | 'luminance_range' | 'colour_range' | 'subject' | 'sky') {
  const m = selectedMask.value;
  if (!m) return;
  if (type === 'colour_range') {
    startColourRangeComponentEyedropper();
    return;
  }
  if (type === 'subject' || type === 'sky') {
    addAutoComponent(type);
    return;
  }
  if (!m.components) {
    m.components = [];
  }
  let comp: MaskComponent;
  if (type === 'linear') {
    comp = {
      op: 'add',
      kind: {
        type: 'linear',
        start: frameToStoredPoint(0.5, 0.1),
        end: frameToStoredPoint(0.5, 0.55),
      },
      invert: false,
    };
  } else if (type === 'radial') {
    comp = {
      op: 'add',
      kind: {
        type: 'radial',
        center: frameToStoredPoint(0.5, 0.5),
        radius_x: 0.2,
        radius_y: 0.2,
        angle: 0,
        feather: 0.5,
      },
      invert: false,
    };
  } else {
    comp = {
      op: 'intersect',
      kind: {
        type: 'luminance_range',
        low: 0.25,
        high: 0.75,
        feather: 0.2,
      },
      invert: false,
    };
  }
  m.components.push(comp);
  selectedComponentIndex.value = m.components.length - 1;
  commitRecipeChange('Masks');
}

async function finishAddAutoComponent(target: AutoTarget, maskId: string) {
  if (!sessionId.value || autoBusy.value) return;
  const m = masks.value.find((x) => x.id === maskId);
  if (!m) return;
  const seq = ++autoSeq;
  const session = sessionId.value;
  autoBusy.value = target;
  try {
    const raster = await desktop.makeAutoMask(session, target);
    if (seq !== autoSeq || session !== sessionId.value) return;
    if (!m.components) m.components = [];
    m.components.push({
      op: 'add',
      kind: { type: 'auto', target, mask: raster },
      invert: false,
    });
    selectedMaskId.value = m.id;
    selectedComponentIndex.value = m.components.length - 1;
    commitRecipeChange('Masks');
  } finally {
    if (seq === autoSeq) autoBusy.value = null;
  }
}

async function addAutoComponent(target: AutoTarget) {
  if (!sessionId.value || autoBusy.value) return;
  const m = selectedMask.value;
  if (!m) return;
  error.value = null;
  try {
    const model = (await desktop.autoMaskModels()).find((md) => md.target === target);
    if (!model) throw new Error(`No model is known for ${AUTO_LABEL[target]}`);
    if (!model.present) {
      autoDownload.value = { target, name: model.name, jobId: null, componentForMaskId: m.id };
      return;
    }
    await finishAddAutoComponent(target, m.id);
  } catch (err: unknown) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

function setLuminanceRangeField(field: 'low' | 'high' | 'feather', val: number, settle: boolean) {
  if (!activeEditingKind.value || activeEditingKind.value.type !== 'luminance_range') return;
  activeEditingKind.value[field] = Math.round(val) / 100;
  if (settle) commitRecipeChange('Masks');
  else requestRender('Drag');
}

function setColourRangeField(field: 'range', val: number, settle: boolean) {
  if (!activeEditingKind.value || activeEditingKind.value.type !== 'colour_range') return;
  activeEditingKind.value[field] = val;
  if (settle) commitRecipeChange('Masks');
  else requestRender('Drag');
}

function componentSummary(comp: MaskComponent): string {
  if (comp.kind.type === 'luminance_range') {
    const low = Math.round((comp.kind.low ?? 0.25) * 100);
    const high = Math.round((comp.kind.high ?? 0.75) * 100);
    return `Luminance ${low}–${high}%`;
  }
  if (comp.kind.type === 'colour_range') {
    return `Colour (range ${comp.kind.range ?? 50}%)`;
  }
  if (comp.kind.type === 'linear') return 'Linear';
  if (comp.kind.type === 'radial') return 'Radial';
  if (comp.kind.type === 'auto') return AUTO_LABEL[comp.kind.target] || 'Auto';
  if (comp.kind.type === 'brush') return 'Brush';
  return 'Component';
}

function baseShapeSummary(mask: Mask): string {
  if (mask.kind.type === 'linear') return 'Linear';
  if (mask.kind.type === 'radial') return 'Radial';
  if (mask.kind.type === 'brush') return 'Brush';
  if (mask.kind.type === 'auto') return AUTO_LABEL[mask.kind.target] || 'Auto';
  if (mask.kind.type === 'luminance_range') {
    const low = Math.round((mask.kind.low ?? 0.25) * 100);
    const high = Math.round((mask.kind.high ?? 0.75) * 100);
    return `Luminance ${low}–${high}%`;
  }
  if (mask.kind.type === 'colour_range') {
    return `Colour (range ${mask.kind.range ?? 50}%)`;
  }
  return 'Shape';
}

/** The overlay moves gradient handles only; its shape type is the narrower view of `MaskKind`. */
function updateMaskKind(id: string, kind: { type: string }) {
  const m = masks.value.find((x) => x.id === id);
  if (!m) return;
  if (
    id === selectedMaskId.value &&
    selectedComponentIndex.value !== null &&
    m.components?.[selectedComponentIndex.value]
  ) {
    m.components[selectedComponentIndex.value].kind = kind as MaskKind;
  } else {
    m.kind = kind as MaskKind;
  }
  requestRender('Drag');
}

function onMaskCommit() {
  commitRecipeChange('Masks');
}

function deleteMask(id: string) {
  const m = masks.value.find((x) => x.id === id);
  if (!m) return;
  const before = cloneRecipe(recipe.value);
  recipe.value.masks = masks.value.filter((x) => x.id !== id);
  if (selectedMaskId.value === id) selectedMaskId.value = null;
  commitRecipeChange('Masks');
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
  if (field === 'opacity') {
    m.opacity = percent / 100;
  } else if (field === 'feather') {
    if (activeEditingKind.value?.type === 'radial') {
      activeEditingKind.value.feather = percent / 100;
    } else if (m.kind.type === 'radial') {
      m.kind.feather = percent / 100;
    }
  }
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
  if (isBwActive.value) {
    resetBwMix();
    return;
  }
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
watch([isCropMode, isBefore, compareMode], () => {
  if (isCropMode.value || isBefore.value || compareMode.value !== 'off') {
    exitEyedropper();
  }
});
watch(isCropMode, (active) => {
  if (active) {
    compareMode.value = 'off';
    setZoom('fit');
  }
});
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
    scheduleSave('Crop');
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
  scheduleSave('Crop');
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

function ensureDetail(): Detail {
  if (!recipe.value.detail) {
    recipe.value.detail = createDefaultDetail();
  }
  return recipe.value.detail;
}

function onPresenceSliderInput(prop: 'clarity' | 'texture' | 'dehaze', val: number) {
  const d = ensureDetail();
  d[prop] = val;
  onRecipeValueInput();
}

function onPresenceSliderChange(prop: 'clarity' | 'texture' | 'dehaze', val: number) {
  const d = ensureDetail();
  d[prop] = val;
  onRecipeValueChange();
}

function resetPresence() {
  const d = ensureDetail();
  d.clarity = 0;
  d.texture = 0;
  d.dehaze = 0;
  commitRecipeChange('Presence');
}

function onDetailSliderInput(prop: keyof Detail, val: number) {
  const d = ensureDetail();
  (d as any)[prop] = val;
  onRecipeValueInput();
}

function onDetailSliderChange(prop: keyof Detail, val: number) {
  const d = ensureDetail();
  (d as any)[prop] = val;
  onRecipeValueChange();
}

function toggleHotPixels() {
  const d = ensureDetail();
  d.hot_pixels = !d.hot_pixels;
  commitRecipeChange('Detail');
}

function resetDetail() {
  const d = ensureDetail();
  d.sharpen_amount = 0;
  d.sharpen_radius = 1.0;
  d.sharpen_masking = 0;
  d.noise_luminance = 0;
  d.noise_colour = 0;
  d.hot_pixels = false;
  commitRecipeChange('Detail');
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
      desktop.saveRecipe(sourcePath.value, cleanRecipeForSave(recipe.value)).catch((err: unknown) => {
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
        :selectable="['jpg', 'jpeg', 'tif', 'tiff']"
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
        <div
          ref="viewportContainerRef"
          class="canvas-viewport__container"
          :class="{
            'is-space-held': spaceHeld,
            'is-panning': isPanning,
            'is-side-by-side': compareMode === 'side-by-side',
            'is-eyedropper': isEyedropperActive,
          }"
          @pointerdown="onViewportPointerDown"
          @wheel="onViewportWheel"
        >
          <!-- Loading full resolution quiet indicator -->
          <div
            v-if="loadingFull"
            class="canvas-viewport__loading-indicator"
            data-testid="loading-full-indicator"
          >
            Loading full resolution…
          </div>

          <!-- Side-by-side compare mode -->
          <template v-if="compareMode === 'side-by-side'">
            <div class="canvas-viewport__half canvas-viewport__half--before">
              <div class="canvas-viewport__half-pane" @dblclick="onStageDoubleClick">
                <div class="canvas-viewport__stage" :style="stageStyle">
                  <canvas
                    ref="sideBySideBeforeCanvasRef"
                    class="canvas-viewport__canvas canvas-viewport__before-canvas"
                    data-testid="before-canvas"
                    :style="canvasStyle"
                  ></canvas>
                </div>
              </div>
              <span class="canvas-viewport__badge">Before</span>
            </div>
            <div class="canvas-viewport__half canvas-viewport__half--after">
              <div class="canvas-viewport__half-pane" @dblclick="onStageDoubleClick">
                <div class="canvas-viewport__stage" :style="stageStyle">
                  <canvas
                    ref="canvasRef"
                    class="canvas-viewport__canvas"
                    data-testid="edit-canvas"
                    :style="canvasStyle"
                  ></canvas>
                </div>
              </div>
              <span class="canvas-viewport__badge">After</span>
            </div>
          </template>

          <!-- Single / Split mode -->
          <div
            v-else
            class="canvas-viewport__single"
            @dblclick="onStageDoubleClick"
          >
            <div
              class="canvas-viewport__stage"
              :style="stageStyle"
            >
              <canvas
                ref="canvasRef"
                class="canvas-viewport__canvas"
                :class="{ 'cursor-crosshair': isEyedropperActive }"
                data-testid="edit-canvas"
                :style="canvasStyle"
              ></canvas>
              <canvas
                v-if="compareMode === 'split'"
                ref="beforeCanvasRef"
                class="canvas-viewport__canvas canvas-viewport__before-canvas"
                data-testid="before-canvas"
                :style="{
                  ...canvasStyle,
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  clipPath: `polygon(0 0, ${splitDivider}% 0, ${splitDivider}% 100%, 0 100%)`,
                  zIndex: 'calc(var(--z-canvas) + 1)',
                }"
              ></canvas>

              <!-- Split divider -->
              <div
                v-if="compareMode === 'split'"
                class="canvas-viewport__split-divider"
                :style="{ left: `${splitDivider}%` }"
                @pointerdown.stop="startDividerDrag"
              >
                <div class="canvas-viewport__split-line"></div>
                <div
                  class="canvas-viewport__split-handle"
                  data-testid="split-divider-handle"
                  title="Drag to split before / after"
                >
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
                    <path d="M5 4l-4 4 4 4V4zm6 0v8l4-4-4-4z"/>
                  </svg>
                </div>
              </div>

              <!-- Badges for split mode -->
              <span
                v-if="compareMode === 'split'"
                class="canvas-viewport__badge canvas-viewport__badge--split-before"
              >
                Before
              </span>
              <span
                v-if="compareMode === 'split'"
                class="canvas-viewport__badge canvas-viewport__badge--split-after"
              >
                After
              </span>

              <!-- Overlays (hidden during compare mode) -->
              <canvas
                v-show="clipOverlayActive && compareMode === 'off'"
                ref="clipCanvasRef"
                class="canvas-viewport__clip"
                data-testid="clip-overlay"
                aria-hidden="true"
                :style="{ ...canvasStyle, position: 'absolute', zIndex: 'calc(var(--z-canvas) + 1)' }"
              ></canvas>
              <canvas
                v-show="showMaskOverlay && selectedMask && !isCropMode && !isBefore && compareMode === 'off'"
                ref="maskCanvasRef"
                class="canvas-viewport__clip"
                data-testid="mask-coverage"
                aria-hidden="true"
                :style="{ ...canvasStyle, position: 'absolute', zIndex: 'calc(var(--z-canvas) + 1)' }"
              ></canvas>
              <MaskOverlay
                v-if="sessionId && masks.length && !isCropMode && !isBefore && compareMode === 'off'"
                :masks="overlayMasks"
                :selected-id="selectedMaskId"
                :to-stored="frameToStored"
                :aspect="storedAspect"
                :frame-width="frameWidth"
                :frame-height="frameHeight"
                :paint="paintRadius"
                :style="{
                  ...canvasStyle,
                  position: 'absolute',
                  zIndex: 'calc(var(--z-canvas) + 2)',
                  pointerEvents: isEyedropperActive ? 'none' : (spaceHeld ? 'none' : 'auto'),
                }"
                @select="(id) => (selectedMaskId = id)"
                @update="updateMaskKind"
                @commit="onMaskCommit"
                @stroke-start="onStrokeStart"
                @stroke-point="onStrokePoint"
                @stroke-end="onStrokeEnd"
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
        </div>

        <!-- Before / After toggle button and viewport bar -->
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
            class="secondary canvas-viewport__view-btn"
            data-testid="compare-side-by-side-btn"
            :class="{ active: compareMode === 'side-by-side' }"
            :disabled="isCropMode"
            title="Side-by-side compare (Y)"
            @click="toggleCompare('side-by-side')"
          >
            Side by Side
          </button>
          <button
            type="button"
            class="secondary canvas-viewport__view-btn"
            data-testid="compare-split-btn"
            :class="{ active: compareMode === 'split' }"
            :disabled="isCropMode"
            title="Split compare (Shift-Y)"
            @click="toggleCompare('split')"
          >
            Split
          </button>
          <button
            type="button"
            class="secondary canvas-viewport__view-btn"
            data-testid="zoom-fit-btn"
            :class="{ active: zoom === 'fit' }"
            :disabled="isCropMode"
            title="Fit to view (Cmd-0)"
            @click="setZoom('fit')"
          >
            Fit
          </button>
          <button
            type="button"
            class="secondary canvas-viewport__view-btn"
            data-testid="zoom-100-btn"
            :class="{ active: zoom === 1.0 }"
            :disabled="isCropMode"
            title="100% zoom (Cmd-1)"
            @click="setZoom(1.0)"
          >
            100%
          </button>
          <span
            class="canvas-viewport__zoom-readout"
            data-testid="zoom-readout"
            title="Current zoom"
          >
            {{ zoomReadoutText }}
          </span>
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

        <!-- ED-20: a selected mask takes the panel, as in every darkroom application, so its
             sliders cannot be mistaken for the whole photograph's. -->
        <section v-if="selectedMask" class="mask-editing" data-testid="mask-editing">
          <header class="mask-editing__head">
            <span class="mask-editing__title">
              Editing mask <strong data-testid="mask-editing-name">{{ selectedMask.name }}</strong>
            </span>
            <button type="button" class="primary" data-testid="mask-done" @click="finishMaskEditing">
              Done
            </button>
          </header>
          <p class="mask-note">
            These sliders act only where the mask covers. Done (or Esc) returns to the whole
            photograph.
          </p>

          <div class="mask-components" data-testid="mask-components">
            <span class="effects-group-title">Components</span>
            <div class="component-list" data-testid="component-list">
              <div
                class="component-row component-row--base"
                :class="{ 'is-selected': selectedComponentIndex === null }"
                data-testid="component-row-base"
                @click="selectBaseMask"
              >
                <button
                  type="button"
                  class="ghost component-select"
                  data-testid="component-select-base"
                  @click.stop="selectBaseMask"
                >
                  Base: {{ baseShapeSummary(selectedMask) }}
                </button>
              </div>
              <div
                v-for="(comp, i) in (selectedMask.components ?? [])"
                :key="i"
                class="component-row"
                :class="{ 'is-selected': selectedComponentIndex === i }"
                :data-testid="`component-row-${i}`"
                @click="selectComponent(i)"
              >
                <button
                  type="button"
                  class="ghost component-select"
                  :data-testid="`component-select-${i}`"
                  @click.stop="selectComponent(i)"
                >
                  {{ componentSummary(comp) }}
                </button>
                <select
                  :data-testid="`component-op-${i}`"
                  class="component-op-select"
                  :value="comp.op"
                  @click.stop
                  @change="(e) => setComponentOp(i, (e.target as HTMLSelectElement).value as MaskOp)"
                >
                  <option value="add">Add</option>
                  <option value="subtract">Subtract</option>
                  <option value="intersect">Intersect</option>
                </select>
                <button
                  type="button"
                  class="ghost component-invert"
                  :class="{ active: comp.invert }"
                  :data-testid="`component-invert-${i}`"
                  title="Invert component"
                  @click.stop="toggleComponentInvert(i)"
                >
                  Inv
                </button>
                <button
                  type="button"
                  class="ghost component-delete"
                  :data-testid="`component-delete-${i}`"
                  title="Delete component"
                  @click.stop="deleteComponent(i)"
                >
                  ×
                </button>
              </div>
            </div>
            <div class="component-add-menu" data-testid="add-component-menu">
              <button
                type="button"
                class="ghost"
                data-testid="add-component-linear"
                :disabled="!sessionId || isReadOnly"
                @click="addComponent('linear')"
              >
                + Linear
              </button>
              <button
                type="button"
                class="ghost"
                data-testid="add-component-radial"
                :disabled="!sessionId || isReadOnly"
                @click="addComponent('radial')"
              >
                + Radial
              </button>
              <button
                type="button"
                class="ghost"
                data-testid="add-component-luminance"
                :disabled="!sessionId || isReadOnly"
                @click="addComponent('luminance_range')"
              >
                + Luminance
              </button>
              <button
                type="button"
                class="ghost"
                data-testid="add-component-colour"
                :disabled="!sessionId || isReadOnly"
                @click="addComponent('colour_range')"
              >
                + Colour
              </button>
              <button
                type="button"
                class="ghost"
                data-testid="add-component-subject"
                :disabled="!sessionId || isReadOnly || !!autoBusy"
                @click="addComponent('subject')"
              >
                + Subject
              </button>
              <button
                type="button"
                class="ghost"
                data-testid="add-component-sky"
                :disabled="!sessionId || isReadOnly || !!autoBusy"
                @click="addComponent('sky')"
              >
                + Sky
              </button>
            </div>
          </div>

          <div
            v-if="activeEditingKind?.type === 'luminance_range'"
            class="range-controls"
            data-testid="luminance-range-controls"
          >
            <span class="effects-group-title">Luminance Range</span>
            <AdjustmentSlider
              label="Low"
              :model-value="Math.round((activeEditingKind.low ?? 0.25) * 100)"
              :min="0"
              :max="100"
              :step="1"
              unit="%"
              :default-value="25"
              test-id="mask-range-low"
              @update:model-value="(v) => setLuminanceRangeField('low', v, false)"
              @change="(v) => setLuminanceRangeField('low', v, true)"
            />
            <AdjustmentSlider
              label="High"
              :model-value="Math.round((activeEditingKind.high ?? 0.75) * 100)"
              :min="0"
              :max="100"
              :step="1"
              unit="%"
              :default-value="75"
              test-id="mask-range-high"
              @update:model-value="(v) => setLuminanceRangeField('high', v, false)"
              @change="(v) => setLuminanceRangeField('high', v, true)"
            />
            <AdjustmentSlider
              label="Feather"
              :model-value="Math.round((activeEditingKind.feather ?? 0.2) * 100)"
              :min="0"
              :max="100"
              :step="1"
              unit="%"
              :default-value="20"
              test-id="mask-range-feather"
              @update:model-value="(v) => setLuminanceRangeField('feather', v, false)"
              @change="(v) => setLuminanceRangeField('feather', v, true)"
            />
          </div>

          <div
            v-if="activeEditingKind?.type === 'colour_range'"
            class="range-controls"
            data-testid="colour-range-controls"
          >
            <span class="effects-group-title">Colour Range</span>
            <AdjustmentSlider
              label="Range"
              :model-value="activeEditingKind.range"
              :min="0"
              :max="100"
              :step="1"
              unit="%"
              :default-value="50"
              test-id="mask-colour-range"
              @update:model-value="(v) => setColourRangeField('range', v, false)"
              @change="(v) => setColourRangeField('range', v, true)"
            />
            <button
              type="button"
              class="ghost"
              data-testid="mask-colour-repick"
              @click="startColourRangeRepick"
            >
              Re-pick colour
            </button>
          </div>

        <div class="effects-group" data-testid="mask-controls">
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

          <div class="brush-tool" data-testid="brush-tool">
            <div class="brush-tool__row">
              <button
                type="button"
                class="ghost mask-show"
                :class="{ active: painting }"
                :aria-pressed="painting"
                :disabled="isReadOnly || isCropMode"
                data-testid="brush-paint"
                :title="selectedMask.kind.type === 'brush' ? 'Paint the mask' : 'Paint onto this mask to refine it'"
                @click="painting = !painting"
              >
                {{ painting ? 'Painting' : 'Paint' }}
              </button>
              <button
                type="button"
                class="ghost mask-show"
                :class="{ active: brushErase }"
                :aria-pressed="brushErase"
                data-testid="brush-erase"
                title="Erase instead of adding (or hold ⌥ while painting)"
                @click="brushErase = !brushErase"
              >
                Erase
              </button>
            </div>
            <template v-if="painting">
              <AdjustmentSlider
                v-model="brushSize"
                label="Brush size"
                :min="BRUSH_MIN"
                :max="BRUSH_MAX"
                :step="0.5"
                unit="%"
                :default-value="4"
                test-id="brush-size"
              />
              <AdjustmentSlider
                v-model="brushFeather"
                label="Brush feather"
                :min="0"
                :max="100"
                :step="1"
                unit="%"
                :default-value="50"
                test-id="brush-feather"
              />
              <AdjustmentSlider
                v-model="brushFlow"
                label="Brush flow"
                :min="1"
                :max="100"
                :step="1"
                unit="%"
                :default-value="100"
                test-id="brush-flow"
              />
              <p class="mask-note">[ and ] change the size; hold ⌥ to erase; Esc stops.</p>
            </template>
            <div v-if="selectedMask.strokes?.length" class="brush-tool__row">
              <button
                type="button"
                class="ghost mask-show"
                data-testid="brush-undo-stroke"
                @click="undoStroke"
              >
                Undo stroke
              </button>
              <button
                type="button"
                class="ghost mask-show"
                data-testid="brush-clear"
                @click="clearStrokes"
              >
                Clear painting
              </button>
            </div>
          </div>
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
            v-if="activeEditingKind?.type === 'radial'"
            label="Feather"
            :model-value="Math.round(activeEditingKind.feather * 100)"
            :min="0"
            :max="100"
            :step="1"
            unit="%"
            :default-value="50"
            test-id="mask-feather"
            @update:model-value="(v) => setMaskField('feather', v, false)"
            @change="(v) => setMaskField('feather', v, true)"
          />
        </div>
        </section>

        <div class="adjustment-panel__sections">
          <!-- 1. Basic -->
          <CollapsibleSection
            title="Basic"
            test-id="section-basic"
            v-show="!selectedMask"
            :default-open="true"
            @reset="resetBasic"
          >
            <template #actions>
              <button
                type="button"
                class="ghost auto-tone-btn"
                data-testid="auto-tone-btn"
                :disabled="!sessionId || loading"
                title="Auto Tone: Exposure, Whites, Blacks"
                @click.stop="triggerAutoTone"
              >
                Auto
              </button>
            </template>
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

            <div class="wb-slider-row">
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
              <button
                type="button"
                class="ghost eyedropper-btn"
                :class="{ active: eyedropperTarget === 'white-balance' }"
                :disabled="isCropMode || isBefore || compareMode !== 'off' || !sessionId"
                title="White Balance Eyedropper (W)"
                data-testid="eyedropper-btn"
                @click="toggleEyedropper"
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M14 2a2 2 0 0 0-2.83 0L9.41 3.76l2.83 2.83L14 4.83A2 2 0 0 0 14 2z"/>
                  <path d="M8 5.17L2.34 10.83a2 2 0 0 0-.54.97L1 15l3.2-.8a2 2 0 0 0 .97-.54L10.83 8"/>
                </svg>
              </button>
            </div>

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
              :disabled="isBwActive"
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
              :disabled="isBwActive"
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

          <!-- Presence (RP-8) -->
          <CollapsibleSection
            title="Presence"
            test-id="section-presence"
            v-show="!selectedMask"
            :default-open="false"
            @reset="resetPresence"
          >
            <div class="presence-panel" data-testid="presence-panel">
              <AdjustmentSlider
                label="Clarity"
                :model-value="recipe.detail?.clarity ?? 0"
                :min="-100"
                :max="100"
                :step="1"
                :default-value="0"
                :disabled="loading || isReadOnly"
                test-id="presence-clarity"
                @update:model-value="(v) => onPresenceSliderInput('clarity', v)"
                @change="(v) => onPresenceSliderChange('clarity', v)"
              />
              <AdjustmentSlider
                label="Texture"
                :model-value="recipe.detail?.texture ?? 0"
                :min="-100"
                :max="100"
                :step="1"
                :default-value="0"
                :disabled="loading || isReadOnly"
                test-id="presence-texture"
                @update:model-value="(v) => onPresenceSliderInput('texture', v)"
                @change="(v) => onPresenceSliderChange('texture', v)"
              />
              <AdjustmentSlider
                label="Dehaze"
                :model-value="recipe.detail?.dehaze ?? 0"
                :min="-100"
                :max="100"
                :step="1"
                :default-value="0"
                :disabled="loading || isReadOnly"
                test-id="presence-dehaze"
                @update:model-value="(v) => onPresenceSliderInput('dehaze', v)"
                @change="(v) => onPresenceSliderChange('dehaze', v)"
              />
            </div>
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
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-brush-mask"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
                  @click="addMask('brush')"
                >
                  + Brush
                </button>
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-luminance-mask"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
                  @click="addLuminanceMask"
                >
                  + Luminance range
                </button>
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-colour-mask"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT"
                  @click="startColourRangeEyedropper"
                >
                  + Colour range
                </button>
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-subject-mask"
                  title="Find the main subject (a model, downloaded once)"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
                  @click="addAutoMask('subject')"
                >
                  + Subject
                </button>
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-sky-mask"
                  title="Find the sky (a model, downloaded once)"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
                  @click="addAutoMask('sky')"
                >
                  + Sky
                </button>
                <button
                  type="button"
                  class="secondary"
                  data-testid="add-background-mask"
                  title="Find the background (subject model inverted)"
                  :disabled="!sessionId || isReadOnly || masks.length >= MASK_LIMIT || !!autoBusy"
                  @click="addBackgroundMask"
                >
                  + Background
                </button>
              </div>

              <div v-if="autoDownload" class="effects-group" data-testid="auto-download">
                <p class="mask-note">
                  Finding the {{ AUTO_LABEL[autoDownload.target].toLowerCase() }} needs a model,
                  downloaded once and kept for every photograph: {{ autoDownload.name }}.
                </p>
                <div v-if="!autoDownload.jobId" class="brush-tool__row">
                  <button
                    type="button"
                    class="primary"
                    data-testid="auto-download-start"
                    @click="startModelDownload"
                  >
                    Download
                  </button>
                  <button
                    type="button"
                    class="ghost"
                    data-testid="auto-download-cancel"
                    @click="autoDownload = null"
                  >
                    Not now
                  </button>
                </div>
                <JobProgress
                  v-else
                  :job-id="autoDownload.jobId"
                  :can-cancel="true"
                  @cancel="cancelModelDownload"
                  @done="onModelDownloaded"
                />
              </div>

              <div v-if="autoBusy" class="auto-busy" role="status" data-testid="auto-busy">
                <span>
                  Finding the {{ AUTO_LABEL[autoBusy].toLowerCase() }}<span class="edit-screen__cursor">_</span>
                  a few seconds
                </span>
                <button type="button" class="ghost" data-testid="auto-stop" @click="stopAutoMask">
                  Stop
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

            </div>
          </CollapsibleSection>

          <p v-if="selectedMask" class="mask-note" data-testid="global-put-away">
            The whole photograph's adjustments are put away while a mask is edited.
          </p>

          <!-- 2. Tone Curve -->
          <CollapsibleSection
            title="Tone Curve"
            test-id="section-tone-curve"
            v-show="!selectedMask"
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
            v-show="!selectedMask"
            :default-open="false"
            @reset="resetHsl"
          >
            <div class="colour-mode-bar">
              <button
                type="button"
                class="ghost bw-toggle-btn"
                data-testid="bw-toggle-btn"
                :class="{ active: isBwActive }"
                title="Toggle Black & White (V)"
                @click="toggleBlackAndWhite"
              >
                Black & White
              </button>
            </div>

            <!-- B&W Mix Panel (RP-4) -->
            <div v-if="isBwActive" class="bw-mix-panel" data-testid="bw-mix-panel">
              <div class="bw-mix-header">
                <span class="bw-mix-title">B&W Mix</span>
                <button
                  type="button"
                  class="ghost bw-mix-reset-btn"
                  data-testid="bw-mix-reset-btn"
                  :disabled="isBwMixDefault || loading || isReadOnly"
                  @click="resetBwMix"
                >
                  Reset Mix
                </button>
              </div>

              <div class="bw-mix-sliders">
                <AdjustmentSlider
                  v-for="band in BW_BANDS"
                  :key="band.key"
                  :label="band.label"
                  :model-value="getBwBand(band.key)"
                  :min="-100"
                  :max="100"
                  :step="1"
                  :disabled="loading || isReadOnly"
                  :test-id="`bw-slider-${band.key}`"
                  @update:model-value="(v) => onBwSliderInput(band.key, v)"
                  @change="(v) => onBwSliderChange(band.key, v)"
                />
              </div>
            </div>

            <!-- HSL Panel -->
            <div v-else class="hsl-panel" data-testid="hsl-panel">
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

            <!-- Point Colour Group (RP-10) -->
            <div v-if="!isBwActive" class="point-colour-panel" data-testid="point-colour-panel">
              <div class="point-colour-header">
                <span class="point-colour-title">Point colour</span>
                <button
                  type="button"
                  class="ghost point-colour-pick-btn"
                  :class="{ active: eyedropperTarget === 'point-colour' }"
                  :disabled="loading || isReadOnly || (recipe.point_colours?.length ?? 0) >= 8"
                  data-testid="point-colour-pick-btn"
                  :title="(recipe.point_colours?.length ?? 0) >= 8 ? 'Maximum 8 points reached' : 'Pick a colour'"
                  @click="togglePointColourEyedropper"
                >
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M14 2a2 2 0 0 0-2.83 0L9.41 3.76l2.83 2.83L14 4.83A2 2 0 0 0 14 2z"/>
                    <path d="M8 5.17L2.34 10.83a2 2 0 0 0-.54.97L1 15l3.2-.8a2 2 0 0 0 .97-.54L10.83 8"/>
                  </svg>
                  Pick a colour
                </button>
              </div>

              <div
                v-if="(recipe.point_colours?.length ?? 0) >= 8"
                class="point-colour-limit-msg"
                data-testid="point-colour-limit-msg"
              >
                Maximum 8 points reached
              </div>

              <!-- Swatches list -->
              <div
                v-if="(recipe.point_colours?.length ?? 0) > 0"
                class="point-colour-swatches"
                role="tablist"
                aria-label="Point Colours"
              >
                <button
                  v-for="(pt, idx) in recipe.point_colours"
                  :key="idx"
                  type="button"
                  class="point-colour-swatch"
                  :class="{ active: selectedPointIndex === idx }"
                  :style="{ backgroundColor: `oklch(${pt.lightness} ${pt.chroma} ${pt.hue}deg)` }"
                  :title="`Point colour ${idx + 1}`"
                  :aria-label="`Point colour ${idx + 1}`"
                  :aria-selected="selectedPointIndex === idx"
                  role="tab"
                  tabindex="0"
                  :data-testid="`point-colour-swatch-${idx}`"
                  @click="selectPoint(idx)"
                />
              </div>

              <!-- Selected point controls -->
              <div v-if="selectedPoint" class="point-colour-controls" data-testid="point-colour-controls">
                <AdjustmentSlider
                  label="Hue"
                  :model-value="selectedPoint.hue_shift"
                  :min="-100"
                  :max="100"
                  :step="1"
                  unit="°"
                  :disabled="loading || isReadOnly"
                  test-id="point-colour-hue"
                  @update:model-value="(v) => onPointColourSliderInput('hue_shift', v)"
                  @change="(v) => onPointColourSliderChange('hue_shift', v)"
                />

                <AdjustmentSlider
                  label="Saturation"
                  :model-value="selectedPoint.saturation"
                  :min="-100"
                  :max="100"
                  :step="1"
                  unit="%"
                  :disabled="loading || isReadOnly"
                  test-id="point-colour-sat"
                  @update:model-value="(v) => onPointColourSliderInput('saturation', v)"
                  @change="(v) => onPointColourSliderChange('saturation', v)"
                />

                <AdjustmentSlider
                  label="Luminance"
                  :model-value="selectedPoint.luminance"
                  :min="-100"
                  :max="100"
                  :step="1"
                  unit="%"
                  :disabled="loading || isReadOnly"
                  test-id="point-colour-lum"
                  @update:model-value="(v) => onPointColourSliderInput('luminance', v)"
                  @change="(v) => onPointColourSliderChange('luminance', v)"
                />

                <AdjustmentSlider
                  label="Range"
                  :model-value="selectedPoint.range"
                  :min="0"
                  :max="100"
                  :step="1"
                  :disabled="loading || isReadOnly"
                  test-id="point-colour-range"
                  @update:model-value="(v) => onPointColourSliderInput('range', v)"
                  @change="(v) => onPointColourSliderChange('range', v)"
                />

                <div class="point-colour-actions">
                  <button
                    type="button"
                    class="ghost point-colour-delete-btn"
                    data-testid="point-colour-delete-btn"
                    :disabled="loading || isReadOnly"
                    @click="deleteSelectedPoint"
                  >
                    Delete
                  </button>
                </div>
              </div>
            </div>
          </CollapsibleSection>

          <!-- 4. Colour Grading -->
          <CollapsibleSection
            title="Colour Grading"
            test-id="section-grading"
            v-show="!selectedMask"
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
            v-show="!selectedMask"
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

          <!-- Detail (RP-7) -->
          <CollapsibleSection
            title="Detail"
            test-id="section-detail"
            v-show="!selectedMask"
            :default-open="false"
            @reset="resetDetail"
          >
            <div class="detail-panel" data-testid="detail-panel">
              <div
                v-if="zoom === 'fit'"
                class="detail-hint"
                data-testid="detail-hint"
              >
                Judge detail at 100% (Z)
              </div>

              <div class="detail-group">
                <div class="detail-group-title">Sharpening</div>
                <AdjustmentSlider
                  label="Amount"
                  :model-value="recipe.detail?.sharpen_amount ?? 0"
                  :min="0"
                  :max="150"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="detail-sharpen-amount"
                  @update:model-value="(v) => onDetailSliderInput('sharpen_amount', v)"
                  @change="(v) => onDetailSliderChange('sharpen_amount', v)"
                />
                <AdjustmentSlider
                  label="Radius"
                  :model-value="recipe.detail?.sharpen_radius ?? 1.0"
                  :min="0.5"
                  :max="3.0"
                  :step="0.1"
                  :default-value="1.0"
                  :disabled="loading || isReadOnly"
                  test-id="detail-sharpen-radius"
                  @update:model-value="(v) => onDetailSliderInput('sharpen_radius', v)"
                  @change="(v) => onDetailSliderChange('sharpen_radius', v)"
                />
                <AdjustmentSlider
                  label="Masking"
                  :model-value="recipe.detail?.sharpen_masking ?? 0"
                  :min="0"
                  :max="100"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="detail-sharpen-masking"
                  @update:model-value="(v) => onDetailSliderInput('sharpen_masking', v)"
                  @change="(v) => onDetailSliderChange('sharpen_masking', v)"
                />
              </div>

              <div class="detail-group">
                <div class="detail-group-title">Noise reduction</div>
                <AdjustmentSlider
                  label="Luminance"
                  :model-value="recipe.detail?.noise_luminance ?? 0"
                  :min="0"
                  :max="100"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="detail-noise-luminance"
                  @update:model-value="(v) => onDetailSliderInput('noise_luminance', v)"
                  @change="(v) => onDetailSliderChange('noise_luminance', v)"
                />
                <AdjustmentSlider
                  label="Colour"
                  :model-value="recipe.detail?.noise_colour ?? 0"
                  :min="0"
                  :max="100"
                  :step="1"
                  :default-value="0"
                  :disabled="loading || isReadOnly"
                  test-id="detail-noise-colour"
                  @update:model-value="(v) => onDetailSliderInput('noise_colour', v)"
                  @change="(v) => onDetailSliderChange('noise_colour', v)"
                />
              </div>

              <div class="detail-group detail-group--toggle">
                <button
                  type="button"
                  class="ghost hot-pixels-toggle-btn"
                  data-testid="detail-hot-pixels-toggle"
                  :class="{ active: recipe.detail?.hot_pixels ?? false }"
                  :disabled="loading || isReadOnly"
                  title="Remove hot pixels"
                  @click="toggleHotPixels"
                >
                  Remove hot pixels
                </button>
              </div>
            </div>
          </CollapsibleSection>

          <!-- 6. Geometry -->
          <CollapsibleSection
            title="Geometry"
            test-id="section-geometry"
            v-show="!selectedMask"
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
            v-show="!selectedMask"
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

          <!-- 8. History (RP-5) -->
          <CollapsibleSection
            title="History"
            test-id="section-history"
            v-show="!selectedMask"
            :default-open="false"
          >
            <div class="history-panel" data-testid="history-panel">
              <div v-if="historyEntries.length === 0" class="history-empty" data-testid="history-empty">
                No history
              </div>
              <ul v-else class="history-list" data-testid="history-list">
                <li
                  v-for="item in reversedHistory"
                  :key="item.index"
                  class="history-item"
                  :class="{ 'history-item--current': item.index === historyCursor }"
                  :data-testid="`history-item-${item.index}`"
                  :data-current="item.index === historyCursor ? 'true' : undefined"
                  @click="jumpToHistory(item.index)"
                >
                  <span class="history-item__label">{{ item.label }}</span>
                  <span v-if="item.index === historyCursor" class="history-item__current-badge">Current</span>
                </li>
              </ul>
            </div>
          </CollapsibleSection>

          <!-- 9. Snapshots (RP-5) -->
          <CollapsibleSection
            title="Snapshots"
            test-id="section-snapshots"
            v-show="!selectedMask"
            :default-open="false"
          >
            <div class="snapshots-panel" data-testid="snapshots-panel">
              <div class="snapshots-form">
                <input
                  v-model="newSnapshotName"
                  type="text"
                  placeholder="Snapshot name"
                  class="snapshots-input"
                  data-testid="snapshot-name-input"
                  :disabled="loading || !sessionId"
                  @keydown.enter="saveSnapshot"
                />
                <button
                  type="button"
                  class="snapshots-save-btn"
                  data-testid="snapshot-save-btn"
                  :disabled="loading || !sessionId"
                  @click="saveSnapshot"
                >
                  Save snapshot
                </button>
              </div>
              <div
                v-if="snapshotMessage"
                class="snapshots-message"
                data-testid="snapshot-message"
              >
                {{ snapshotMessage }}
              </div>
              <div v-if="!recipe.snapshots || recipe.snapshots.length === 0" class="snapshots-empty" data-testid="snapshots-empty">
                No snapshots
              </div>
              <ul v-else class="snapshots-list" data-testid="snapshots-list">
                <li
                  v-for="snap in recipe.snapshots"
                  :key="snap.name"
                  class="snapshot-item"
                  :data-testid="`snapshot-item-${snap.name}`"
                >
                  <span class="snapshot-item__name">{{ snap.name }}</span>
                  <div class="snapshot-item__actions">
                    <button
                      type="button"
                      class="snapshot-action-btn"
                      :data-testid="`snapshot-restore-${snap.name}`"
                      :disabled="loading || !sessionId"
                      @click="restoreSnapshot(snap)"
                    >
                      Restore
                    </button>
                    <button
                      type="button"
                      class="snapshot-action-btn snapshot-action-btn--delete"
                      :data-testid="`snapshot-delete-${snap.name}`"
                      :disabled="loading || !sessionId"
                      @click="deleteSnapshot(snap.name)"
                    >
                      Delete
                    </button>
                  </div>
                </li>
              </ul>
            </div>
          </CollapsibleSection>

          <!-- 10. Versions (RP-6) -->
          <CollapsibleSection
            title="Versions"
            test-id="section-versions"
            v-show="!selectedMask"
            :default-open="false"
          >
            <div class="versions-panel" data-testid="versions-panel">
              <div class="current-version-card" data-testid="current-version-card">
                <div class="current-version-header">
                  <span class="current-version-badge">Current</span>
                  <span class="current-version-name" data-testid="current-version-name">{{ currentVersionName }}</span>
                </div>
                <div v-if="editingCurrentVersion" class="version-rename-form">
                  <input
                    v-model="renameCurrentInput"
                    type="text"
                    class="version-input"
                    data-testid="rename-current-input"
                    placeholder="New name"
                    @keydown.enter="submitRenameCurrent"
                    @keydown.esc="editingCurrentVersion = false"
                  />
                  <button
                    type="button"
                    class="version-btn version-btn--save"
                    data-testid="rename-current-save-btn"
                    @click="submitRenameCurrent"
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    class="version-btn"
                    data-testid="rename-current-cancel-btn"
                    @click="editingCurrentVersion = false"
                  >
                    Cancel
                  </button>
                </div>
                <div v-else class="current-version-actions">
                  <button
                    type="button"
                    class="version-btn"
                    data-testid="rename-current-btn"
                    :disabled="loading || !sessionId"
                    @click="startRenameCurrent"
                  >
                    Rename
                  </button>
                </div>
              </div>

              <div class="versions-new-form">
                <input
                  v-model="newVersionInput"
                  type="text"
                  placeholder="Version name"
                  class="version-input"
                  data-testid="version-name-input"
                  :disabled="loading || !sessionId"
                  @keydown.enter="createVersion"
                />
                <button
                  type="button"
                  class="version-btn version-btn--primary"
                  data-testid="version-create-btn"
                  :disabled="loading || !sessionId"
                  @click="createVersion"
                >
                  New version
                </button>
              </div>

              <div
                v-if="versionMessage"
                class="version-message"
                data-testid="version-message"
              >
                {{ versionMessage }}
              </div>

              <div v-if="!recipe.versions || recipe.versions.length === 0" class="versions-empty" data-testid="versions-empty">
                No other versions
              </div>
              <ul v-else class="versions-list" data-testid="versions-list">
                <li
                  v-for="v in recipe.versions"
                  :key="v.name"
                  class="version-item"
                  :data-testid="`version-item-${v.name}`"
                >
                  <div v-if="editingVersionName === v.name" class="version-rename-form">
                    <input
                      v-model="renameOtherInput"
                      type="text"
                      class="version-input"
                      :data-testid="`rename-input-${v.name}`"
                      placeholder="New name"
                      @keydown.enter="submitRenameOther(v.name)"
                      @keydown.esc="editingVersionName = null"
                    />
                    <button
                      type="button"
                      class="version-btn version-btn--save"
                      :data-testid="`rename-save-${v.name}`"
                      @click="submitRenameOther(v.name)"
                    >
                      Save
                    </button>
                    <button
                      type="button"
                      class="version-btn"
                      :data-testid="`rename-cancel-${v.name}`"
                      @click="editingVersionName = null"
                    >
                      Cancel
                    </button>
                  </div>
                  <div v-else class="version-item__row">
                    <span class="version-item__name" :data-testid="`version-name-${v.name}`">{{ v.name }}</span>
                    <div class="version-item__actions">
                      <button
                        type="button"
                        class="version-btn"
                        :data-testid="`version-switch-${v.name}`"
                        :disabled="loading || !sessionId"
                        @click="switchVersion(v.name)"
                      >
                        Switch
                      </button>
                      <button
                        type="button"
                        class="version-btn"
                        :data-testid="`version-rename-${v.name}`"
                        :disabled="loading || !sessionId"
                        @click="startRenameOther(v.name)"
                      >
                        Rename
                      </button>
                      <button
                        type="button"
                        class="version-btn version-btn--delete"
                        :data-testid="`version-delete-${v.name}`"
                        :disabled="loading || !sessionId"
                        @click="deleteVersion(v.name)"
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                </li>
              </ul>
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

          <!-- Format selection -->
          <div class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Format</label>
            <select
              v-model="exportState.formatKind"
              class="adjustment-panel__export-select"
              data-testid="export-format-select"
            >
              <option value="AsSource">As source</option>
              <option value="Jpeg">JPEG</option>
              <option value="Tiff">TIFF</option>
              <option value="Png">PNG</option>
              <option value="Webp">WebP</option>
              <option value="Avif">AVIF</option>
            </select>
          </div>

          <!-- Quality for JPEG -->
          <div v-if="exportState.formatKind === 'Jpeg'" class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Quality (1–100)</label>
            <input
              v-model.number="exportState.jpegQuality"
              type="number"
              min="1"
              max="100"
              class="adjustment-panel__export-input"
              data-testid="export-quality-input"
            />
          </div>

          <!-- Quality for AVIF -->
          <div v-if="exportState.formatKind === 'Avif'" class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Quality (1–100)</label>
            <input
              v-model.number="exportState.avifQuality"
              type="number"
              min="1"
              max="100"
              class="adjustment-panel__export-input"
              data-testid="export-quality-input"
            />
          </div>

          <!-- Bits for TIFF -->
          <div v-if="exportState.formatKind === 'Tiff'" class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Bit depth</label>
            <select
              v-model.number="exportState.tiffBits"
              class="adjustment-panel__export-select"
              data-testid="export-bits-select"
            >
              <option :value="8">8-bit</option>
              <option :value="16">16-bit</option>
            </select>
          </div>

          <!-- Bits for PNG -->
          <div v-if="exportState.formatKind === 'Png'" class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Bit depth</label>
            <select
              v-model.number="exportState.pngBits"
              class="adjustment-panel__export-select"
              data-testid="export-bits-select"
            >
              <option :value="8">8-bit</option>
              <option :value="16">16-bit</option>
            </select>
          </div>

          <!-- WebP notice -->
          <div v-if="exportState.formatKind === 'Webp'" class="adjustment-panel__export-notice" data-testid="export-webp-notice">
            WebP is lossless.
          </div>

          <!-- Size options -->
          <div class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Size</label>
            <select
              v-model="exportState.resizeKind"
              class="adjustment-panel__export-select"
              data-testid="export-size-select"
            >
              <option value="Full">Full</option>
              <option value="LongEdge">Long edge</option>
              <option value="Megapixels">Megapixels</option>
            </select>
          </div>

          <!-- Numeric input for LongEdge or Megapixels -->
          <div v-if="exportState.resizeKind !== 'Full'" class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">
              {{ exportState.resizeKind === 'LongEdge' ? 'Long edge (px)' : 'Megapixels (MP)' }}
            </label>
            <input
              v-model="exportState.resizeValue"
              type="number"
              min="1"
              class="adjustment-panel__export-input"
              data-testid="export-size-input"
            />
          </div>

          <!-- Output sharpening -->
          <div class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Output sharpening</label>
            <select
              v-model="exportState.outputSharpening"
              class="adjustment-panel__export-select"
              data-testid="export-sharpening-select"
            >
              <option value="Off">Off</option>
              <option value="Screen">Screen</option>
              <option value="Print">Print</option>
            </select>
          </div>

          <!-- Name template -->
          <div class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">File name template</label>
            <input
              v-model="exportState.nameTemplate"
              type="text"
              placeholder="{name}{edit}"
              class="adjustment-panel__export-input"
              data-testid="export-template-input"
            />
            <span class="adjustment-panel__export-tokens" data-testid="export-template-tokens">
              Tokens: {name}, {version}, {date}, {edit}
            </span>
          </div>

          <!-- Live example -->
          <div class="adjustment-panel__export-example" data-testid="export-name-example">
            Example: {{ liveExampleName }}
          </div>

          <!-- Metadata policy -->
          <div class="adjustment-panel__export-row">
            <label class="adjustment-panel__export-label">Metadata</label>
            <select
              v-model="exportState.metadataPolicy"
              class="adjustment-panel__export-select"
              data-testid="export-metadata-select"
            >
              <option value="All">All</option>
              <option value="NoLocation">No location</option>
              <option value="None">None</option>
            </select>
          </div>

          <!-- Size validation error message -->
          <div v-if="!isResizeValid" class="adjustment-panel__export-size-error" data-testid="export-size-error">
            {{ resizeInvalidReason }}
          </div>

          <div class="adjustment-panel__export-actions">
            <button
              type="button"
              class="primary adjustment-panel__export-btn"
              data-testid="export-btn"
              :disabled="!sessionId || !exportDir || isExportingAll || !isResizeValid"
              @click="handleExport"
            >
              Export render
            </button>
            <button
              v-if="hasOtherVersions"
              type="button"
              class="adjustment-panel__export-all-btn"
              data-testid="export-all-btn"
              :disabled="!sessionId || !exportDir || isExportingAll || !isResizeValid"
              @click="exportAllVersions"
            >
              {{ isExportingAll ? exportAllProgressText : 'Export all versions' }}
            </button>
          </div>

          <div v-if="exportAllSummary" class="adjustment-panel__export-all-summary" data-testid="export-all-summary">
            {{ exportAllSummary }}
          </div>

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
  display: block;
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  user-select: none;
}

.canvas-viewport__container.is-side-by-side {
  display: flex;
  flex-direction: row;
}

.canvas-viewport__container.is-space-held {
  cursor: grab !important;
}

.canvas-viewport__container.is-panning {
  cursor: grabbing !important;
}

.canvas-viewport__single {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
}

.canvas-viewport__half {
  flex: 1;
  position: relative;
  height: 100%;
  overflow: hidden;
  border-right: 1px solid var(--border-hair);
}

.canvas-viewport__half:last-child {
  border-right: none;
}

.canvas-viewport__half-pane {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
}

.canvas-viewport__loading-indicator {
  position: absolute;
  top: var(--space-3);
  left: 50%;
  transform: translateX(-50%);
  background: rgba(10, 10, 15, 0.85);
  color: var(--text-muted);
  border: var(--border-hair);
  border-radius: var(--radius-sm);
  padding: var(--space-1) var(--space-3);
  font-size: 11px;
  font-family: var(--font-sans);
  pointer-events: none;
  z-index: calc(var(--z-canvas) + 10);
  backdrop-filter: blur(4px);
}

.canvas-viewport__badge {
  position: absolute;
  top: var(--space-3);
  background: rgba(10, 10, 15, 0.7);
  color: var(--text-muted);
  border: var(--border-hair);
  border-radius: var(--radius-sm);
  padding: 2px var(--space-2);
  font-size: 11px;
  font-family: var(--font-sans);
  pointer-events: none;
  z-index: calc(var(--z-canvas) + 5);
}

.canvas-viewport__half--before .canvas-viewport__badge {
  left: var(--space-3);
}

.canvas-viewport__half--after .canvas-viewport__badge {
  right: var(--space-3);
}

.canvas-viewport__badge--split-before {
  left: var(--space-3);
}

.canvas-viewport__badge--split-after {
  right: var(--space-3);
}

.canvas-viewport__split-divider {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 40px;
  margin-left: -20px;
  cursor: ew-resize;
  z-index: calc(var(--z-canvas) + 6);
  display: flex;
  align-items: center;
  justify-content: center;
}

.canvas-viewport__split-line {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 19px;
  width: 2px;
  background: var(--accent);
  pointer-events: none;
}

.canvas-viewport__split-handle {
  position: relative;
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm);
  background: var(--surface-raised);
  border: var(--border-hair);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.5);
  cursor: ew-resize;
}

.canvas-viewport__view-btn {
  min-height: 40px;
  min-width: 40px;
  padding: var(--space-2) var(--space-3);
  font-size: 12px;
}

.canvas-viewport__view-btn.active {
  background: var(--accent);
  color: var(--void);
}

.canvas-viewport__zoom-readout {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 40px;
  min-width: 48px;
  padding: 0 var(--space-2);
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
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

.adjustment-panel__export-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.adjustment-panel__export-label {
  font-family: var(--font-label, sans-serif);
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted, #888);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.adjustment-panel__export-select,
.adjustment-panel__export-input {
  min-height: 40px;
  height: 40px;
  padding: 0 var(--space-2, 8px);
  background: var(--bg-secondary, #1a1a1a);
  color: var(--text-primary, #fff);
  border: var(--border-hair, 1px solid #333);
  border-radius: var(--radius-sm, 2px);
  font-size: 13px;
  box-sizing: border-box;
  width: 100%;
}

.adjustment-panel__export-select:focus,
.adjustment-panel__export-input:focus {
  outline: none;
  border-color: var(--accent, #3b82f6);
}

.adjustment-panel__export-notice {
  font-size: 12px;
  color: var(--text-muted, #888);
  font-style: italic;
}

.adjustment-panel__export-tokens {
  font-size: 11px;
  color: var(--text-muted, #888);
  margin-top: 2px;
}

.adjustment-panel__export-example {
  font-size: 12px;
  color: var(--text-muted, #888);
  font-family: var(--font-mono, monospace);
  word-break: break-all;
}

.adjustment-panel__export-size-error {
  font-size: 12px;
  color: var(--color-error, #f44336);
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

.point-colour-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: var(--border-hair);
}

.point-colour-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 40px;
}

.point-colour-title {
  font-family: var(--font-label);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-heading);
}

.point-colour-pick-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  cursor: pointer;
}

.point-colour-pick-btn.active {
  color: var(--accent);
  border-color: var(--accent);
}

.point-colour-pick-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.point-colour-limit-msg {
  font-family: var(--font-body);
  font-size: 12px;
  color: var(--text-muted);
  font-style: italic;
  padding: var(--space-1) 0;
}

.point-colour-swatches {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding: var(--space-1) 0;
}

.point-colour-swatch {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  min-width: 40px;
  min-height: 40px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  transition: transform var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.point-colour-swatch:hover {
  transform: translateY(-1px);
  border-color: var(--border-strong);
}

.point-colour-swatch.active {
  border: var(--border-active);
  box-shadow: 0 0 0 1px var(--accent);
}

.point-colour-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-1);
}

.point-colour-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: var(--space-1);
}

.point-colour-delete-btn {
  min-height: 40px;
  min-width: 80px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  cursor: pointer;
}

.point-colour-delete-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.point-colour-delete-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
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
  grid-template-columns: 1fr 1fr 1fr;
  gap: var(--space-2);
}

.mask-add button {
  min-height: 40px;
  padding: 0 var(--space-1);
  font-size: 12px;
  letter-spacing: 0.04em;
  white-space: nowrap;
}

.auto-busy {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--accent);
}

.auto-busy button {
  min-height: 40px;
  font-size: 12px;
}

.mask-editing {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-3);
  border: var(--border-active);
  background: var(--bg-panel);
}

.mask-editing__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.mask-editing__title {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.mask-editing__title strong {
  color: var(--accent);
  font-weight: normal;
}

.mask-editing__head button {
  min-height: 40px;
  padding: 0 var(--space-4);
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

.brush-tool {
  display: grid;
  gap: var(--space-2);
}

.brush-tool__row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-2);
}

.brush-tool__row button {
  padding: 0 var(--space-1);
  letter-spacing: 0.04em;
  white-space: nowrap;
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

.mask-components {
  display: grid;
  gap: var(--space-2);
}

.component-list {
  display: grid;
  gap: var(--space-1);
}

.component-row {
  display: grid;
  grid-template-columns: 1fr auto 44px 40px;
  gap: var(--space-1);
  align-items: center;
  border: var(--border-hair);
  background: var(--bg-panel);
  min-height: 40px;
}

.component-row--base {
  grid-template-columns: 1fr;
}

.component-row.is-selected {
  border: var(--border-active);
}

.component-row button {
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-2);
  font-size: 12px;
}

.component-select {
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.component-op-select {
  min-height: 40px;
  min-width: 80px;
  background: var(--bg-input);
  color: var(--text-base);
  border: var(--border-hair);
  border-radius: var(--radius-sm);
  padding: 0 var(--space-2);
  font-size: 12px;
}

.component-invert.active {
  color: var(--accent);
  border: var(--border-active);
}

.component-add-menu {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: var(--space-1);
}

.component-add-menu button {
  min-height: 40px;
  padding: 0 var(--space-1);
  font-size: 12px;
  letter-spacing: 0.04em;
  white-space: nowrap;
}

.range-controls {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2);
  background: var(--bg-surface);
  border: var(--border-hair);
}

.range-controls button {
  min-height: 40px;
  font-size: 12px;
}

.effects-group-title {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

/* Eyedropper and WB row (RP-4) */
.wb-slider-row {
  display: flex;
  align-items: flex-end;
  gap: var(--space-2);
}

.wb-slider-row :deep(.adjustment-slider) {
  flex: 1;
}

.eyedropper-btn {
  min-height: 44px;
  min-width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  color: var(--text-muted);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.eyedropper-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.eyedropper-btn.active {
  background: var(--accent);
  color: #000;
  border-color: var(--accent);
}

.eyedropper-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.canvas-viewport__container.is-eyedropper,
.canvas-viewport__container.is-eyedropper canvas,
.cursor-crosshair {
  cursor: crosshair !important;
}

/* Auto Tone button (RP-4) */
.auto-tone-btn {
  min-height: 40px;
  min-width: 44px;
  padding: 0 var(--space-2);
  font-family: var(--font-label);
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.auto-tone-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.auto-tone-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* Colour mode bar and Black & White mix (RP-4) */
.colour-mode-bar {
  display: flex;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.bw-toggle-btn {
  flex: 1;
  min-height: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.bw-toggle-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.bw-toggle-btn.active {
  background: var(--accent);
  color: #000;
  border-color: var(--accent);
}

.bw-mix-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.bw-mix-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: var(--space-1);
  border-bottom: 1px solid var(--border);
}

.bw-mix-title {
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-heading);
}

.bw-mix-reset-btn {
  font-family: var(--font-label);
  font-size: 12px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: var(--space-1) var(--space-2);
  min-height: 40px;
  min-width: 40px;
  border-radius: var(--radius-none);
  transition: color var(--dur-fast) var(--ease);
}

.bw-mix-reset-btn:hover:not(:disabled) {
  color: var(--text-heading);
}

.bw-mix-reset-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.bw-mix-sliders {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

/* --- History Section (RP-5) --------------------------------------------- */
.history-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.history-empty {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  padding: var(--space-2) 0;
}

.history-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.history-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  user-select: none;
  transition: border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.history-item:hover {
  border-color: var(--text-muted);
}

.history-item--current {
  border-color: var(--accent);
  background: var(--bg-hover);
  font-weight: 600;
}

.history-item__current-badge {
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--accent);
}

/* --- Snapshots Section (RP-5) ------------------------------------------- */
.snapshots-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.snapshots-form {
  display: flex;
  gap: var(--space-2);
}

.snapshots-input {
  flex: 1;
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 14px;
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  box-sizing: border-box;
}

.snapshots-input:focus {
  outline: none;
  border-color: var(--accent);
}

.snapshots-save-btn {
  min-height: 40px;
  min-width: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-heading);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  white-space: nowrap;
  transition: border-color var(--dur-fast) var(--ease);
}

.snapshots-save-btn:hover:not(:disabled) {
  border-color: var(--accent);
}

.snapshots-save-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.snapshots-message {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--accent);
}

.snapshots-empty {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  padding: var(--space-1) 0;
}

.snapshots-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.snapshot-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 40px;
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  gap: var(--space-2);
}

.snapshot-item__name {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.snapshot-item__actions {
  display: flex;
  gap: var(--space-1);
}

.snapshot-action-btn {
  min-height: 40px;
  min-width: 40px;
  padding: var(--space-1) var(--space-3);
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-muted);
  background: transparent;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}

.snapshot-action-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--text-muted);
}

.snapshot-action-btn--delete:hover:not(:disabled) {
  color: var(--accent);
  border-color: var(--accent);
}

.snapshot-action-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

/* --- Versions Section (RP-6) -------------------------------------------- */
.versions-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.current-version-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2);
  border: 1px solid var(--border);
  background: var(--bg-panel);
}

.current-version-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 24px;
}

.current-version-badge {
  font-family: var(--font-label);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--accent);
  font-weight: 600;
}

.current-version-name {
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  color: var(--text-heading);
}

.current-version-actions {
  display: flex;
  gap: var(--space-2);
}

.versions-new-form {
  display: flex;
  gap: var(--space-2);
}

.version-input {
  flex: 1;
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 14px;
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  box-sizing: border-box;
}

.version-input:focus {
  outline: none;
  border-color: var(--accent);
}

.version-btn {
  min-height: 40px;
  min-width: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-heading);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  white-space: nowrap;
  box-sizing: border-box;
  transition: border-color var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
}

.version-btn:hover:not(:disabled) {
  border-color: var(--accent);
}

.version-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.version-btn--primary {
  background: var(--bg-panel);
  border-color: var(--border);
}

.version-btn--save {
  background: var(--bg-panel);
}

.version-btn--delete:hover:not(:disabled) {
  color: var(--accent);
  border-color: var(--accent);
}

.version-message {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--accent);
}

.versions-empty {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text-muted);
  padding: var(--space-1) 0;
}

.versions-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.version-item {
  display: flex;
  flex-direction: column;
  min-height: 40px;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  box-sizing: border-box;
}

.version-item__row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 40px;
  padding: var(--space-1) var(--space-2);
  gap: var(--space-2);
}

.version-item__name {
  font-family: var(--font-label);
  font-size: 13px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.version-item__actions {
  display: flex;
  gap: var(--space-1);
}

.version-rename-form {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-1);
}

.adjustment-panel__export-actions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.adjustment-panel__export-all-btn {
  min-height: 40px;
  padding: var(--space-2) var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-heading);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  cursor: pointer;
  white-space: nowrap;
  box-sizing: border-box;
  transition: border-color var(--dur-fast) var(--ease);
}

.adjustment-panel__export-all-btn:hover:not(:disabled) {
  border-color: var(--accent);
}

.adjustment-panel__export-all-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.adjustment-panel__export-all-summary {
  font-family: var(--font-label);
  font-size: 12px;
  color: var(--text-heading);
  padding: var(--space-1) 0;
}

/* Presence Section (RP-8) */
.presence-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) 0;
}

/* Detail Section (RP-7) */
.detail-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}

.detail-hint {
  font-family: var(--font-label);
  font-size: 11px;
  font-style: italic;
  color: var(--text-muted);
  padding: var(--space-1) var(--space-2);
  line-height: 1.4;
}

.detail-group {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.detail-group-title {
  font-family: var(--font-label);
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  padding: 0 var(--space-2);
}

.detail-group--toggle {
  margin-top: var(--space-1);
}

.hot-pixels-toggle-btn {
  width: 100%;
  min-height: 40px;
  min-width: 40px;
  padding: 0 var(--space-3);
  font-family: var(--font-label);
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

.hot-pixels-toggle-btn:hover:not(:disabled) {
  color: var(--text-heading);
  border-color: var(--accent);
}

.hot-pixels-toggle-btn.active {
  background: var(--accent);
  color: #000;
  border-color: var(--accent);
}
</style>
