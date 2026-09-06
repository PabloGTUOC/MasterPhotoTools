/**
 * Wire types, mirroring the server's DTOs.
 *
 * These are the shapes `phototools-server` serialises; a change on either side
 * without the other is a bug, so each type names the Rust type it mirrors.
 */

/** `phototools_core::jobs::JobStatus` */
export type JobStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'interrupted';

/** `phototools_core::jobs::Job` */
export interface Job {
  id: string;
  kind: string;
  status: JobStatus;
  progress: number;
  total: number;
  started_at: number;
  finished_at: number | null;
  error: string | null;
  /** What the job reported when it ended, persisted so a late watcher sees it. */
  summary: string | null;
}

/** `phototools_server::jobs::JobEvent` */
export interface JobEvent {
  id: string;
  kind: string;
  state: JobStatus;
  progress: number;
  total: number;
  message: string;
  terminal: boolean;
}

/** `phototools_server::api::JobAccepted` */
export interface JobAccepted {
  job_id: string;
}

/** `phototools_core::tools::Skip` */
export interface Skip {
  file: string;
  reason: string;
}

/** `phototools_core::tools::Plan<T>` */
export interface Plan<T> {
  actions: T[];
  skipped: Skip[];
}

/** `phototools_core::tools::f3_rename::BatchRenameAction` */
export interface RenameAction {
  source: string;
  target: string;
}

/** `phototools_core::tools::f9_browser::BrowserEntry` */
export interface BrowserEntry {
  name: string;
  absolute_path: string;
  is_dir: boolean;
  size: number | null;
}

/**
 * `phototools_core::tools::f1_dates::RepairMode`, externally tagged as serde
 * renders Rust enums by default.
 */
export type RepairMode =
  | 'Auto'
  | 'Sidecar'
  /** An absolute date, `YYYY-MM-DDTHH:MM:SS`. */
  | { Manual: string }
  /** A delta in exiftool's syntax, e.g. `+5:0:0 0:0:0`. */
  | { Shift: string };

/** `phototools_core::tools::f3_rename::RenameOrder` */
export type RenameOrder = 'Capture' | 'Numeric';

/** `phototools_core::tools::f6_transform::TargetFormat` */
export type TargetFormat = 'Jpeg' | 'Png' | 'Tiff' | 'WebP';

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/** `phototools_core::tools::f1_dates::DateRepairAction` */
export interface DateRepairAction {
  path: string;
  /** What the file's date will read after applying. */
  new_date: string;
  /** Set for a shift, which is applied as a relative delta. */
  shift: string | null;
}

/** `phototools_core::tools::f1_dates::DateStatus` */
export type DateStatus = 'Ok' | 'Mismatch' | 'MissingMetadata';

/** `phototools_core::tools::f1_dates::FsTimeSource` */
export type FsTimeSource = 'Created' | 'Modified';

/** `phototools_core::tools::f1_dates::ScanResult` */
export interface ScanResult {
  name: string;
  path: string;
  metadata_date: string | null;
  /** Which of F1's seven tags supplied the date. */
  tag: string | null;
  fs_date: string | null;
  fs_date_source: FsTimeSource | null;
  status: DateStatus;
}

export interface DatesScanRequest {
  path: string;
  recursive?: boolean;
}

export interface DatesFixRequest {
  paths: string[];
  mode: RepairMode;
  dry_run?: boolean;
  /** Whether a folder among `paths` contributes its subfolders too. */
  recursive?: boolean;
}

export interface RenameRequest {
  paths: string[];
  date?: string | null;
  subject?: string | null;
  camera?: string | null;
  film?: string | null;
  order: RenameOrder;
}

