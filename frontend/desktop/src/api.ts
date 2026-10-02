/**
 * The Tauri implementation of the shared {@link ApiClient}.
 *
 * This is the **only** file that differs from the web build. Every view imports
 * `api` and is written once (specification §2.7).
 *
 * Specification §8: the desktop calls `core` directly through `invoke` for local
 * work, and reaches the server from the **Rust side** with `reqwest` — never
 * from this webview. Nothing here issues an HTTP request.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type {
  ApiClient,
  BorderRequest,
  BrowserEntry,
  CardScan,
  CardSummary,
  CardValidation,
  ContactSheetRequest,
  DateRepairAction,
  DatesFixRequest,
  DatesScanRequest,
  DeriveRequest,
  FillResult,
  GeoScanRow,
  GeotagPreview,
  GeotagRequest,
  Job,
  JobEvent,
  JobStatus,
  Plan,
  RecordedConflict,
  RemediateRequest,
  RemediationPlan,
  RenameAction,
  RenameRequest,
  ScanResult,
  SplitPreview,
  SplitPreviewRequest,
  SplitRequest,
  TiffRequest,
  ExportTimelineRequest,
  ExportedTimeline,
  PlacePointsRequest,
  PlacedStop,
  TimelineRequest,
  TimelineView,
  TrackImportPreview,
  TrackImportRequest,
  TrackImportResult,
  TrackSummary,
  TransformRequest,
  ValidateRequest,
} from '@phototools/shared';

/** Statuses a job never leaves again, mirroring `JobStatus::is_terminal`. */
const TERMINAL: JobStatus[] = ['completed', 'failed', 'interrupted', 'cancelled'];

/** The Tauri event `core`'s job runner emits through. */
const JOB_EVENT = 'phototools://job';

/** How the server connection is reported to the UI. */
export interface ServerStatus {
  reachable: boolean;
  base_url: string;
  version: string | null;
  detail: string | null;
}

export class TauriApiClient implements ApiClient {
  /**
   * The desktop's health is its own: it runs local tools whether or not the NAS
   * is answering. {@link serverStatus} is what gates server-backed features.
   */
  async health(): Promise<{ status: string; version: string }> {
    const status = await this.serverStatus();
    return {
      status: status.reachable ? 'ok' : 'server-unreachable',
      version: status.version ?? 'local',
    };
  }

  serverStatus(): Promise<ServerStatus> {
    return invoke<ServerStatus>('server_status');
  }

  scanDates(request: DatesScanRequest): Promise<ScanResult[]> {
    // The scan runs locally and writes nothing, so its answer is the table
    // itself rather than a job to follow. This used to discard that answer and
    // return an empty id, which the screen read as "nothing to show".
    return invoke<ScanResult[]>('scan_dates', {
      path: request.path,
      recursive: request.recursive ?? false,
    });
  }

  planDates(request: DatesFixRequest): Promise<Plan<DateRepairAction>> {
    return invoke<Plan<DateRepairAction>>('plan_dates', { args: request });
  }

  fixDates(request: DatesFixRequest): Promise<string> {
    return invoke<string>('fix_dates', { args: request });
  }

  planRename(request: RenameRequest): Promise<Plan<RenameAction>> {
    return invoke<Plan<RenameAction>>('plan_rename', { args: request });
  }

  applyRename(request: RenameRequest): Promise<string> {
    return invoke<string>('apply_rename', { args: request });
  }

  splitPreview(request: SplitPreviewRequest): Promise<SplitPreview> {
    return invoke<SplitPreview>('split_preview', {
      inputs: request.inputs,
      recursive: request.recursive ?? false,
      settings: request.settings ?? null,
    });
  }

  split(request: SplitRequest): Promise<string> {
    return invoke<string>('split', { args: request });
  }

  contactSheet(request: ContactSheetRequest): Promise<string> {
    return invoke<string>('contact_sheet', { args: request });
  }

  transform(request: TransformRequest): Promise<string> {
    return invoke<string>('transform', { args: request });
  }

  border(request: BorderRequest): Promise<string> {
    return invoke<string>('border', { args: request });
  }

