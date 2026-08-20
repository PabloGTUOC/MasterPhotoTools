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
  Job,
  JobEvent,
  JobStatus,
  Plan,
  RemediateRequest,
  RemediationPlan,
  RenameAction,
  RenameRequest,
  ScanResult,
  SplitPreview,
  SplitPreviewRequest,
  SplitRequest,
  TiffRequest,
  TransformRequest,
} from '@phototools/shared';

/** Statuses a job never leaves again, mirroring `JobStatus::is_terminal`. */
const TERMINAL: JobStatus[] = ['completed', 'failed', 'interrupted'];

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
  // Ingest — F11 to F14
  // -------------------------------------------------------------------------

  scanCard(path: string): Promise<string> {
    return invoke<string>('scan_card', { path });
  }

  validateCard(path: string): Promise<CardValidation> {
    return invoke<CardValidation>('validate_card', { path });
  }

  remediate(request: RemediateRequest): Promise<RemediationPlan | string> {
    return invoke<RemediationPlan | string>('remediate', { args: request });
  }

  deriveRaw(request: DeriveRequest): Promise<string> {
    return invoke<string>('derive_raw', {
      path: request.path,
      outDir: request.out_dir,
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
}

export const api: ApiClient = new TauriApiClient();
export const desktop = api as TauriApiClient;