/** `phototools_core::tools::f4_split::SplitSettings` */
export interface SplitSettings {
  /** Pixel value at or below which a pixel counts as black. */
  threshold_dark: number;
  /** Pixel value at or above which a pixel counts as white. */
  threshold_white: number;
  /** Fraction of extreme pixels needed to call a line "border". */
  border_tol: number;
  /** Maximum proportion removable from one side. */
  max_crop_pct: number;
  /** Proportion of width ignored at each end when seeking the divider. */
  margin: number;
  /** Refinement radius around the darkest column, in pixels. */
  window: number;
  /** Target height ÷ width. */
  ratio: number;
}

export interface PreviewImage {
  /** A `data:image/jpeg;base64,...` URL, ready for an `<img src>`. */
  src: string;
  /** Dimensions of the full-size result, not of the preview bytes. */
  width: number;
  height: number;
}

/** §F4's preview: the border-cropped whole image and both halves. */
export interface SplitPreview {
  /** The file this preview is of — the request may have named a folder. */
  source: string;
  divider_x: number;
  /** Where the divider falls across the width, 0.0 to 1.0. */
  divider_fraction: number;
  cropped: PreviewImage;
  a: PreviewImage;
  b: PreviewImage;
}

export interface SplitPreviewRequest {
  /** The same inputs the apply takes: files, folders, or both. */
  inputs: string[];
  recursive?: boolean;
  settings?: SplitSettings | null;
}

export interface ImageToolRequest {
  inputs: string[];
  recursive?: boolean;
  out_dir: string;
}

/** `phototools_core::tools::f5_contact::SheetStyle` */
export type SheetStyle = 'Grid' | 'Filmstrip';

export interface ContactSheetRequest {
  inputs: string[];
  recursive?: boolean;
  out_path: string;
  style?: SheetStyle;
}

export interface TiffRequest extends ImageToolRequest {
  /**
   * Longest edge of the output. §F8's default is 2048 — a distributable size,
   * which throws away most of a 36 MP camera TIFF.
   */
  max_long_edge?: number | null;
  /** JPEG quality. §F8's default is 90. */
  quality?: number | null;
}

/** `phototools_core::tools::f7_border::CanvasSizing` */
export type CanvasSizing = 'FixedCanvas' | 'ImagePlusMargin';

/** `phototools_core::tools::f7_border::BorderStyle` */
export interface BorderStyle {
  /**
   * `FixedCanvas` is §F7's: one size and shape for every output, the
   * photograph scaled to fit — which discards its resolution.
   * `ImagePlusMargin` keeps the photograph untouched and grows the canvas
   * around it.
   */
  sizing: CanvasSizing;
  /** The canvas the photograph sits on, as [r, g, b]. */
  canvas_colour: [number, number, number];
  /** Canvas width; the height follows from the input's shape. */
  canvas_width: number;
  /** Smallest gap between the photograph and any edge. */
  min_margin: number;
  /** Corner radius as a proportion of the placed image's short side. */
  corner_radius_fraction: number;
}

export interface BorderRequest extends ImageToolRequest {
  /**
   * Trim dark scan edges before placing the image. On by default, as F7 is.
   *
   * The one thing about the border that is a choice: everything else — the
   * 3000px canvas, the 50px margin, the 2% radius — is fixed by §F7.
   */
  trim_dark_edges?: boolean;
  /** How the canvas looks. Absent means §F7's fixed appearance. */
  style?: BorderStyle | null;
}

export interface SplitRequest extends ImageToolRequest {
  settings?: SplitSettings | null;
}

export interface TransformRequest extends ImageToolRequest {
  rotate_degrees?: number | null;
  max_long_edge?: number | null;
  format?: TargetFormat | null;
  quality?: number | null;
}


// ---------------------------------------------------------------------------
// Ingest — F11, F12, F13
// ---------------------------------------------------------------------------

/**
 * One shot on a card.
 *
 * `phototools_desktop::commands::ShotRow`. The desktop is the only producer:
 * §2.3 puts scanning on the Mac, because that is where the card reader is.
 */
