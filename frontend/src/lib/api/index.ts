// Typed, per-resource helpers over the low-level `http` client.
//
// Every helper accepts an optional request-scoped `fetch` so it can be called
// from a SvelteKit `load` (forward the `fetch` SvelteKit provides there).

import { http, ApiError, errorMessage, type Fetch } from './client';
import type {
  AcceptInviteRequest,
  AddTeamMemberRequest,
  AdminUser,
  AuthConfig,
  CreateInviteRequest,
  CreateProjectRequest,
  CreateTeamRequest,
  Dsn,
  Id,
  Invite,
  InvitePreview,
  Issue,
  IssueDetail,
  IssueListQuery,
  LoginRequest,
  Profile,
  Project,
  ProjectMember,
  RegisterRequest,
  SdkSetup,
  ServiceSettings,
  SetupRequest,
  SoikaEvent,
  Team,
  TeamSummary,
  UpdateProfileRequest,
  UpdateProjectRequest,
  UpdateSettingsRequest,
  UpdateTeamRequest,
  User
} from './types';

export { ApiError, errorMessage };
export type { Fetch };
export * from './types';

/** Options passed through to the underlying request (request-scoped fetch). */
interface Ctx {
  fetch?: Fetch;
}

export const auth = {
  /** Public auth config (SSO state, password-login + signup flags). */
  config: (ctx?: Ctx) => http.get<AuthConfig>('/auth/config', { fetch: ctx?.fetch }),
  login: (body: LoginRequest, ctx?: Ctx) =>
    http.post<User>('/auth/login', { body, fetch: ctx?.fetch }),
  logout: (ctx?: Ctx) => http.post<void>('/auth/logout', { fetch: ctx?.fetch }),
  register: (body: RegisterRequest, ctx?: Ctx) =>
    http.post<User>('/auth/register', { body, fetch: ctx?.fetch }),
  /** First-run admin provisioning; only succeeds while uninitialized. */
  setup: (body: SetupRequest, ctx?: Ctx) =>
    http.post<User>('/auth/setup', { body, fetch: ctx?.fetch }),
  /** Current user, derived from `GET /profile`; `null` when unauthenticated. */
  me: async (ctx?: Ctx): Promise<User | null> => {
    try {
      return await http.get<Profile>('/profile', { fetch: ctx?.fetch });
    } catch (err) {
      if (err instanceof ApiError && err.isUnauthorized) return null;
      throw err;
    }
  }
};

export const invites = {
  preview: (token: string, ctx?: Ctx) =>
    http.get<InvitePreview>(`/invite/${encodeURIComponent(token)}`, { fetch: ctx?.fetch }),
  accept: (token: string, body: AcceptInviteRequest, ctx?: Ctx) =>
    http.post<User>(`/invite/${encodeURIComponent(token)}`, { body, fetch: ctx?.fetch })
};

export const profile = {
  get: (ctx?: Ctx) => http.get<Profile>('/profile', { fetch: ctx?.fetch }),
  update: (body: UpdateProfileRequest, ctx?: Ctx) =>
    http.patch<Profile>('/profile', { body, fetch: ctx?.fetch })
};

