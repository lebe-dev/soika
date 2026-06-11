// Wire types mirroring the Rust JSON API DTOs.
//
// Conventions from the backend:
//   * `Id`        -> opaque string id. Project and issue ids are short public
//                    codes (6-char `a-z0-9`); other ids (user, team, event) are
//                    UUID strings. Treat all as opaque.
//   * `Timestamp` -> RFC 3339 / ISO-8601 string (chrono DateTime<Utc>)
//   * `Role`        serializes lowercase: "admin" | "member"
//   * `IssueStatus` serializes lowercase: "unresolved" | "resolved" | "muted"

export type Id = string;
export type Timestamp = string;

export type Role = 'admin' | 'member';
export type IssueStatus = 'unresolved' | 'resolved' | 'muted';
/** Account origin (`UserView.auth_provider`); `oidc` accounts have no local password. */
export type AuthProvider = 'local' | 'oidc';
/** Account activation status; `pending` accounts await admin approval and hold no session. */
export type UserStatus = 'active' | 'pending';

/** Error body shape returned by every failing endpoint: `{ "error": "..." }`. */
export interface ApiErrorBody {
  error: string;
}

// --- Auth & profile (src/auth/handlers.rs, src/api/profile.rs) ---

/** `UserView` from the auth handlers; also the login/register response body. */
export interface User {
  id: Id;
  email: string;
  display_name: string;
  is_admin: boolean;
  notifications_enabled: boolean;
  /** Account origin; `oidc` users sign in via SSO and have no local password. */
  auth_provider: AuthProvider;
}

/**
 * Bootstrap config (`GET /auth/config`), fetched once per page load.
 *
 * The first block is public (read before rendering the login page). `user` and
 * `telemetry` are the session slice: populated for a signed-in caller, `null`
 * for an anonymous one — so the layout can boot the whole app from one request
 * instead of also hitting `/profile` (and fetching the telemetry config
 * separately).
 */
export interface AuthConfig {
  oauth_enabled: boolean;
  oauth_provider_name: string;
  password_login_enabled: boolean;
  allow_signup: boolean;
  /**
   * Whether the instance admin has been provisioned. When `false` the SPA
   * routes the operator to `/setup`; when `true`, `/setup` bounces to `/login`.
   */
  initialized: boolean;
  /** Current user when the session is valid; `null` when unauthenticated. */
  user: User | null;
  /** Frontend telemetry; only present for an authenticated session. */
  telemetry: ClientConfig | null;
  /**
   * Dashboard bootstrap: the signed-in user's projects, each with its
   * unresolved-issue count. `null` when unauthenticated. Lets the dashboard
   * render from this single request instead of fetching `/projects` plus one
   * `/issues` call per project.
   */
  projects: ProjectOverview[] | null;
  /** Dashboard bootstrap: team summaries (create-project picker); `null` when unauthenticated. */
  teams: TeamSummary[] | null;
}

/** A project plus its unresolved-issue count (the `/auth/config` dashboard slice). */
export interface ProjectOverview extends Project {
  unresolved_count: number;
  favorited: boolean;
}

/**
 * Client telemetry config, delivered as the `telemetry` field of the
 * authenticated `GET /auth/config` bootstrap (not a standalone endpoint). The
 * Sentry DSN is only served to a logged-in session, so the SPA initializes
 * error reporting after sign-in. Sentry fields are `null` when disabled.
 */
export interface ClientConfig {
  sentry_dsn: string | null;
  sentry_environment: string | null;
  /** Release identifier, matching the backend (`soika@<version>`). */
  release: string;
}

/** `ProfileView` (GET/PATCH /profile). Identical shape to `User`. */
export type Profile = User;

export interface LoginRequest {
  email: string;
  password: string;
}

export interface RegisterRequest {
  email: string;
  password: string;
  display_name: string;
}

/** `POST /auth/setup` body — first-run provisioning of the built-in admin. */
export interface SetupRequest {
  email: string;
  password: string;
  display_name: string;
  org_name: string;
}

export interface UpdateProfileRequest {
  display_name?: string;
  notifications_enabled?: boolean;
  current_password?: string;
  new_password?: string;
}

// --- Invites: accept flow (src/auth/handlers.rs) ---

/** `InviteView` returned by `GET /invite/{token}` (the accept page). */
export interface InvitePreview {
  token: string;
  /** Project's short public id; the accept flow redirects to `/projects/{id}`. */
  project_id: Id;
  role: Role;
  email: string | null;
  requires_registration: boolean;
}

export interface AcceptInviteRequest {
  email?: string;
  password?: string;
  display_name?: string;
}

// --- Projects (src/api/projects.rs) ---

export interface Project {
  /** Short public id (6-char `a-z0-9`) used in URLs and API paths, not a UUID. */
  id: Id;
  team_id: Id;
  name: string;
  slug: string;
  dsn_public_key: string;
  dsn: string;
  retention_events: number;
  /** Age-based retention in days; 0 disables age-based pruning. */
  retention_days: number;
  muted: boolean;
  /** Per-project webhook URL for notifications; null when unset. */
  webhook_url: string | null;
  created_at: Timestamp;
  updated_at: Timestamp;
}

/**
 * Project detail (`GET /projects/{id}`): the project plus its default
 * (unresolved) issue list, embedded so the project page renders its initial
 * view without a follow-up `/projects/{id}/issues` request.
 */
export interface ProjectDetail extends Project {
  issues: Issue[];
}

