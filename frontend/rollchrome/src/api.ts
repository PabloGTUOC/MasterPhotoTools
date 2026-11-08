/**
 * The Tauri transport for Rollchrome.
 *
 * Rollchrome is desktop-only (`docs/rollchrome-plan.md`, decision 2): every
 * operation calls `rollchrome-core` or `phototools-core` on this Mac through
 * `invoke`, and there is no HTTP client, ever.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type {
  BrowserEntry,
  Job,
  JobEvent,
  JobStatus,
} from '@phototools/shared';

/** Statuses a job never leaves again, mirroring `JobStatus::is_terminal`. */
const TERMINAL: JobStatus[] = ['completed', 'failed', 'interrupted', 'cancelled'];

/** The Tauri event Rollchrome's job runner emits through. */
const JOB_EVENT = 'rollchrome://job';

export class RollchromeClient {
  roots(): Promise<string[]> {
    return invoke<string[]>('list_roots');
  }

  list(path: string): Promise<BrowserEntry[]> {
    return invoke<BrowserEntry[]>('list_directory', { path });
  }

  async job(id: string): Promise<Job> {
    const job = await invoke<Job | null>('get_job', { id });
    if (!job) throw new Error(`No job ${id}`);
    return job;
  }

  /**
   * Follow a job through Tauri events rather than SSE.
   *
   * F17 names both transports; this is the desktop half. Resolves on the
   * terminal update, or when the caller aborts.
   */
  async watchJob(
    id: string,
    onEvent: (event: JobEvent) => void,
    signal?: AbortSignal,
  ): Promise<void> {
    if (!id) return;

    return new Promise<void>((resolve, reject) => {
      let unlisten: (() => void) | null = null;
      let settled = false;

      const finish = (error?: Error) => {
        if (settled) return;
        settled = true;
        unlisten?.();
        signal?.removeEventListener('abort', onAbort);
        if (error) reject(error);
        else resolve();
      };

      const onAbort = () => {
        const error = new Error('aborted');
        error.name = 'AbortError';
        finish(error);
      };

      if (signal?.aborted) return onAbort();
      signal?.addEventListener('abort', onAbort);

      listen<JobEvent>(JOB_EVENT, (event) => {
        if (event.payload.id !== id) return;
        onEvent(event.payload);
        if (event.payload.terminal) finish();
      })
        .then((stop) => {
          unlisten = stop;
          if (settled) {
            stop();
            return;
          }

          // The job may have finished between the invoke and this listener
          // attaching, in which case no further event is ever coming and a
          // watcher would wait for one indefinitely — which is what the UI
          // showed: "starting", and then nothing, for a job already done.
          //
          // The server closes the same race for the HTTP transport by
          // replaying a terminal event to a late subscriber; this is that,
          // for Tauri. Asking once is enough: a job that is not terminal now
          // will emit its own events from here on.
          void this.job(id)
            .then((job) => {
              if (settled || !job) return;
              if (!TERMINAL.includes(job.status)) return;
              onEvent({
                id: job.id,
                kind: job.kind,
                state: job.status,
                progress: job.progress,
                total: job.total,
                message: job.summary ?? job.error ?? 'done',
                terminal: true,
              });
              finish();
            })
            // Not fatal: the listener is attached, so a job still running will
            // still report. Only the already-finished case is lost, and that
            // is better than rejecting a watch that may yet succeed.
            .catch(() => undefined);
        })
        .catch((e) => finish(e instanceof Error ? e : new Error(String(e))));
    });
  }

  cancelJob(id: string): Promise<boolean> {
    return invoke<boolean>('cancel_job', { id });
  }

  // ---------------------------------------------------------------------------
  // Editing & LUT commands (ED-6)
  // ---------------------------------------------------------------------------

  loadRecipe(path: string): Promise<AdjustmentRecipe | null> {
    return invoke<AdjustmentRecipe | null>('load_recipe', { path });
  }

  saveRecipe(path: string, recipe: AdjustmentRecipe): Promise<string> {
    return invoke<string>('save_recipe', { path, recipe });
  }