export const projects = {
  list: (ctx?: Ctx) => http.get<Project[]>('/projects', { fetch: ctx?.fetch }),
  create: (body: CreateProjectRequest, ctx?: Ctx) =>
    http.post<Project>('/projects', { body, fetch: ctx?.fetch }),
  get: (id: Id, ctx?: Ctx) => http.get<Project>(`/projects/${id}`, { fetch: ctx?.fetch }),
  update: (id: Id, body: UpdateProjectRequest, ctx?: Ctx) =>
    http.patch<Project>(`/projects/${id}`, { body, fetch: ctx?.fetch }),
  remove: (id: Id, ctx?: Ctx) => http.delete<void>(`/projects/${id}`, { fetch: ctx?.fetch }),
  issues: (id: Id, query?: IssueListQuery, ctx?: Ctx) =>
    http.get<Issue[]>(`/projects/${id}/issues`, { query, fetch: ctx?.fetch }),
  dsn: (id: Id, ctx?: Ctx) => http.get<Dsn>(`/projects/${id}/dsn`, { fetch: ctx?.fetch }),
  sdkSetup: (id: Id, ctx?: Ctx) =>
    http.get<SdkSetup>(`/projects/${id}/sdk-setup`, { fetch: ctx?.fetch }),
  mute: (id: Id, muted?: boolean, ctx?: Ctx) =>
    http.post<Project>(`/projects/${id}/mute`, { body: { muted }, fetch: ctx?.fetch }),
  regenerateDsn: (id: Id, ctx?: Ctx) =>
    http.post<Dsn>(`/projects/${id}/regenerate-dsn`, { fetch: ctx?.fetch }),
  members: (id: Id, ctx?: Ctx) =>
    http.get<ProjectMember[]>(`/projects/${id}/members`, { fetch: ctx?.fetch }),
  removeMember: (id: Id, userId: Id, ctx?: Ctx) =>
    http.delete<void>(`/projects/${id}/members/${userId}`, { fetch: ctx?.fetch }),
  invites: (id: Id, ctx?: Ctx) =>
    http.get<Invite[]>(`/projects/${id}/invites`, { fetch: ctx?.fetch }),
  createInvite: (id: Id, body: CreateInviteRequest, ctx?: Ctx) =>
    http.post<Invite>(`/projects/${id}/invites`, { body, fetch: ctx?.fetch }),
  revokeInvite: (id: Id, token: string, ctx?: Ctx) =>
    http.delete<void>(`/projects/${id}/invites/${encodeURIComponent(token)}`, {
      fetch: ctx?.fetch
    })
};

export const issues = {
  get: (id: Id, ctx?: Ctx) => http.get<IssueDetail>(`/issues/${id}`, { fetch: ctx?.fetch }),
  events: (id: Id, limit?: number, ctx?: Ctx) =>
    http.get<SoikaEvent[]>(`/issues/${id}/events`, { query: { limit }, fetch: ctx?.fetch }),
  resolve: (id: Id, ctx?: Ctx) => http.post<Issue>(`/issues/${id}/resolve`, { fetch: ctx?.fetch }),
  mute: (id: Id, ctx?: Ctx) => http.post<Issue>(`/issues/${id}/mute`, { fetch: ctx?.fetch }),
  unresolve: (id: Id, ctx?: Ctx) =>
    http.post<Issue>(`/issues/${id}/unresolve`, { fetch: ctx?.fetch }),
  /** Override an issue's fingerprint (merge / split). Returns the surviving issue. */
  setFingerprint: (id: Id, fingerprint: string, ctx?: Ctx) =>
    http.patch<Issue>(`/issues/${id}/fingerprint`, { body: { fingerprint }, fetch: ctx?.fetch })
};

export const events = {
  get: (id: Id, ctx?: Ctx) => http.get<SoikaEvent>(`/events/${id}`, { fetch: ctx?.fetch })
};

export const teams = {
  list: (ctx?: Ctx) => http.get<TeamSummary[]>('/teams', { fetch: ctx?.fetch }),
  create: (body: CreateTeamRequest, ctx?: Ctx) =>
    http.post<Team>('/teams', { body, fetch: ctx?.fetch }),
  get: (id: Id, ctx?: Ctx) => http.get<Team>(`/teams/${id}`, { fetch: ctx?.fetch }),
  update: (id: Id, body: UpdateTeamRequest, ctx?: Ctx) =>
    http.patch<Team>(`/teams/${id}`, { body, fetch: ctx?.fetch }),
  remove: (id: Id, ctx?: Ctx) => http.delete<void>(`/teams/${id}`, { fetch: ctx?.fetch }),
  addMember: (id: Id, body: AddTeamMemberRequest, ctx?: Ctx) =>
    http.post<Team>(`/teams/${id}/members`, { body, fetch: ctx?.fetch }),
  removeMember: (id: Id, userId: Id, ctx?: Ctx) =>
    http.delete<void>(`/teams/${id}/members/${userId}`, { fetch: ctx?.fetch })
};

export const settings = {
  get: (ctx?: Ctx) => http.get<ServiceSettings>('/settings', { fetch: ctx?.fetch }),
  update: (body: UpdateSettingsRequest, ctx?: Ctx) =>
    http.patch<ServiceSettings>('/settings', { body, fetch: ctx?.fetch })
};

export const admin = {
  users: (ctx?: Ctx) => http.get<AdminUser[]>('/admin/users', { fetch: ctx?.fetch })
};

/** Aggregate namespace so callers can `import { api } from '$lib/api'`. */
export const api = {
  auth,
  invites,
  profile,
  projects,
  issues,
  events,
  teams,
  settings,
  admin
};
