/**
 * The single place a transport is chosen.
 *
 * Views import {@link api} and never construct a client or call `fetch`. The
 * desktop build swaps this one module for a Tauri implementation of the same
 * interface (Phase 7).
 */

import { HttpApiClient, type ApiClient } from '@phototools/shared';
import { tokenProvider } from './auth';

const baseUrl = import.meta.env.VITE_API_BASE_URL ?? '';

const client = new HttpApiClient(baseUrl, tokenProvider);

/** What the shared views see: the interface, never a transport. */
export const api: ApiClient = client;

/**
 * The same client, typed as itself.
 *
 * Publishing and the Google connector are the server's alone — the refresh
 * token lives on exactly one machine (§2.3) — so they are not on `ApiClient`.
 * A view reaching for them through this export is declaring itself a web view,
 * which it is.
 */
export const server = client;
