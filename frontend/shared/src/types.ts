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
  divider_x: number;
  /** Where the divider falls across the width, 0.0 to 1.0. */
  divider_fraction: number;
  cropped: PreviewImage;
  a: PreviewImage;
  b: PreviewImage;
}

export interface SplitPreviewRequest {
  path: string;
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

export interface RemediateRequest {
  path: string;
  failure: string;
  action: string;
  date?: string | null;
  out_dir: string;
  dry_run?: boolean;
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
