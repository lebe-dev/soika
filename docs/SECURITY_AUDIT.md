# Security audit — 2026-10-29

The audit reviewed YAML/UI management boundaries, first-run provisioning,
local accounts and environment Owner grants, password login, session resolution,
passkeys, OIDC/GitHub SSO, invitations, authorization and dependency advisories.
It used source review and regression tests against SQLite and PostgreSQL.
It did not test a deployed installation or audit the external identity provider,
reverse proxy, container host or secret storage.

## Findings and fixes

| Finding | Impact before the fix | Resolution |
| --- | --- | --- |
| Replayable WebAuthn state cookie | A copied challenge and assertion could be reused within the cookie lifetime | Persist state behind an opaque random handle and atomically consume it before verification; purpose and expiry are enforced by both DB adapters |
| Unverified registration of `ADMIN_USERS` identities | Public signup or invite registration could mint an effective Owner by choosing a configured email | Reserve these addresses for existing local accounts or verified SSO; block anonymous setup when configured Owner identities exist |
| Concurrent initial setup | Different emails bypassed the pre-insert Owner count and created multiple Owners | Check and create under a SQLite immediate write transaction or a PostgreSQL singleton-row lock |
| Pending-user session bypass | Some password/session paths ignored approval status; duplicated extractors drifted | Share a single session resolver and enforce approval on every credential path |
| Manager approval of an effective Owner | A Manager could activate a privileged pending identity | Require an Owner to approve an effective Owner |
| Addressed invite accepted by another account | Possession of a link granted access without matching its email | Validate the recipient before account creation and again inside the acceptance transaction |
| Concurrent invitation reuse | Checking the token, granting membership and marking acceptance were separate operations | Atomically consume a live token and add membership; preserve existing Admin access |
| Missing CSRF enforcement | Cookie-authenticated bodyless writes and anonymous auth flows relied on SameSite/CORS | Enforce source-origin checks with a preflight-protected custom-header fallback; keep ingest separate |
| Forged forwarding headers | An attacker could change the apparent IP to escape password/passkey lockout | Use the TCP peer; document proxy-side per-client throttling |
| Unbounded failed-login subjects | Spraying distinct emails grew the guard's live map | Bound tracked subjects and apply a shared overflow budget; validate email shape and length |
| Anonymous challenge-table growth | Starting ceremonies without submitting assertions created unlimited writes | Apply an independent per-instance options budget and expire old state |
| SSO account enumeration and unthrottled bad guesses | Known non-Owners got a distinct response without recording password failures | Give unknown/SSO-only accounts equivalent hashing work; only proven credentials can receive the SSO hint |
| Case-folded IdP group identifiers | Distinct groups differing only in case could share privileges | Match exact group names; clear old lossy snapshots and require fresh verified SSO claims |
| Browser-normalized redirect controls | Control characters could turn a seemingly relative target into a foreign URL | Reject control characters in backend and frontend return targets |
| Internal error disclosure and browser defaults | DB/SMTP/internal causes reached clients; session-dependent reads had inconsistent cache policy | Mask internal causes, disable API/auth caching and add frame, MIME and referrer protections |

YAML/UI write restrictions, project/team access checks, last-Owner/last-team-Admin
protections, verified SSO email/domain checks, userinfo subject binding, OIDC
state/nonce/PKCE validation and SSO conversion credential revocation were also
reviewed. Existing tests cover these boundaries. OIDC/GitHub calls now have a
15-second timeout and disable redirects.

## Dependency findings

- Tokio 1.54.0 → 1.54.1 fixes
  [CVE-2026-115567](https://github.com/tokio-rs/tokio/security/advisories/GHSA-5wjr-9356-4wc8).
- SvelteKit 2.70.3 → 3.x fixes
  [CVE-2026-114060](https://github.com/sveltejs/kit/security/advisories/GHSA-pfhg-q3qv-9q6r)
  and [CVE-2026-114061](https://github.com/sveltejs/kit/security/advisories/GHSA-27mw-82g8-phjj).
  The compatible static adapter and TypeScript were updated; the vulnerable
  transitive `cookie` 0.6.0 was replaced as part of this upgrade.
- Upstream deepmerge 4.3.1 has no published fix for
  [CVE-2026-93753](https://github.com/TehShrike/deepmerge/issues/273).
  `frontend/vendor/deepmerge` contains the MIT-licensed upstream implementation
  with a local patch that filters `__proto__`, `constructor` and `prototype`
  from both source and target keys. Yarn resolves every consumer to this copy
  (`4.3.1-soika.1`). Tests exercise null-prototype targets, malicious source and
  target objects, nested merges, array options and symbols. The dependency is
  used by the Vite build plugin, not by the Rust HTTP server.

Trivy scans include development dependencies. Its version-range database still
flags the locally patched deepmerge version; this finding is documented rather
than hidden with an ignore rule. Replace the local patch with a verified
upstream release when one becomes available. A clean production-only scan does
not imply that all build tools are free of advisories.

## Upgrade and operational notes

Read [the operator guide](OPERATIONS.md#browser-requests-and-security-boundaries)
for CSRF headers, public-origin configuration, invitation behavior and proxy
rate buckets. Existing passkeys and sessions remain stored; an in-progress
WebAuthn ceremony must restart. Group-only access is recalculated without old
case-folded snapshots and restored on the next matching verified SSO login.
Local accounts intended for `ADMIN_USERS` must exist before enabling that list,
or be created through verified SSO.

The configuration file and environment are trusted administrator inputs. Protect
them and the database from other users on the host. Public setup remains a
first-run provisioning surface when enabled without `ADMIN_USERS`; complete it
before exposing a fresh instance, or disable it. Webhook destinations can include
private networks, so network egress and the users allowed to administer projects
remain part of the deployment trust boundary. IdP membership revocation takes
effect on the next SSO login; existing sessions do not introspect the IdP on each
request. In-memory rate limits are per replica. This audit is not a guarantee of
absence of other vulnerabilities.


## Validation

- Full backend suite on SQLite: 590 tests passed.
- PostgreSQL authentication, invitations, instance roles, passkeys and repository
  contracts: 187 tests passed.
- Frontend: 227 tests passed, formatting and Svelte/TypeScript checks passed,
  production static build completed into `web/dist`.
- `just lint` (formatting and clippy with warnings denied) passed.
- Trivy dependency scans included development dependencies. Cargo dependencies
  have no remaining reported vulnerability; the sole frontend finding is the
  documented locally patched deepmerge version. No ignore rule was added.

WebAuthn replay regression checks the one-use persistence boundary and failed
assertion reuse; a hardware-authenticator ceremony was not exercised. The tests
use a fake provider for HTTP callback flows; token verification and userinfo
validation have separate library-level tests. No live IdP account was used.
