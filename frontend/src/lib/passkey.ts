// Passkey (WebAuthn) ceremonies, driven from the browser.
//
// The backend issues the challenge and verifies the response; this module only
// bridges it to the platform authenticator via `@simplewebauthn/browser`, which
// handles the base64url ⇄ ArrayBuffer conversions and the browser quirks.
//
// Sign-in is usernameless: the backend sends no `allowCredentials`, so the
// browser shows its own picker of discoverable credentials for this origin.

import {
  browserSupportsWebAuthn,
  startAuthentication,
  startRegistration
} from '@simplewebauthn/browser';
import { auth, passkeys } from '$lib/api';
import type { Passkey, User } from '$lib/api';

/** Whether this browser can do WebAuthn at all (HTTPS/localhost + API present). */
export function passkeysSupported(): boolean {
  return browserSupportsWebAuthn();
}

/**
 * A ceremony the user dismissed (closed the system dialog, or hit the timeout).
 *
 * Not an error worth a toast: the user simply changed their mind, so callers
 * use this to stay silent instead of reporting a failure.
 */
export function isCeremonyCancelled(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  return err.name === 'NotAllowedError' || err.name === 'AbortError';
}

/**
 * Sign in with a discoverable passkey. Resolves with the signed-in user.
 *
 * Throws whatever the API client or the browser raised; use
 * {@link isCeremonyCancelled} to tell a user-cancelled ceremony apart from a
 * genuine failure.
 */
export async function signInWithPasskey(): Promise<User> {
  const options = await auth.passkeyLoginOptions();
  const assertion = await startAuthentication({
    optionsJSON: options.publicKey as Parameters<typeof startAuthentication>[0]['optionsJSON']
  });
  return auth.passkeyLogin(assertion);
}

/**
 * Register a new passkey for the signed-in user and return the stored record.
 *
 * `name` is the label shown in the profile; the backend falls back to a default
 * when it is blank.
 */
export async function registerPasskey(name: string): Promise<Passkey> {
  const options = await passkeys.registerOptions();
  const credential = await startRegistration({
    optionsJSON: options.publicKey as Parameters<typeof startRegistration>[0]['optionsJSON']
  });
  return passkeys.register({ name, credential });
}
