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

export interface DatesScanRequest {
  path: string;
  recursive?: boolean;
}

export interface DatesFixRequest {
  paths: string[];
  mode: RepairMode;
  dry_run?: boolean;
}

export interface RenameRequest {
  paths: string[];
  date?: string | null;
  subject?: string | null;
  camera?: string | null;
  film?: string | null;
  order: RenameOrder;
}

export interface ImageToolRequest {
  inputs: string[];
  recursive?: boolean;
  out_dir: string;
}

export interface ContactSheetRequest {
  inputs: string[];
  recursive?: boolean;
  out_path: string;
}

export interface TransformRequest extends ImageToolRequest {
  rotate_degrees?: number | null;
  max_long_edge?: number | null;
  format?: TargetFormat | null;
  quality?: number | null;
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
