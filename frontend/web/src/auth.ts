/**
 * Firebase Authentication (specification §5.2).
 *
 * Firebase answers "may this person use PhotoTools?". It does **not** grant
 * access to Google Photos — that is a separate OAuth flow owned by the server
 * (§5.1). Nothing here touches Google Photos.
 */

import { initializeApp, type FirebaseApp } from 'firebase/app';
import {
  GoogleAuthProvider,
  getAuth,
  onAuthStateChanged,
  signInWithPopup,
  signOut,
  type Auth,
  type User,
} from 'firebase/auth';
import { ref, type Ref } from 'vue';
import type { TokenProvider } from '@phototools/shared';

interface FirebaseSettings {
  apiKey: string;
  authDomain: string;
  projectId: string;
  appId: string;
}

/** Read the config from the environment, or report that it is absent. */
function settings(): FirebaseSettings | null {
  const env = import.meta.env;
  const apiKey = env.VITE_FIREBASE_API_KEY;
  const authDomain = env.VITE_FIREBASE_AUTH_DOMAIN;
  const projectId = env.VITE_FIREBASE_PROJECT_ID;
  const appId = env.VITE_FIREBASE_APP_ID;

  if (!apiKey || !authDomain || !projectId || !appId) return null;
  return { apiKey, authDomain, projectId, appId };
}

let app: FirebaseApp | null = null;
let auth: Auth | null = null;

export const user: Ref<User | null> = ref(null);
export const authReady = ref(false);
/** True when the build has no Firebase configuration to sign in against. */
export const isConfigured = ref(false);

export function initAuth(): void {
  const config = settings();
  if (!config) {
    // Not an error: a build without credentials should still run and say why.
    isConfigured.value = false;
    authReady.value = true;
    return;
  }

  app = initializeApp(config);
  auth = getAuth(app);
  isConfigured.value = true;

  onAuthStateChanged(auth, (current) => {
    user.value = current;
    authReady.value = true;
  });
}

export async function signIn(): Promise<void> {
  if (!auth) throw new Error('Firebase is not configured for this build');
  await signInWithPopup(auth, new GoogleAuthProvider());
}

export async function signOutOfPhotoTools(): Promise<void> {
  if (auth) await signOut(auth);
}

/**
 * Hands the API client a current ID token.
 *
 * Tokens expire after an hour (§5.3), so the client asks for a fresh one when
 * the server reports `token_expired` and retries transparently.
 */
export const tokenProvider: TokenProvider = {
  async getToken(forceRefresh = false): Promise<string | null> {
    const current = auth?.currentUser;
    if (!current) return null;
    return current.getIdToken(forceRefresh);
  },
};
