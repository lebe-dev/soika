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

  /**
   * Whether the current user can manage the instance (`Owner | Manager`):
   * create/delete teams, manage any team's membership, invites, pending
   * approvals, and instance roles. Gates the Admin nav item and admin routes.
   */
  get canManageInstance(): boolean {
    const role = this.user?.instance_role;
    return role === 'owner' || role === 'manager';
  }

  /**
   * Whether the current user is the instance `Owner`. Owner-only operations
   * (granting/revoking Owner, destructive instance actions) gate on this.
   */
  get isOwner(): boolean {
    return this.user?.instance_role === 'owner';
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