  tiffToJpeg(request: TiffRequest): Promise<string> {
    return invoke<string>('tiff_to_jpeg', { args: request });
  }

  list(path: string): Promise<BrowserEntry[]> {
    return invoke<BrowserEntry[]>('list_directory', { path });
  }

  roots(): Promise<string[]> {
    return invoke<string[]>('list_roots');
  }

  // -------------------------------------------------------------------------
  // Geotagging — the track library and the matching tool
  //
  // The desktop keeps its own timeline, because it keeps its own ledger: the
  // tracks on this Mac are the ones fed to this Mac. That is the same split as
  // the roots, and for the same reason — §2.3 puts this machine and the NAS on
  // different sides of the card reader.
  // -------------------------------------------------------------------------

  tracks(): Promise<TrackSummary[]> {
    return invoke<TrackSummary[]>('list_tracks');
  }

  previewTrackImport(path: string): Promise<TrackImportPreview> {
    return invoke<TrackImportPreview>('preview_track_import', { path });
  }

  commitTrackImport(request: TrackImportRequest): Promise<TrackImportResult> {
    return invoke<TrackImportResult>('import_track', { args: request });
  }

  deleteTrack(id: string): Promise<number> {
    return invoke<number>('delete_track', { id });
  }

  timeline(request: TimelineRequest): Promise<TimelineView> {
    return invoke<TimelineView>('timeline', { args: request });
  }

  previewPlacedPoints(stops: PlacedStop[]): Promise<TrackImportPreview> {
    return invoke<TrackImportPreview>('preview_placed_points', { stops });
  }

  placePoints(request: PlacePointsRequest): Promise<TrackImportResult> {
    return invoke<TrackImportResult>('place_points', { args: request });
  }

  exportTimeline(request: ExportTimelineRequest): Promise<ExportedTimeline> {
    return invoke<ExportedTimeline>('export_timeline', { args: request });
  }

  trackConflicts(id: string): Promise<RecordedConflict[]> {
    return invoke<RecordedConflict[]>('track_conflicts', { id });
  }

  fillPublishing(paths: string[]): Promise<FillResult> {
    return invoke<FillResult>('fill_publishing', { args: { paths } });
  }

  scanGeo(request: DatesScanRequest): Promise<GeoScanRow[]> {
    // Synchronous, like the date scan: it writes nothing, and the rows are the
    // answer rather than something to fetch afterwards.
    return invoke<GeoScanRow[]>('scan_geo', {
      path: request.path,
      recursive: request.recursive ?? false,
    });
  }

  planGeotag(request: GeotagRequest): Promise<GeotagPreview> {
    return invoke<GeotagPreview>('plan_geotag', { args: request });
  }

  applyGeotag(request: GeotagRequest): Promise<string> {
    return invoke<string>('apply_geotag', { args: request });
  }

  // -------------------------------------------------------------------------
  // Ingest — F11 to F14
  // -------------------------------------------------------------------------

  /**
   * Copy the frames that passed to a folder, keeping the camera's names.
   *
   * Desktop-only: the card reader is on this machine (§2.3), and this is the
   * road off the card that leaves the photographs somewhere the tools can
   * reach them.
   */
  deliverCard(path: string, destination: string): Promise<string> {
    return invoke<string>('deliver_card', { args: { path, destination } });
  }

  scanCard(path: string): Promise<string> {
    return invoke<string>('scan_card', { path });
  }

  validateCard(request: ValidateRequest): Promise<CardValidation> {
    return invoke<CardValidation>('validate_card', {
      path: request.path,
      thresholds: request.thresholds ?? null,
    });
  }

  remediate(request: RemediateRequest): Promise<RemediationPlan | string> {
    return invoke<RemediationPlan | string>('remediate', { args: request });
  }

  deriveRaw(request: DeriveRequest): Promise<string> {
    return invoke<string>('derive_raw', {
      path: request.path,
      outDir: request.out_dir,
      thresholds: request.thresholds ?? null,
    });
  }

