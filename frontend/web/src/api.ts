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

export const api: ApiClient = new HttpApiClient(baseUrl, tokenProvider);