  exportEditedImage(
    path: string,
    recipe: AdjustmentRecipe,
    outDir: string,
    options?: ExportOptions,
  ): Promise<ExportResult> {
    return invoke<ExportResult>('export_edited_image', {
      path,
      recipe,
      outDir,
      options,
    });
  }

  planBulkLut(
    inputs: string[],
    lut: string,
    intensity: number,
    outDir: string,
    recursive?: boolean,
  ): Promise<BulkLutPlanSummary> {
    return invoke<BulkLutPlanSummary>('plan_bulk_lut', {
      inputs,
      lut,
      intensity,
      outDir,
      recursive: recursive ?? false,
    });
  }

  applyBulkLut(
    inputs: string[],
    lut: string,
    intensity: number,
    outDir: string,
    reviewedLutSha256: string,
    recursive?: boolean,
  ): Promise<string> {
    return invoke<string>('apply_bulk_lut', {
      inputs,
      lut,
      intensity,
      outDir,
      reviewedLutSha256,
      recursive: recursive ?? false,
    });
  }

  /** Dry run of a full recipe over photographs (ED-18); nothing is written. */
  planBulkEdit(
    inputs: string[],
    source: RecipeSource,
    outDir: string,
    recursive?: boolean,
  ): Promise<BulkEditPlanSummary> {
    return invoke<BulkEditPlanSummary>('plan_bulk_edit', {
      inputs,
      source,
      outDir,
      recursive: recursive ?? false,
    });
  }

  /** Runs a reviewed batch; refused unless the recipe still hashes to what was reviewed. */
  applyBulkEdit(
    inputs: string[],
    source: RecipeSource,
    outDir: string,
    reviewedRecipeSha256: string,
    recursive?: boolean,
  ): Promise<string> {
    return invoke<string>('apply_bulk_edit', {
      inputs,
      source,
      outDir,
      reviewedRecipeSha256,
      recursive: recursive ?? false,
    });
  }

  openPreview(path: string): Promise<OpenPreviewResult> {
    return invoke<OpenPreviewResult>('open_preview', { path });
  }

  async renderPreview(
    sessionId: string,
    recipe: AdjustmentRecipe,
    stage: PreviewStage = 'Drag',
  ): Promise<PreviewFrame> {
    const res = await invoke<ArrayBuffer | Uint8Array>('render_preview', {
      sessionId,
      recipe,
      stage,
    });
    return decodePreviewFrame(res);
  }

  /** The automatic-mask models and whether each is downloaded (ED-23). */
  autoMaskModels(): Promise<MaskModel[]> {
    return invoke<MaskModel[]>('auto_mask_models');
  }

  /** Downloads one model as a job; it is kept only if it verifies. */
  downloadMaskModel(target: AutoTarget): Promise<string> {
    return invoke<string>('download_mask_model', { target });
  }

  /** Makes the automatic mask of the open photograph: a few seconds for a subject. */
  makeAutoMask(sessionId: string, target: AutoTarget): Promise<StoredRaster> {
    return invoke<StoredRaster>('make_auto_mask', { sessionId, target });
  }

  /** Where one mask acts on the frame `renderPreview` returns for the same arguments. */
  async renderMaskCoverage(
    sessionId: string,
    recipe: AdjustmentRecipe,
    maskId: string,
    stage: PreviewStage = 'Drag',
  ): Promise<MaskCoverage> {
    const res = await invoke<ArrayBuffer | Uint8Array>('render_mask_coverage', {
      sessionId,
      recipe,
      maskId,
      stage,
    });
    const bytes = res instanceof Uint8Array ? res : new Uint8Array(res);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const width = view.getUint32(0, false);
    const height = view.getUint32(4, false);
    if (bytes.byteLength !== 8 + width * height) {
      throw new Error(
        `Mask coverage is ${bytes.byteLength} bytes; a ${width}x${height} frame needs ${8 + width * height}`,
      );
    }
    return { width, height, coverage: bytes.subarray(8) };
  }

  closePreview(sessionId: string): Promise<void> {
    return invoke<void>('close_preview', { sessionId });
  }