export interface ShotRow {
  stem: string;
  candidate_kind: string;
  candidate_path: string;
  bytes: number;
  width: number;
  height: number;
  megapixels: number;
  capture: string | null;
  camera: string | null;
  asset_count: number;
  needs_derivation: boolean;
}

/** `phototools_core::ingest::ScanProblem` */
export interface ScanProblem {
  rel_path: string;
  detail: string;
}

/** `phototools_desktop::commands::CardScanResult` */
export interface CardScan {
  card_id: string;
  label: string | null;
  shots: ShotRow[];
  awaiting_derivation: number;
  problems: ScanProblem[];
}

/** `phototools_desktop::commands::CardSummaryResult` */
export interface CardSummary {
  path: string;
  card_id: string;
  label: string | null;
  shots: number;
  new_shots: number;
  seen_before: boolean;
  looks_like_a_card: boolean;
}

/**
 * `phototools_core::ingest::CheckStatus`.
 *
 * `pending` is a check that cannot be decided yet — a RAW-only shot has no
 * pixels to measure until F14 has derived a JPEG for it.
 */
export type CheckStatus = 'pass' | 'warn' | 'fail' | 'pending';

/**
 * `phototools_server::api::CheckResponse` and
 * `phototools_desktop::commands::CheckRow` — the same JSON from both.
 */
export interface Check {
  rule: string;
  status: CheckStatus;
  failure: string | null;
  detail: string;
}

/** One shot's verdict across every rule. */
export interface ShotVerdict {
  stem: string;
  status: CheckStatus;
  checks: Check[];
}

/**
 * One failure class, with what F13 offers for it and how many shots share it.
 *
 * This is what the bulk action bar renders. A card of four hundred frames
 * becomes a handful of rows rather than four hundred prompts.
 */
export interface FailureGroup {
  failure: string;
  count: number;
  actions: string[];
  default_action: string | null;
}

/** `phototools_core::ingest::ClockOffset` */
export interface ClockOffset {
  median: string;
  spread_days: number;
  median_age_days: number;
  shift: string;
  affected: number;
}

/** `phototools_server::api::ValidationResponse` */
export interface CardValidation {
  shots: ShotVerdict[];
  groups: FailureGroup[];
  clock_offset: ClockOffset | null;
  passing: number;
  failing: number;
}

/**
 * Per-session overrides for F12's two ceilings.
 *
 * Absent means the configured default. `max_megapixels: 0` means no resolution
 * ceiling at all — publishing is limited by file size, and a frame inside the
 * byte cap is worth keeping whole.
 */
export interface ThresholdOverrides {
  max_megapixels?: number | null;
  max_output_bytes?: number | null;
}

export interface RemediateRequest {
  path: string;
  failure: string;
  action: string;
  date?: string | null;
  out_dir: string;
  dry_run?: boolean;
  thresholds?: ThresholdOverrides | null;
}

/** `phototools_server::api::RemediationPreview` */
export interface RemediationPreview {
  stem: string;
  action: string;
  target_dimensions: [number, number] | null;
  new_date: string | null;
  destination: string | null;
}

/** `phototools_server::api::RemediationPlanResponse` */
export interface RemediationPlan {
  actions: RemediationPreview[];
  skipped: Skip[];
  applied: boolean;
}

export interface DeriveRequest {
  path: string;
  out_dir: string;
  thresholds?: ThresholdOverrides | null;
}

export interface ValidateRequest {
  path: string;
  thresholds?: ThresholdOverrides | null;
}

// ---------------------------------------------------------------------------
// Publishing — F15, F16
// ---------------------------------------------------------------------------

/** `phototools_core::publish::PublishItem` */
export interface PublishItem {
  shot_id: string;
  stem: string;
  source_sha256: string;
  file_name: string;
  bytes: number;
}

/** `phototools_core::publish::Skipped` */
export interface Skipped {
  stem: string;
  reason: string;
}

