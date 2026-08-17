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
  BrowserEntry,
  ContactSheetRequest,
  DatesFixRequest,
  DatesScanRequest,
  ImageToolRequest,
  Job,
  JobEvent,
  Plan,
  RenameAction,
  RenameRequest,
  TransformRequest,
} from '@phototools/shared';

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

  async scanDates(request: DatesScanRequest): Promise<string> {
    // The desktop scans locally and returns the table directly, so there is no
    // job to watch. The id is empty, which the UI reads as "nothing to follow".
    await invoke('scan_dates', {
      path: request.path,
      recursive: request.recursive ?? false,
    });
    return '';
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

  split(request: ImageToolRequest): Promise<string> {
    return invoke<string>('split', { args: request });
  }

  contactSheet(request: ContactSheetRequest): Promise<string> {
    return invoke<string>('contact_sheet', { args: request });
  }

  transform(request: TransformRequest): Promise<string> {
    return invoke<string>('transform', { args: request });
  }

  border(request: ImageToolRequest): Promise<string> {
    return invoke<string>('border', { args: request });
  }

  tiffToJpeg(request: ImageToolRequest): Promise<string> {
    return invoke<string>('tiff_to_jpeg', { args: request });
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
          // The job may have finished between the invoke and this listener
          // attaching, in which case no further event is coming.
          if (settled) stop();
        })
        .catch((e) => finish(e instanceof Error ? e : new Error(String(e))));
    });
  }
}

export const api: ApiClient = new TauriApiClient();
export const desktop = api as TauriApiClient;
