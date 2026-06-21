// Per-language SDK setup snippets, generated entirely on the client from a
// project's DSN. This mirrors the backend `sdk_snippets` (src/api/projects.rs)
// so the SDK Setup tab needs no `/sdk-setup` request — the project already
// carries its DSN and public key.
//
// A Sentry DSN is `scheme://public_key@host/project_id`; the ingestion
// endpoints live under `scheme://host/api/{project_id}/{envelope,store}/`.

import type { SdkSnippet } from '$lib/api';

/** Ingestion endpoint URLs derived from a DSN, matching the backend layout. */
type IngestUrls = { envelopeUrl: string; storeUrl: string };

/** Strip leading and trailing `/` from a path without a backtracking regex. */
function trimSlashes(path: string): string {
  let start = 0;
  let end = path.length;
  while (start < end && path[start] === '/') start++;
  while (end > start && path[end - 1] === '/') end--;
  return path.slice(start, end);
}

function ingestUrls(dsn: string): IngestUrls {
  const url = new URL(dsn);
  const projectId = trimSlashes(url.pathname);
  const base = url.origin;
  return {
    envelopeUrl: `${base}/api/${projectId}/envelope/`,
    storeUrl: `${base}/api/${projectId}/store/`
  };
}

/**
 * Build the per-language SDK init snippets for a project's DSN.
 *
 * The language snippets (Go/Rust/JS) embed only the DSN — every modern Sentry
 * SDK targets the `/envelope/` endpoint automatically. The `generic` snippet
 * documents the raw HTTP contract for both the modern `/envelope/` and the
 * legacy `/store/` endpoints, for callers without an SDK.
 */
export function sdkSnippets(dsn: string, publicKey: string): SdkSnippet[] {
  const { envelopeUrl, storeUrl } = ingestUrls(dsn);
  return [
    {
      language: 'go',
      label: 'Go',
      code: `import "github.com/getsentry/sentry-go"

err := sentry.Init(sentry.ClientOptions{
    Dsn: "${dsn}",
})
if err != nil {
    log.Fatalf("sentry.Init: %s", err)
}
defer sentry.Flush(2 * time.Second)`
    },
    {
      language: 'rust',
      label: 'Rust',
      code: `let _guard = sentry::init((
    "${dsn}",
    sentry::ClientOptions {
        release: sentry::release_name!(),
        ..Default::default()
    },
));`
    },
    {
      language: 'javascript',
      label: 'Svelte / JavaScript',
      code: `import * as Sentry from "@sentry/svelte";

Sentry.init({
  dsn: "${dsn}",
  tracesSampleRate: 0,
});`
    },
    {
      language: 'python',
      label: 'Python',
      code: `import sentry_sdk

sentry_sdk.init(
    dsn="${dsn}",
    traces_sample_rate=0,
)`
    },
    {
      language: 'java',
      label: 'Java',
      code: `import io.sentry.Sentry;

Sentry.init(options -> {
  options.setDsn("${dsn}");
});`
    },
    {
      language: 'kotlin',
      label: 'Kotlin',
      code: `import io.sentry.Sentry

Sentry.init { options ->
  options.dsn = "${dsn}"
}`
    },
    {
      language: 'generic',
      label: 'Generic / raw HTTP',
      code: `# Point any Sentry-compatible SDK at this DSN:
SENTRY_DSN=${dsn}

# --- Or send events over raw HTTP ---
# Auth via the \`X-Sentry-Auth\` header (or \`?sentry_key=\` query).

# Modern: newline-delimited envelope (recommended).
printf '{"event_id":"%s"}\\n{"type":"event"}\\n{"message":"hello"}\\n' \\
    "$(uuidgen | tr -d - | tr 'A-Z' 'a-z')" \\
| curl -X POST '${envelopeUrl}' \\
    -H 'X-Sentry-Auth: Sentry sentry_version=7, sentry_key=${publicKey}' \\
    -H 'Content-Type: application/x-sentry-envelope' \\
    --data-binary @-

# Legacy: a single bare JSON event (pre-envelope SDKs).
curl -X POST '${storeUrl}' \\
    -H 'X-Sentry-Auth: Sentry sentry_version=7, sentry_key=${publicKey}' \\
    -H 'Content-Type: application/json' \\
    -d '{"message":"hello"}'`
    }
  ];
}