/** `phototools_core::publish::ResumeCounts` */
export interface ResumeCounts {
  pending: number;
  uploaded: number;
  created: number;
  /** Sent to Google with no answer back. Needs a person to look. */
  unconfirmed: number;
}

/**
 * `phototools_core::publish::PublishPlan` — what a publish would do.
 *
 * §9.2 rule 3 makes producing one mandatory before publishing, because the
 * Google Photos API cannot delete what it has created.
 */
export interface PublishPlan {
  session_id: string;
  items: PublishItem[];
  skipped: Skipped[];
  total_bytes: number;
  upload_requests: number;
  batch_create_requests: number;
  resuming: ResumeCounts;
}

/** `phototools_core::publish::ConnectorStatus` */
export interface ConnectorStatus {
  connected: boolean;
  scope: string | null;
  connected_at: number | null;
  needs_reauthorisation: boolean;
  detail: string | null;
}

/** `phototools_server::api::SessionShot` */
export interface SessionShot {
  stem: string;
  file_name: string;
  width: number;
  height: number;
  bytes: number;
  capture: string | null;
  disposition: string;
  arrival: string;
}

/** `phototools_server::api::SessionShotsResponse` */
export interface SessionShots {
  session_id: string;
  state: string;
  shots: SessionShot[];
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/**
 * A rejection from the server.
 *
 * Both `ApiError` and `AuthError` serialise as `{ code, message }`. The code is
 * what a client branches on: §5.3 requires that `token_expired` leads to a
 * refresh and retry rather than dropping the user to a login screen.
 */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }

  /** The token has expired; refreshing and retrying is the right response. */
  get isExpiredToken(): boolean {
    return this.code === 'token_expired';
  }

  /** The account is authenticated but not permitted. Refreshing will not help. */
  get isForbidden(): boolean {
    return this.code === 'not_authorized' || this.code === 'path_not_allowed';
  }
}

// ---------------------------------------------------------------------------
// Geotagging — the track library and the matching tool
//
// Beyond the specification, which mentions neither GPS nor GPX. The reasoning
// is in `docs/geotag-plan.md`; the deviation is recorded in
// `docs/known-gaps.md` rather than by editing the specification (G9).
// ---------------------------------------------------------------------------

/** One fix: where the phone was, and when. */
export interface TrackPoint {
  /** Unix seconds, UTC. GPX times are UTC by definition. */
  at: number;
  lat: number;
  lon: number;
  /** Metres above sea level, or null where the point carried no elevation. */
  ele: number | null;
}

/** A fix in the form EXIF holds it: unsigned, with a separate hemisphere. */
export interface ExifPoint {
  latitude: string;
  /** `N` or `S`. */
  latitude_ref: string;
  longitude: string;
  /** `E` or `W`. */
  longitude_ref: string;
  altitude: string | null;
  /** `0` above sea level, `1` below. */
  altitude_ref: number | null;
  /** `YYYY:MM:DD`, UTC. */
  date_stamp: string;
  /** `HH:MM:SS`, UTC. */
  time_stamp: string;
}

export interface Bounds {
  min_lat: number;
  min_lon: number;
  max_lat: number;
  max_lon: number;
}

/** One imported GPX file. */
export interface TrackSummary {
  /** The sha256 of the file's bytes, which is what makes importing idempotent. */
  id: string;
  name: string;
  source_path: string;
  creator: string | null;
  imported_at: number;
  point_count: number;
  points_added: number;
  points_identical: number;
  points_conflicting: number;
  first_fix: number | null;
  last_fix: number | null;
  bounds: Bounds | null;
}

/** A point in a GPX file that cannot be used, and why. */
export interface RejectedPoint {
  index: number;
  reason: string;
}

/**
 * One instant where a file and the library disagree.
 *
 * Not supposed to happen — every fix comes from one phone — which is why it is
 * put to a person rather than resolved by whichever import ran last.
 */
