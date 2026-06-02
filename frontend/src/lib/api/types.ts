// Wire types mirroring the Rust JSON API DTOs (MVP §16).
//
// Conventions from the backend:
//   * `Id`        -> UUID string
//   * `Timestamp` -> RFC 3339 / ISO-8601 string (chrono DateTime<Utc>)
//   * `Role`        serializes lowercase: "admin" | "member"
//   * `IssueStatus` serializes lowercase: "unresolved" | "resolved" | "muted"

export type Id = string;
export type Timestamp = string;

export type Role = 'admin' | 'member';
export type IssueStatus = 'unresolved' | 'resolved' | 'muted';
/** Account origin (`UserView.auth_provider`); `oidc` accounts have no local password. */
export type AuthProvider = 'local' | 'oidc';

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

/** Public auth config (`GET /auth/config`); read before rendering the login page. */
export interface AuthConfig {
  oauth_enabled: boolean;
  oauth_provider_name: string;
  password_login_enabled: boolean;
  allow_signup: boolean;
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
  id: Id;
  team_id: Id;
  name: string;
  slug: string;
  dsn_public_key: string;
  dsn: string;
  retention_events: number;
  /** Age-based retention in days (Story 7.1); 0 disables age-based pruning. */
  retention_days: number;
  muted: boolean;
  /** Per-project webhook URL for notifications (Story 6.2); null when unset. */
  webhook_url: string | null;
  created_at: Timestamp;
  updated_at: Timestamp;
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
  id: Id;
  project_id: Id;
  fingerprint: string;
  title: string;
  culprit: string | null;
  level: string | null;
  environment: string | null;
  release: string | null;
  status: IssueStatus;
  first_seen: Timestamp;
  last_seen: Timestamp;
  event_count: number;
}

export interface SoikaEvent {
  id: Id;
  event_id: string;
  issue_id: Id;
  project_id: Id;
  payload: unknown;
  received_at: Timestamp;
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

export interface AdminUser {
  id: Id;
  email: string;
  display_name: string;
  is_admin: boolean;
  notifications_enabled: boolean;
  created_at: Timestamp;
}
