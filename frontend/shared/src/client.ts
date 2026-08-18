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
  type CardValidation,
  type ContactSheetRequest,
  type DatesFixRequest,
  type DatesScanRequest,
  type DeriveRequest,
  type ImageToolRequest,
  type Job,
  type JobEvent,
  type Plan,
  type ConnectorStatus,
  type PublishPlan,
  type RemediateRequest,
  type RemediationPlan,
  type SessionShots,
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

  // F11, F12, F13 — ingest.
  //
  // Only the operations **both** transports genuinely perform. Reading a card's
  // shot rows and handing a card to the server are the desktop's alone, because
  // §2.3 puts the card reader on the Mac; publishing is the server's alone,
  // because that is where the refresh token lives. Those live on the concrete
  // clients, so a view that needs one has to say which build it belongs to
  // rather than discovering at runtime that its transport cannot oblige.

  /** Scan and record a card. A job. */
  scanCard(path: string): Promise<string>;
  /** Validate a card against F12's three rules. Reads no pixels, so no job. */
  validateCard(path: string): Promise<CardValidation>;
  /** Apply one F13 action to every shot sharing one failure. */
  remediate(request: RemediateRequest): Promise<RemediationPlan | string>;
  /** Derive JPEGs for a card's RAW-only shots (F14). A job. */
  deriveRaw(request: DeriveRequest): Promise<string>;

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

  /**
   * Read a JSON body, or say plainly that it was not one.
   *
   * A misconfigured `VITE_API_BASE_URL` — or a dev proxy that is not running —
   * answers `200 text/html`, and `response.json()` then reports
   * `Unexpected token '<'`. That is a true statement about a string and a
   * useless one about the system, so it is turned into a sentence naming the
   * likely cause.
   */
  private async readJson<T>(response: Response, what: string): Promise<T> {
    const text = await response.text();
    try {
      return JSON.parse(text) as T;
    } catch {
      throw new ApiError(
        response.status,
        'not_json',
        `Asked for ${what} and got ${response.headers.get('content-type') ?? 'an unknown format'} ` +
          'instead of JSON. The address configured for the server is probably ' +
          'not the server.',
      );
    }
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const response = await this.send(path, {
      method: 'POST',
      body: JSON.stringify(body),
    });
    return this.readJson<T>(response, 'a result');
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
    return this.readJson(response, 'the server version');
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

  scanCard(path: string): Promise<string> {
    return this.startJob('/api/ingest/scan', { path });
  }

  validateCard(path: string): Promise<CardValidation> {
    return this.post('/api/ingest/validate', { path });
  }

  /**
   * A dry run answers with the plan; a real run answers with a job id.
   *
   * The server distinguishes them by status code, and so does this: the two
   * shapes are genuinely different answers to genuinely different questions.
   */
  async remediate(request: RemediateRequest): Promise<RemediationPlan | string> {
    const response = await this.send('/api/ingest/remediate', {
      method: 'POST',
      body: JSON.stringify(request),
    });
    const body = await this.readJson<RemediationPlan & { job_id?: string }>(
      response,
      'a remediation plan',
    );
    return request.dry_run ? (body as RemediationPlan) : (body.job_id as string);
  }

  deriveRaw(request: DeriveRequest): Promise<string> {
    return this.startJob('/api/ingest/derive', request);
  }

  async list(path: string): Promise<BrowserEntry[]> {
    const query = new URLSearchParams({ path });
    const response = await this.send(`/api/storage/ls?${query}`);
    return this.readJson<BrowserEntry[]>(response, 'a directory listing');
  }

  // -------------------------------------------------------------------------
  // The server's alone — F15 and F16
  // -------------------------------------------------------------------------
  //
  // Not on {@link ApiClient}, and deliberately. The Google refresh token lives
  // on exactly one machine (§2.3), so these are things only a build talking to
  // the server can do. A view that calls them is a web view, and the type
  // system says so at compile time rather than at the moment somebody presses
  // the button.

  /** A session's shots, with their arrival status. */
  async sessionShots(sessionId: string): Promise<SessionShots> {
    const response = await this.send(
      `/api/ingest/sessions/${encodeURIComponent(sessionId)}/shots`,
    );
    return this.readJson<SessionShots>(response, "a session's shots");
  }

  /**
   * Work out what publishing would do, and record that somebody looked.
   *
   * §9.2 rule 3 and §6.1: the Google Photos API cannot delete, so a mistaken
   * bulk publish is cleaned up by hand. The server refuses to publish a session
   * that has had no dry run.
   */
  publishDryRun(sessionId: string): Promise<PublishPlan> {
    return this.post(
      `/api/ingest/sessions/${encodeURIComponent(sessionId)}/publish`,
      { dry_run: true },
    );
  }

  /** Publish for real. A job. */
  publish(sessionId: string): Promise<string> {
    return this.startJob(
      `/api/ingest/sessions/${encodeURIComponent(sessionId)}/publish`,
      { dry_run: false },
    );
  }

  async googleStatus(): Promise<ConnectorStatus> {
    const response = await this.send('/api/connectors/google/status');
    return this.readJson<ConnectorStatus>(response, 'the Google Photos connection');
  }

  /** Begin the consent flow; the answer is where to send the person (§6.2). */
  async googleConnect(): Promise<string> {
    const body = await this.post<{ url: string }>(
      '/api/connectors/google/connect',
      {},
    );
    return body.url;
  }

  async googleDisconnect(): Promise<void> {
    await this.post('/api/connectors/google/disconnect', {});
  }

  async job(id: string): Promise<Job> {
    const response = await this.send(`/api/jobs/${encodeURIComponent(id)}`);
    return this.readJson<Job>(response, 'a job');
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