  /** Samples a 5×5 box on unadjusted linear settle proxy to compute WB shifts (RP-4). */
  whiteBalanceAt(sessionId: string, x: number, y: number): Promise<WhiteBalancePick> {
    return invoke<WhiteBalancePick>('white_balance_at', { sessionId, x, y });
  }

  /** Evaluates exposure, whites, and blacks via bisection on the drag proxy (RP-4). */
  autoTone(sessionId: string): Promise<AutoTone> {
    return invoke<AutoTone>('auto_tone', { sessionId });
  }

  listLuts(): Promise<LutLibraryList> {
    return invoke<LutLibraryList>('list_luts');
  }

  importLut(path: string): Promise<LutEntry> {
    return invoke<LutEntry>('import_lut', { path });
  }

  /** Presets live in the application's own folder, so names, not paths, cross over (ED-17). */
  listPresets(): Promise<PresetList> {
    return invoke<PresetList>('list_presets');
  }

  loadPreset(name: string): Promise<AdjustmentRecipe> {
    return invoke<AdjustmentRecipe>('load_preset', { name });
  }

  savePreset(name: string, recipe: AdjustmentRecipe, overwrite = false): Promise<void> {
    return invoke<void>('save_preset', { name, recipe, overwrite });
  }

  renamePreset(from: string, to: string): Promise<void> {
    return invoke<void>('rename_preset', { from, to });
  }

  deletePreset(name: string): Promise<void> {
    return invoke<void>('delete_preset', { name });
  }

  listBuiltinLooks(): Promise<BuiltinLookInfo[]> {
    return invoke<BuiltinLookInfo[]>('list_builtin_looks');
  }

  listDevelopFolder(folder: string): Promise<string[]> {
    return invoke<string[]>('list_develop_folder', { folder });
  }

  async developThumbnail(folder: string, name: string): Promise<Uint8Array> {
    const res = await invoke<ArrayBuffer | Uint8Array>('develop_thumbnail', { folder, name });
    return res instanceof Uint8Array ? res : new Uint8Array(res);
  }

  async previewDevelop(
    folder: string,
    name: string,
    look: string | null,
    exposureEv: number,
  ): Promise<Uint8Array> {
    const res = await invoke<ArrayBuffer | Uint8Array>('preview_develop', {
      folder,
      name,
      look,
      exposureEv,
    });
    return res instanceof Uint8Array ? res : new Uint8Array(res);
  }

  planDevelop(
    folder: string,
    items: DevelopItem[],
    format: OutputFormat,
  ): Promise<DevelopPlan> {
    return invoke<DevelopPlan>('plan_develop', { folder, items, format });
  }

  applyDevelop(
    folder: string,
    items: DevelopItem[],
    format: OutputFormat,
  ): Promise<string> {
    return invoke<string>('apply_develop', { folder, items, format });
  }
}

/** Counts of the frame on screen, made by `core` from the pixels it sent (ED-15). */
export interface PreviewHistogram {
  red: Uint32Array;
  green: Uint32Array;
  blue: Uint32Array;
  luminance: Uint32Array;
  pixels: number;
  highlightClipped: number;
  shadowClipped: number;
}

/** One mask's coverage, 0–255 per pixel, over a preview frame (ED-20). */
export interface MaskCoverage {
  width: number;
  height: number;
  coverage: Uint8Array;
}

/** A rendered preview frame, as `render_preview` delivers it. */
export interface PreviewFrame {
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
  /** The EXIF orientation still to be applied for display; 1 once geometry is baked in. */
  orientation: number;
  histogram: PreviewHistogram;
  /**
   * This frame's normalised coordinates → the stored frame's, as `[a, b, c, d, e, f]`
   * with `u_s = a·u + b·v + c`, `v_s = d·u + e·v + f`. Mask handles are placed through
   * it, so the view never re-derives the geometry (ED-20).
   */
  toStored: [number, number, number, number, number, number];
}

/** Six words, the six `toStored` coefficients, then four channels of 256 bins:
 * `PREVIEW_FRAME_HEADER_LEN` in `core`. */
const PREVIEW_HEADER_BYTES = (6 + 6 + 4 * 256) * 4;