export interface PointConflict {
  at: number;
  existing: TrackPoint;
  existing_track_id: string;
  existing_track_name: string;
  incoming: TrackPoint;
  /** How far apart the two positions are, over the ground. */
  metres: number;
}

/** What importing a file would do. Nothing is stored until it is committed. */
export interface TrackImportPreview {
  id: string;
  name: string;
  creator: string | null;
  /** When this exact file was first imported, if it has been before. */
  already_imported_at: number | null;
  point_count: number;
  new_points: number;
  identical_points: number;
  conflicts: PointConflict[];
  rejected: RejectedPoint[];
  first_fix: number | null;
  last_fix: number | null;
  /** The first few fixes in the form they will be written. */
  sample: ExifPoint[];
}

export type Take = 'Existing' | 'New';
export type Resolution = 'KeepExisting' | 'TakeNew';

/** One instant decided individually, against the default. */
export interface Decision {
  at: number;
  take: Take;
}

export interface TrackImportRequest {
  path: string;
  resolution: Resolution;
  overrides: Decision[];
}

export interface TrackImportResult {
  id: string;
  name: string;
  added: number;
  identical: number;
  kept_existing: number;
  took_new: number;
  /** Decisions about instants that were no longer in dispute. */
  stale_overrides: number[];
  rejected: RejectedPoint[];
  already_imported: boolean;
}

export type GeoStatus =
  | 'Ok'
  | 'NoLocation'
  | 'NoDate'
  | 'NoDateOrLocation'
  | 'NotSupported';

/** One file in the inventory. */
export interface GeoScanRow {
  name: string;
  path: string;
  capture: string | null;
  tag: string | null;
  /** The UTC offset the camera recorded, in minutes east. */
  utc_offset_minutes: number | null;
  location: ExifPoint | null;
  status: GeoStatus;
}

/**
 * Which recorded fix to use when none was recorded at that exact second.
 *
 * No mode computes a position. A tracker that reports when its owner moves
 * leaves no gaps to fill: between two fixes, its owner was still at the first.
 */
export type MatchMode = 'CarriedForward' | 'Nearest';
export type MatchMethod = 'Exact' | 'Nearest' | 'CarriedForward';

export interface MatchLimits {
  mode: MatchMode;
  /** How old a fix may be and still be used. Zero is no limit. */
  max_edge_seconds: number;
}

export type OffsetSource = 'File' | 'Chosen';

/** One photograph, and the position it would be given. */
export interface GeotagAction {
  path: string;
  name: string;
  /** The camera's local wall clock. */
  capture: string;
  /** The instant that was looked up, in UTC. */
  instant: number;
  offset_minutes: number;
  offset_source: OffsetSource;
  point: TrackPoint;
  method: MatchMethod;
  /** Seconds to the nearest recorded fix. */
  gap_seconds: number;
  exif: ExifPoint;
  /** The position being written over, where there is one. */
  replaces: ExifPoint | null;
}

/** An offset worked out from the photographs themselves. */
export interface OffsetSuggestion {
  minutes: number;
  median_gap_seconds: number;
  /** The range the evidence cannot separate from the winner. */
  plausible_low_minutes: number;
  plausible_high_minutes: number;
  confident: boolean;
  sample: number;
}

export interface GeotagPreview {
  plan: Plan<GeotagAction>;
  matched: number;
  unmatched: number;
  suggestion: OffsetSuggestion | null;
}

export interface GeotagRequest {
  paths: string[];
  recursive: boolean;
  /**
   * The offset for files that carry none, in minutes east.
   *
   * `null` means none was set, and such a file is then skipped rather than
   * quietly read as UTC — which would move every photograph by whatever the
   * offset really was.
   */
  utc_offset_minutes: number | null;
  clock_correction_seconds: number;
  limits: MatchLimits;
  overwrite_existing: boolean;
  write_altitude: boolean;
}
