// Extraction of the non-stacktrace context from a raw Sentry event payload.
//
// The backend stores each event payload exactly as received and enriches it at
// ingest time with request-derived data the browser SDK can't send: the client
// IP (`user.ip_address`), and `contexts.browser` / `contexts.os` parsed from the
// User-Agent (see src/ingest/enrich.rs). This module pulls the display-relevant
// slices out of that payload — tags, user, contexts, request, SDK, and release
// metadata — so the issue page can surface "who / where / what environment"
// alongside the stacktrace.

type Json = Record<string, unknown>;

function asObject(value: unknown): Json | undefined {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as Json;
  return undefined;
}

function str(value: unknown): string | undefined {
  if (typeof value === 'string') {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : undefined;
  }
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return undefined;
}

/** A simple key/value row for tabular display. */
export interface KeyValue {
  key: string;
  value: string;
}

/** A named product context (browser / OS / device / runtime). */
export interface NamedContext {
  label: string;
  name?: string;
  version?: string;
}

/** A free-form context interface rendered as a full key/value block. */
export interface ContextDetail {
  label: string;
  rows: KeyValue[];
}

/** The display-relevant context extracted from an event payload. */
export interface EventContext {
  /** Headline chips: browser, OS, device, runtime, IP, environment, release. */
  highlights: KeyValue[];
  user: KeyValue[];
  tags: KeyValue[];
  request?: {
    url?: string;
    method?: string;
    query?: string;
    headers: KeyValue[];
  };
  contexts: NamedContext[];
  /**
   * Contexts that carry no `name`/`version` — SDK-specific interfaces such as
   * `trace` or the `Rust Tracing Fields`/`Location` blocks the Rust SDK attaches
   * to `tracing::error!` events. These hold the actual diagnostic payload (the
   * error string, the source file/line), so they get a full key/value block.
   */
  details: ContextDetail[];
  /** The `error` field of a tracing context, promoted for headline display. */
  error?: string;
  sdk?: string;
  additional: KeyValue[];
}

/** Format a one-line label for a named product context (e.g. "Chrome 120"). */
function productLabel(ctx: NamedContext): string | undefined {
  if (ctx.name && ctx.version) return `${ctx.name} ${ctx.version}`;
  return ctx.name ?? ctx.version;
}

/** Read a `{ type, name, version, ... }` context entry. */
function namedContext(label: string, raw: unknown): NamedContext | undefined {
  const obj = asObject(raw);
  if (!obj) return undefined;
  const name = str(obj.name);
  const version = str(obj.version) ?? str(obj.version_string);
  if (!name && !version) return undefined;
  return { label, name, version };
}

/**
 * Read a context entry as a key/value block: every scalar field, plus the
 * contents of a nested `data` object (the `trace` context nests its request
 * metadata there). `type` is the Sentry discriminator, not information.
 */
function contextDetail(label: string, raw: unknown): ContextDetail | undefined {
  const obj = asObject(raw);
  if (!obj) return undefined;

  const rows: KeyValue[] = [];
  const push = (key: string, value: unknown) => {
    if (key === 'type') return;
    const scalar = str(value);
    if (scalar) {
      rows.push({ key, value: scalar });
      return;
    }
    if (value !== null && value !== undefined && typeof value === 'object') {
      rows.push({ key, value: JSON.stringify(value) });
    }
  };

  for (const [key, value] of Object.entries(obj)) {
    if (key === 'data') {
      const data = asObject(value);
      if (data) {
        for (const [k, v] of Object.entries(data)) push(k, v);
        continue;
      }
    }
    push(key, value);
  }

  return rows.length > 0 ? { label, rows } : undefined;
}

/** Normalize `tags` (object map OR array of `[k, v]` / `{key, value}`) to rows. */
function extractTags(raw: unknown): KeyValue[] {
  const obj = asObject(raw);
  if (obj) {
    return Object.entries(obj)
      .map(([key, value]) => ({ key, value: str(value) ?? '' }))
      .filter((t) => t.value.length > 0);
  }
  if (Array.isArray(raw)) {
    return raw
      .map((entry) => {
        if (Array.isArray(entry) && entry.length === 2) {
          return { key: String(entry[0]), value: str(entry[1]) ?? '' };
        }
        const o = asObject(entry);
        if (o) return { key: str(o.key) ?? '', value: str(o.value) ?? '' };
        return undefined;
      })
      .filter((t): t is KeyValue => !!t && t.key.length > 0 && t.value.length > 0);
  }
  return [];
}

/** User identity rows (id, username, email, ip), in a stable order. */
function extractUser(raw: unknown): KeyValue[] {
  const obj = asObject(raw);
  if (!obj) return [];
  const order: [string, string][] = [
    ['id', 'ID'],
    ['username', 'Username'],
    ['email', 'Email'],
    ['ip_address', 'IP address']
  ];
  const rows = order
    .map(([key, label]) => ({ key: label, value: str(obj[key]) }))
    .filter((r): r is KeyValue => r.value !== undefined);
  return rows;
}

