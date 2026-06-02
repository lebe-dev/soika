// Client-side auth store (Svelte 5 runes).
//
// The source of truth for "who am I" is the server session cookie; this store
// is a reactive cache of the current `User` so the nav and guards can react
// without re-fetching. The root layout's `load` seeds it (see +layout.ts and
// +layout.svelte), and login/logout flows update it directly.

import type { User } from '$lib/api';

class AuthStore {
  /** Current user, or `null` when unauthenticated. `undefined` = not yet known. */
  user = $state<User | null | undefined>(undefined);

  /** Whether a session is established. */
  get isAuthenticated(): boolean {
    return !!this.user;
  }

  /** Whether the current user is the instance-wide built-in admin. */
  get isAdmin(): boolean {
    return !!this.user?.is_admin;
  }

  /** Replace the cached user (e.g. after login, profile update, or layout load). */
  set(user: User | null) {
    this.user = user;
  }

  /** Clear the cached user (after logout). */
  clear() {
    this.user = null;
  }
}

export const authStore = new AuthStore();