export interface CreateProjectRequest {
  name: string;
  team_id: Id;
  slug?: string;
  retention_events?: number;
  retention_days?: number;
  webhook_url?: string;
}

export interface UpdateProjectRequest {
  name?: string;
  retention_events?: number;
  retention_days?: number;
  muted?: boolean;
  webhook_url?: string;
}

export interface Dsn {
  dsn: string;
  public_key: string;
  project_id: Id;
}

export interface SdkSnippet {
  language: string;
  label: string;
  code: string;
}

export interface SdkSetup {
  dsn: string;
  snippets: SdkSnippet[];
}

// --- Project members (src/api/members.rs) ---

export interface ProjectMember {
  user_id: Id;
  email: string;
  display_name: string;
  role: Role;
}

// --- Project invites (src/api/invites.rs) ---

export interface Invite {
  token: string;
  project_id: Id;
  role: Role;
  email: string | null;
  link: string;
  created_at: Timestamp;
  expires_at: Timestamp;
  accepted_at: Timestamp | null;
  email_sent: boolean;
}

export interface CreateInviteRequest {
  role?: Role;
  email?: string;
}

// --- Issues & events (src/api/issues.rs, src/api/events.rs) ---

export interface Issue {
  /** Short public id (6-char `a-z0-9`) used in URLs and API paths, not a UUID. */
  id: Id;
  /** Parent project's short public id (used to build the back-link), not a UUID. */
  project_id: Id;
  fingerprint: string;
  title: string;
  culprit: string | null;
  level: string | null;
  environment: string | null;
  release: string | null;
  status: IssueStatus;
  /** Time-based mute expiry (RFC3339), if muted until a fixed time. */
  muted_until: Timestamp | null;
  /** Event-rate mute threshold, if muted under a rate condition. */
  mute_threshold: number | null;
  /** Rolling window (seconds) paired with `mute_threshold`. */
  mute_window_seconds: number | null;
  first_seen: Timestamp;
  last_seen: Timestamp;
  event_count: number;
}

/** Body for `POST /issues/{id}/mute`. Empty object mutes forever. */
export interface MuteRequest {
  /** Time-based mute: auto-unmutes after this many seconds. */
  duration_seconds?: number;
  /** Event-rate mute: event count that auto-resurfaces the issue. */
  events?: number;
  /** Rolling window (seconds) paired with `events`. */
  window_seconds?: number;
}

export interface SoikaEvent {
  id: Id;
  event_id: string;
  issue_id: Id;
  project_id: Id;
  payload: unknown;
  received_at: Timestamp;
}

/** A single `key=value` tag condition within a tag-mute rule. */
export interface TagMatch {
  key: string;
  value: string;
}

/**
 * A project-level "mute by tags" rule. An event matches when ALL of `tags` are
 * present with the same value (AND); a match suppresses the project's
 * notification for that event (the event is still ingested and counted).
 */
export interface TagMuteRule {
  id: Id;
  name: string | null;
  tags: TagMatch[];
  created_at: Timestamp;
}

export interface CreateTagMuteRuleRequest {
  name?: string;
  tags: TagMatch[];
}

/** `IssueDetail`: an `Issue` flattened with its latest event (GET /issues/{id}). */
export type IssueDetail = Issue & {
  latest_event: SoikaEvent | null;
};

export interface IssueListQuery {
  status?: IssueStatus;
  /** Exact-match severity level (`error`, `warning`, ...); free-form per Sentry. */
  level?: string;
  /** Exact-match environment (`production`, `staging`, ...). */
  environment?: string;
  /** Exact-match release (version/build identifier). */
  release?: string;
  query?: string;
  limit?: number;
  offset?: number;
  /** Result ordering: `last_seen` (recency, default) or `event_count` (frequency). */
  sort?: 'last_seen' | 'event_count';
}

// --- Teams (src/api/teams.rs) ---

export interface TeamSummary {
  id: Id;
  name: string;
  member_count: number;
  project_count: number;
}

export interface TeamMember {
  id: Id;
  email: string;
  display_name: string;
}

export interface TeamProject {
  id: Id;
  name: string;
  slug: string;
}

export interface Team {
  id: Id;
  name: string;
  created_at: Timestamp;
  updated_at: Timestamp;
  members: TeamMember[];
  projects: TeamProject[];
}

export interface CreateTeamRequest {
  name: string;
}

export interface UpdateTeamRequest {
  name: string;
}

export interface AddTeamMemberRequest {
  user_id: Id;
}

// --- Service settings & admin (src/api/settings.rs) ---

export interface SmtpStatus {
  configured: boolean;
  host: string | null;
  port: number | null;
  from: string | null;
}

export interface ServiceSettings {
  allow_signup: boolean;
  org_name: string;
  updated_at: Timestamp;
  smtp: SmtpStatus;
}

export interface UpdateSettingsRequest {
  allow_signup?: boolean;
  org_name?: string;
}

/** `POST /settings/test-email` body. Omit `to` to send to the current admin. */
export interface TestEmailRequest {
  to?: string;
}

/** `POST /settings/test-email` response: the address the test was sent to. */
export interface TestEmailResult {
  sent_to: string;
}

export interface AdminUser {
  id: Id;
  email: string;
  display_name: string;
  is_admin: boolean;
  notifications_enabled: boolean;
  /** Activation status: `active` or `pending` (awaiting admin approval). */
  status: UserStatus;
  created_at: Timestamp;
}

/** `DELETE /admin/users/{id}` response: the id of the removed account. */
export interface DeletedUser {
  id: Id;
}
