/**
 * The API client interface, and its HTTP implementation.
 *
 * Views import {@link ApiClient} and never a transport. Phase 7 adds a Tauri
 * implementation of this same interface for the desktop build, so every view is
 * written once (specification §2.7).
 */

import {
  ApiError,
  type BrowserEntry,
  type ContactSheetRequest,
  type DatesFixRequest,
  type DatesScanRequest,
  type ImageToolRequest,
  type Job,
  type JobEvent,
  type Plan,
  type RenameAction,
  type RenameRequest,
  type TransformRequest,
} from './types';

/** Supplies a Firebase ID token, refreshing it on demand. */
export interface TokenProvider {
  /**
   * @param forceRefresh Ask the identity provider for a new token rather than
   *   returning a cached one. Used after the server reports `token_expired`.
   */
  getToken(forceRefresh?: boolean): Promise<string | null>;
}

/** Everything the front ends can ask the system to do. */
export interface ApiClient {
  health(): Promise<{ status: string; version: string }>;

  // F1 — dates
  scanDates(request: DatesScanRequest): Promise<string>;
  fixDates(request: DatesFixRequest): Promise<string>;

  // F3 — rename
  planRename(request: RenameRequest): Promise<Plan<RenameAction>>;
  applyRename(request: RenameRequest): Promise<string>;

  // F4–F8 — the image tools
  split(request: ImageToolRequest): Promise<string>;
  contactSheet(request: ContactSheetRequest): Promise<string>;
  transform(request: TransformRequest): Promise<string>;
  border(request: ImageToolRequest): Promise<string>;
  tiffToJpeg(request: ImageToolRequest): Promise<string>;

  // F9 — storage
  list(path: string): Promise<BrowserEntry[]>;

  // F17 — jobs
  job(id: string): Promise<Job>;
  /**
   * Follow a job's progress until it ends.
   *
   * Resolves once the terminal event arrives. Abort the supplied signal to stop
   * watching; the job itself keeps running on the server.
   */
  watchJob(
    id: string,
    onEvent: (event: JobEvent) => void,
    signal?: AbortSignal,
  ): Promise<void>;
}

export class HttpApiClient implements ApiClient {
  constructor(
    private readonly baseUrl: string,
    private readonly tokens: TokenProvider,
  ) {}

  // -------------------------------------------------------------------------
  // Transport
  // -------------------------------------------------------------------------

  /**
   * Issue a request, refreshing the token once if the server says it expired.
   *
   * §5.3: an expired token must lead to a transparent refresh and retry, not a
   * login screen. Any other rejection is final and is surfaced to the caller.
   */
  private async send(
    path: string,
    init: RequestInit = {},
    retryOnExpiry = true,
  ): Promise<Response> {
    const token = await this.tokens.getToken();
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...init,
      headers: this.headers(init, token),
    });

    if (response.ok) return response;

    const error = await this.toError(response);

    if (error.isExpiredToken && retryOnExpiry) {
      const refreshed = await this.tokens.getToken(true);
      if (refreshed && refreshed !== token) {
        const retried = await fetch(`${this.baseUrl}${path}`, {
          ...init,
          headers: this.headers(init, refreshed),
        });
        if (retried.ok) return retried;
        throw await this.toError(retried);
      }
    }

    throw error;
  }

  private headers(init: RequestInit, token: string | null): Headers {
    const headers = new Headers(init.headers ?? {});
    if (token) headers.set('Authorization', `Bearer ${token}`);
    if (init.body && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json');
    }
    return headers;
  }

  private async toError(response: Response): Promise<ApiError> {
    let code = 'unknown';
    let message = response.statusText || `Request failed (${response.status})`;
    try {
      const body = await response.json();
      if (typeof body?.code === 'string') code = body.code;
      if (typeof body?.message === 'string') message = body.message;
    } catch {
      // A non-JSON error body is still an error; keep the status text.
    }
    return new ApiError(response.status, code, message);
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const response = await this.send(path, {
      method: 'POST',
      body: JSON.stringify(body),
    });
    return (await response.json()) as T;
  }

  /** Start a job and return its id. */
  private async startJob(path: string, body: unknown): Promise<string> {
    const accepted = await this.post<{ job_id: string }>(path, body);
    return accepted.job_id;
  }

  // -------------------------------------------------------------------------
  // Operations
  // -------------------------------------------------------------------------

  async health(): Promise<{ status: string; version: string }> {
    const response = await fetch(`${this.baseUrl}/api/health`);
    if (!response.ok) throw await this.toError(response);
    return response.json();
  }

  scanDates(request: DatesScanRequest): Promise<string> {
    return this.startJob('/api/tools/dates/scan', request);
  }

  fixDates(request: DatesFixRequest): Promise<string> {
    return this.startJob('/api/tools/dates/fix', request);
  }

  planRename(request: RenameRequest): Promise<Plan<RenameAction>> {
    return this.post('/api/tools/rename/plan', request);
  }

  applyRename(request: RenameRequest): Promise<string> {
    return this.startJob('/api/tools/rename/apply', request);
  }

  split(request: ImageToolRequest): Promise<string> {
    return this.startJob('/api/tools/split', request);
  }

  contactSheet(request: ContactSheetRequest): Promise<string> {
    return this.startJob('/api/tools/contact-sheet', request);
  }

  transform(request: TransformRequest): Promise<string> {
    return this.startJob('/api/tools/transform', request);
  }

  border(request: ImageToolRequest): Promise<string> {
    return this.startJob('/api/tools/border', request);
  }

  tiffToJpeg(request: ImageToolRequest): Promise<string> {
    return this.startJob('/api/tools/tiff-to-jpeg', request);
  }

  async list(path: string): Promise<BrowserEntry[]> {
    const query = new URLSearchParams({ path });
    const response = await this.send(`/api/storage/ls?${query}`);
    return (await response.json()) as BrowserEntry[];
  }

  async job(id: string): Promise<Job> {
    const response = await this.send(`/api/jobs/${encodeURIComponent(id)}`);
    return (await response.json()) as Job;
  }

  /**
   * Read the job's SSE stream.
   *
   * `EventSource` cannot carry an `Authorization` header, so the stream is read
   * from a `fetch` body instead. Every request to the server carries the ID
   * token (§5.2), including this one.
   */
  async watchJob(
    id: string,
    onEvent: (event: JobEvent) => void,
    signal?: AbortSignal,
  ): Promise<void> {
    const response = await this.send(`/api/jobs/${encodeURIComponent(id)}/events`, {
      headers: { Accept: 'text/event-stream' },
      signal,
    });

    const body = response.body;
    if (!body) return;

    const reader = body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = '';

    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += value;

        // Events are separated by a blank line.
        let boundary = buffer.indexOf('\n\n');
        while (boundary !== -1) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);

          const event = parseFrame(frame);
          if (event) {
            onEvent(event);
            if (event.terminal) return;
          }
          boundary = buffer.indexOf('\n\n');
        }
      }
    } finally {
      reader.releaseLock();
    }
  }
}

/** Pull the JSON payload out of one SSE frame. */
function parseFrame(frame: string): JobEvent | null {
  const data = frame
    .split('\n')
    .filter((line) => line.startsWith('data:'))
    .map((line) => line.slice(5).trim())
    .join('\n');

  if (!data) return null;

  try {
    return JSON.parse(data) as JobEvent;
  } catch {
    return null;
  }
}