/**
 * Reads the frame `core::media::edit::encode_preview_frame` lays out.
 *
 * A length that does not match is refused rather than guessed at: painting a
 * misread header shows a plausible picture that is wrong, which nobody would
 * think to question.
 */
export function decodePreviewFrame(res: ArrayBuffer | Uint8Array): PreviewFrame {
  const bytes = res instanceof Uint8Array ? res : new Uint8Array(res);
  if (bytes.byteLength < PREVIEW_HEADER_BYTES) {
    throw new Error(
      `Preview frame is ${bytes.byteLength} bytes, shorter than its ${PREVIEW_HEADER_BYTES}-byte header`,
    );
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const word = (i: number) => view.getUint32(i * 4, false);

  const width = word(0);
  const height = word(1);
  const pixelBytes = width * height * 4;
  if (bytes.byteLength !== PREVIEW_HEADER_BYTES + pixelBytes) {
    throw new Error(
      `Preview frame is ${bytes.byteLength} bytes; a ${width}x${height} frame needs ${
        PREVIEW_HEADER_BYTES + pixelBytes
      }`,
    );
  }

  const channel = (c: number) => {
    const bins = new Uint32Array(256);
    for (let b = 0; b < 256; b++) bins[b] = word(12 + c * 256 + b);
    return bins;
  };

  return {
    width,
    height,
    orientation: word(2),
    pixels: new Uint8ClampedArray(
      bytes.buffer,
      bytes.byteOffset + PREVIEW_HEADER_BYTES,
      pixelBytes,
    ),
    toStored: [0, 1, 2, 3, 4, 5].map((i) => view.getFloat32((6 + i) * 4, false)) as PreviewFrame['toStored'],
    histogram: {
      pixels: word(3),
      highlightClipped: word(4),
      shadowClipped: word(5),
      red: channel(0),
      green: channel(1),
      blue: channel(2),
      luminance: channel(3),
    },
  };
}

/** The preset library, and any preset file that could not be read (ED-17). */
export interface PresetList {
  presets: { name: string }[];
  errors: { name: string; error: string }[];
}

/** 3D LUT reference in a recipe. */
export interface LutRef {
  name: string;
  sha256: string;
}

/** A 2D control point for tone curves (ED-10). Coordinates are normalised in [0.0, 1.0]. */
export interface CurvePoint {
  x: number;
  y: number;
}

/** Evaluated tone curves for Luma and individual RGB color channels (ED-10). */
export interface ToneCurves {
  luma?: CurvePoint[];
  red?: CurvePoint[];
  green?: CurvePoint[];
  blue?: CurvePoint[];
}

/** Adjustments for an individual color band (ED-11). */
export interface HslBand {
  hue: number;
  saturation: number;
  luminance: number;
}

/** 8-band selective color adjustments in OkLCh (ED-11). */
export interface HslAdjustments {
  red: HslBand;
  orange: HslBand;
  yellow: HslBand;
  green: HslBand;
  aqua: HslBand;
  blue: HslBand;
  purple: HslBand;
  magenta: HslBand;
}

/** Tonal color wheel parameters for 3-way color grading (ED-12). */
export interface ColorWheel {
  hue: number;
  saturation: number;
  luminance: number;
}

/** 3-way color grading configuration (ED-12). */
export interface ColorGrading {
  shadows: ColorWheel;
  midtones: ColorWheel;
  highlights: ColorWheel;
  global: ColorWheel;
  blending: number;
  balance: number;
}

/** Normalised crop rectangle in the upright displayed frame [0.0, 1.0] (ED-13). */
export interface NormalizedCrop {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Geometric transformation configuration (ED-13). */
export interface Geometry {
  crop?: NormalizedCrop | null;
  rotate: number;
  straighten: number;
  flip_h: boolean;
  flip_v: boolean;
  aspect?: string | null;
}

/** Scale-independent vignette configuration (ED-14). */
export interface Vignette {
  amount: number;
  midpoint: number;
  roundness: number;
  feather: number;
}

/** Scale-independent film grain configuration (ED-14). */
export interface FilmGrain {
  amount: number;
  size: number;
  roughness: number;
}

/** Photographic looks (Glow/Bloom, Halation) and optional filmic tone mapper (ED-16). */
export interface LookEffects {
  glow_amount: number;
  glow_threshold: number;
  glow_radius: number;
  halation_amount: number;
  halation_threshold: number;
  halation_radius: number;
  tone_mapper?: string | null;
}

/** Eyedropper white balance pick result (RP-4). */
export interface WhiteBalancePick {
  temperature: number;
  tint: number;
  clamped: boolean;
}

/** Auto tone determination result (RP-4). */
export interface AutoTone {
  exposure: number;
  whites: number;
  blacks: number;
}

/** 8-band black & white mixer in OkLCh (RP-4). */
export interface BlackAndWhite {
  red: number;
  orange: number;
  yellow: number;
  green: number;
  aqua: number;
  blue: number;
  purple: number;
  magenta: number;
}

/** Detail processing adjustments: sharpening, noise reduction, hot pixels (RP-7), presence (RP-8). */
export interface Detail {
  sharpen_amount: number;
  sharpen_radius: number;
  sharpen_masking: number;
  noise_luminance: number;
  noise_colour: number;
  hot_pixels: boolean;
  clarity?: number;
  texture?: number;
  dehaze?: number;
}

/** Non-destructive adjustment recipe (ED-1, ED-6, ED-9, ED-10, ED-11, ED-12, ED-13, ED-14, ED-16, RP-4). */
export interface AdjustmentRecipe {
  version?: number;
  source_sha256?: string;
  exposure?: number;
  temperature?: number;
  tint?: number;
  highlights?: number;
  shadows?: number;
  contrast?: number;
  saturation?: number;
  vibrance?: number;
  lut?: LutRef | null;
  lut_intensity?: number;
  whites?: number;
  blacks?: number;
  brightness?: number;
  hue?: number;
  curves?: ToneCurves | null;
  hsl?: HslAdjustments | null;
  /** Optional 8-band black & white mixer evaluated in OkLCh (RP-4). */
  black_and_white?: BlackAndWhite | null;
  grading?: ColorGrading | null;
  geometry?: Geometry | null;
  vignette?: Vignette | null;
  grain?: FilmGrain | null;
  looks?: LookEffects | null;
  /** Sharpening, noise reduction and hot pixel removal (RP-7). */
  detail?: Detail | null;
  /** Masks with their own adjustments, in stored-frame coordinates (ED-19). */
  masks?: Mask[];
  /** Named snapshots of adjustment recipes for this photograph (RP-5). */
  snapshots?: Snapshot[];
  /** The name of this version, or null/undefined if Main (RP-6). */
  version_name?: string | null;
  /** Other versions of this photograph (RP-6). */
  versions?: Version[];
}

/** A named virtual copy of an adjustment recipe (RP-6). */
export interface Version {
  name: string;
  recipe: AdjustmentRecipe;
}

/** A named snapshot of an adjustment recipe (RP-5). */
export interface Snapshot {
  name: string;
  recipe: AdjustmentRecipe;
}

/** The adjustments a mask applies where it covers, in the global sliders' units (ED-19). */
export interface LocalAdjustments {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  temperature: number;
  tint: number;
  saturation: number;
  vibrance: number;
}

/**
 * A mask's shape. Points are normalised to the **stored** frame (the file's own pixel grid,
 * before orientation and geometry); radii are fractions of its long edge.
 */
export type MaskKind =
  | { type: 'linear'; start: [number, number]; end: [number, number] }
  | {
      type: 'radial';
      center: [number, number];
      radius_x: number;
      radius_y: number;
      angle: number;
      feather: number;
    }
  | { type: 'brush' }
  | { type: 'auto'; target: AutoTarget; mask: StoredRaster };

/** What an automatic mask finds (ED-23). */
export type AutoTarget = 'subject' | 'sky';

/** An automatic mask's pixels as the sidecar keeps them: 8-bit PNG, base64, stored frame. */
export interface StoredRaster {
  width: number;
  height: number;
  png: string;
  model_sha256: string;
}

/** One automatic-mask model, and whether it is on disk yet. */
export interface MaskModel {
  target: AutoTarget;
  name: string;
  present: boolean;
  bytes: number;
}

/** A brush stroke in stored-frame coordinates; `radius` is a fraction of the long edge (ED-21). */
export interface BrushStroke {
  points: [number, number][];
  radius: number;
  feather: number;
  flow: number;
  erase: boolean;
}

export interface Mask {
  id: string;
  name: string;
  kind: MaskKind;
  invert: boolean;
  opacity: number;
  enabled: boolean;
  adjustments: LocalAdjustments;
  /** Painted after the shape and its invert, in order (ED-21). */
  strokes?: BrushStroke[];
}

/** Stage for preview rendering. */
export type PreviewStage = 'Drag' | 'Settle' | 'Full';

/** Open preview response. */
export interface OpenPreviewResult {
  session_id: string;
  source_sha256?: string;
  drag: [number, number];
  settle: [number, number];
  full_width: number;
  full_height: number;
  settle_width: number;
  settle_height: number;
  orientation: number;
  read_only: boolean;
}

/** Format selection and format-specific parameters for export (RP-9). */
export type ExportFormat =
  | 'AsSource'
  | { Jpeg: { quality: number } }
  | { Tiff: { bits: number } }
  | { Png: { bits: number } }
  | 'Webp'
  | { Avif: { quality: number } };

/** Spatial resize mode (RP-9). */
export type ExportResize =
  | 'None'
  | { LongEdge: number }
  | { Megapixels: number };

/** Output sharpening tailored to display/print medium (RP-9). */
export type OutputSharpening = 'Off' | 'Screen' | 'Print';

/** Metadata preservation policy (RP-9). */
export type MetadataPolicy = 'All' | 'NoLocation' | 'None';

/** Full export configuration (RP-9). */
export interface ExportOptions {
  format?: ExportFormat;
  resize?: ExportResize;
  output_sharpening?: OutputSharpening;
  name_template?: string | null;
  metadata?: MetadataPolicy;
}

/** Export result. */
export interface ExportResult {
  path: string;
  metadata_skipped?: { file: string; reason: string } | null;
}

/** Where a batch's recipe comes from (ED-18). */
export type RecipeSource =
  | { kind: 'Preset'; name: string }
  | { kind: 'Recipe'; recipe: AdjustmentRecipe };

export interface BulkEditPlanSummary {
  actions_count: number;
  skipped: Array<{ file: string; reason: string }>;
  /** The lock: a run is refused unless the recipe still hashes to this. */
  recipe_sha256: string;
  sample_frames: string[];
}

/** Plan summary for Bulk LUT tool (ED-6). */
export interface BulkLutPlanSummary {
  actions_count: number;
  skipped: Array<{ file: string; reason: string }>;
  lut_sha256: string;
  sample_frames: string[];
}

/** Metadata for an imported LUT entry. */
export interface LutEntry {
  name: string;
  sha256: string;
  format: string;
}

/** An unparseable file in the LUT library. */
export interface LutError {
  name: string;
  error: string;
}

/** Contents of the managed LUT library. */
export interface LutLibraryList {
  luts: LutEntry[];
  errors: LutError[];
}

/** Output format for develop export (RP-1). Matches Rust serde tag="kind", rename_all="snake_case". */
export type OutputFormat =
  | { kind: 'jpeg'; quality: number }
  | { kind: 'tiff' };

/** Information about a built-in look (RP-1). */
export interface BuiltinLookInfo {
  id: string;
  name: string;
  maker: string;
}

/** One item in a develop plan. */
export interface DevelopItem {
  name: string;
  look: string | null;
  exposure_ev: number;
}

/** One planned develop action. */
export interface DevelopAction {
  source: string;
  output: string;
  look: string;
  exposure_ev: number;
}

/** One item skipped during develop planning. */
export interface DevelopSkip {
  file: string;
  reason: string;
}

/** Reviewed development plan summary (RP-1). */
export interface DevelopPlan {
  actions: DevelopAction[];
  skipped: DevelopSkip[];
  format: OutputFormat;
}

export const rollchrome = new RollchromeClient();
export const api = rollchrome;
export const desktop = rollchrome;