/** A handful of request headers worth surfacing, in a stable order. */
function extractHeaders(raw: unknown): KeyValue[] {
  const obj = asObject(raw);
  if (!obj) return [];
  // Header names are case-insensitive; index case-folded for stable lookup.
  const lower = new Map<string, string>();
  for (const [k, v] of Object.entries(obj)) {
    const value = str(v);
    if (value) lower.set(k.toLowerCase(), value);
  }
  const wanted: [string, string][] = [
    ['user-agent', 'User-Agent'],
    ['referer', 'Referer'],
    ['accept-language', 'Accept-Language']
  ];
  return wanted
    .map(([key, label]) => ({ key: label, value: lower.get(key) }))
    .filter((r): r is KeyValue => r.value !== undefined);
}

function extractRequest(raw: unknown): EventContext['request'] {
  const obj = asObject(raw);
  if (!obj) return undefined;
  const url = str(obj.url);
  const method = str(obj.method);
  const query = str(obj.query_string);
  const headers = extractHeaders(obj.headers);
  if (!url && !method && !query && headers.length === 0) return undefined;
  return { url, method, query, headers };
}

/** Flatten `extra` / additional data into JSON-stringified rows. */
function extractAdditional(raw: unknown): KeyValue[] {
  const obj = asObject(raw);
  if (!obj) return [];
  return Object.entries(obj).map(([key, value]) => ({
    key,
    value: typeof value === 'string' ? value : JSON.stringify(value)
  }));
}

/** Extract the display-relevant context from a raw event payload. */
export function extractContext(payload: unknown): EventContext {
  const obj = asObject(payload) ?? {};
  const contextsObj = asObject(obj.contexts) ?? {};

  const browser = namedContext('Browser', contextsObj.browser);
  const os = namedContext('OS', contextsObj.os);
  const device = namedContext('Device', contextsObj.device);
  const runtime = namedContext('Runtime', contextsObj.runtime);

  // Named contexts, in a sensible order; skip the ones already shown as chips.
  const handled = new Set(['browser', 'os', 'device', 'runtime']);
  const rest = Object.entries(contextsObj).filter(([key]) => !handled.has(key));

  const otherContexts = rest
    .map(([key, raw]) => namedContext(titleCase(key), raw))
    .filter((c): c is NamedContext => !!c);
  const contexts = [browser, os, device, runtime, ...otherContexts].filter(
    (c): c is NamedContext => !!c
  );

  // Anything left has no name/version and would otherwise be dropped — yet this
  // is where SDKs put the diagnostics (error strings, source location, trace).
  const named = new Set(otherContexts.map((c) => c.label));
  const details = rest
    .map(([key, raw]) => contextDetail(titleCase(key), raw))
    .filter((d): d is ContextDetail => !!d && !named.has(d.label));

  const error = rest
    .map(([, raw]) => str(asObject(raw)?.error))
    .find((value): value is string => !!value);

  const user = extractUser(obj.user);
  const ip = user.find((r) => r.key === 'IP address')?.value;

  const highlights: KeyValue[] = [];
  const pushHighlight = (key: string, value: string | undefined) => {
    if (value) highlights.push({ key, value });
  };
  if (browser) pushHighlight('Browser', productLabel(browser));
  if (os) pushHighlight('OS', productLabel(os));
  if (device) pushHighlight('Device', productLabel(device));
  if (runtime) pushHighlight('Runtime', productLabel(runtime));
  pushHighlight('IP', ip);
  pushHighlight('Environment', str(obj.environment));
  pushHighlight('Release', str(obj.release));
  pushHighlight('Server', str(obj.server_name));

  const sdkObj = asObject(obj.sdk);
  const sdkName = str(sdkObj?.name);
  const sdkVersion = str(sdkObj?.version);
  const sdk = sdkName && sdkVersion ? `${sdkName} ${sdkVersion}` : (sdkName ?? sdkVersion);

  return {
    highlights,
    user,
    tags: extractTags(obj.tags),
    request: extractRequest(obj.request),
    contexts,
    details,
    error,
    sdk,
    additional: extractAdditional(obj.extra)
  };
}

/** True when there is no context worth rendering. */
export function isContextEmpty(ctx: EventContext): boolean {
  return (
    ctx.highlights.length === 0 &&
    ctx.user.length === 0 &&
    ctx.tags.length === 0 &&
    !ctx.request &&
    ctx.contexts.length === 0 &&
    ctx.details.length === 0 &&
    !ctx.sdk &&
    ctx.additional.length === 0
  );
}

function titleCase(key: string): string {
  return key
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (c) => c.toUpperCase())
    .trim();
}