  // -------------------------------------------------------------------------
  // The desktop's alone
  // -------------------------------------------------------------------------
  //
  // Not on {@link ApiClient}, because the server genuinely cannot do these:
  // §2.3 puts the card reader on the Mac, and the handoff is the Mac writing to
  // the NAS share. A view that calls them is a desktop view, and the type
  // system says so at compile time.

  /** A cheap look at a directory offered as a card — entries only (F10). */
  summariseCard(path: string): Promise<CardSummary> {
    return invoke<CardSummary>('summarise_card', { path });
  }

  /** The card's shots, ready to render. Not a job — the scan already ran. */
  readCard(path: string): Promise<CardScan> {
    return invoke<CardScan>('read_card', { path });
  }

  /** Copy the card's candidates into local staging, verified by hash (G5). */
  stageCard(path: string): Promise<string> {
    return invoke<string>('stage_card', { path });
  }

  /** Hand the derivatives to the server (F16). A job, and a long one. */
  handOffCard(
    path: string,
    derivedDir: string,
    stagingDir: string,
  ): Promise<string> {
    return invoke<string>('hand_off_card', {
      path,
      derivedDir,
      stagingDir,
    });
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

  // ---------------------------------------------------------------------------
  // Editing & LUT commands (ED-6) - on TauriApiClient only, never ApiClient
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
  ): Promise<ExportResult> {
    return invoke<ExportResult>('export_edited_image', {
      path,
      recipe,
      outDir,
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

  cancelJob(id: string): Promise<boolean> {
    return invoke<boolean>('cancel_job', { id });
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

/** Non-destructive adjustment recipe (ED-1, ED-6, ED-9, ED-10, ED-11, ED-12, ED-13, ED-14, ED-16). */
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
  grading?: ColorGrading | null;
  geometry?: Geometry | null;
  vignette?: Vignette | null;
  grain?: FilmGrain | null;
  looks?: LookEffects | null;
  /** Masks with their own adjustments, in stored-frame coordinates (ED-19). */
  masks?: Mask[];
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
    };

export interface Mask {
  id: string;
  name: string;
  kind: MaskKind;
  invert: boolean;
  opacity: number;
  enabled: boolean;
  adjustments: LocalAdjustments;
}

/** Stage for preview rendering. */
export type PreviewStage = 'Drag' | 'Settle';

/** Open preview response. */
export interface OpenPreviewResult {
  session_id: string;
  source_sha256?: string;
  drag: [number, number];
  settle: [number, number];
  orientation: number;
  read_only: boolean;
}

/** Export result. */
export interface ExportResult {
  path: string;
  metadata_skipped?: { file: string; reason: string } | null;
}

/** Plan summary for Bulk LUT tool (ED-6). */
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

/** Where the server is, and what to authenticate with (§5.2, §5.3). */
export interface ServerSettings {
  base_url: string;
  /** Kept in the macOS Keychain, never in the settings file. */
  auth_token: string | null;
}

export function getServerSettings(): Promise<ServerSettings> {
  return invoke<ServerSettings>('get_server_settings');
}

export function setServerSettings(settings: ServerSettings): Promise<void> {
  return invoke<void>('set_server_settings', { settings });
}

/** What a timeline sync did (`docs/timeline-sync-plan.md`). */
export interface SyncReport {
  summary: string;
  pulled: number;
  pushed: number;
  deleted_here: number;
  deleted_there: number;
  conflicts: number;
  skipped: string[];
}

/**
 * Sync this Mac's timeline with the server's.
 *
 * **Not on `ApiClient`**, and it never will be: the server cannot sync with
 * itself, and a method one transport has to throw for is worse than one the
 * type system never offered (`frontend/shared/src/ui/README.md`). The Mac
 * drives, because a NAS cannot open a connection to a sleeping laptop.
 *
 * Geopositions only — tracks, their fixes, the decisions about them, and
 * deletions.
 */
export function syncTimeline(): Promise<SyncReport> {
  return invoke<SyncReport>('sync_timeline');
}

export const api: ApiClient = new TauriApiClient();
export const desktop = api as TauriApiClient;
