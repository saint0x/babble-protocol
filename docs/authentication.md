# HTTP Accounts And Authority

The network API and the native node API have different trust boundaries. Native
Rust callers operate the local node and its signing keys. HTTP callers must prove
an account session before requesting a signature or accessing private state.
An Object ID, author ID, RPC host binding, or CORS origin is not authentication.

## Accounts

- `POST /auth/register`: `{handle, kind: "Person", password}` creates a new
  identity and an account. It never claims a previously published identity.
- `POST /auth/login`: `{identity_id, password}` signs into that same account.
  Handles are not unique identifiers in the protocol.
- Both return `{identity, token, expires_at}`. Send the token in an
  `Authorization: Bearer ...` header, never in a URL or Object payload.
- `GET /auth/session` verifies the session and returns `{identity, expires_at}`.
- `DELETE /auth/session` revokes that session and returns HTTP 204. Other device
  sessions are independent.
- `GET /auth/sessions` lists the account's active sign-ins, including a current
  marker, creation time, expiry, and a stable opaque management ID. This ID is
  independent of the credential and cannot authenticate a request. Legacy
  creation times are null rather than inferred.
- `DELETE /auth/sessions/{id}` revokes one owned sign-in, including the current
  one. Already absent or foreign IDs return the same 204 without exposing another
  account. `POST /auth/sessions/revoke-others` takes an empty body and keeps only
  the acting sign-in.
- `POST /auth/password` takes `{current_password, new_password}`. A successful
  204 means the password changed and **all sessions were revoked**, including
  the caller. An incorrect current password returns 403 without signing out the
  caller. Missing or expired authentication returns 401.

New passwords require at least 15 Unicode scalar values and at most 1024 UTF-8
bytes. Whitespace and Unicode are accepted without trimming, normalization, or
silent truncation; there are no mandatory character classes. Existing passwords
remain valid for login until changed. Password-manager paste and autocomplete
remain available. The length policy and current-password verification follow
the [OWASP Authentication Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html),
checked through Aegis on 2026-09-30. This is not a claim of complete OWASP/NIST
conformance: compromised-password screening and additional factors remain open.

Password verification runs outside storage locks. Before issuing a login, the
store checks that the verified password hash is still current. Password changes
recheck the acting session and observed hash, then update the hash and revoke
all sessions in one transaction. This prevents an in-flight old-password login
from creating a session after the change. The existing execution gate rejects
revoked credentials before queued work executes; already executing mutations
are not rolled back.

Session management follows the
[OWASP Session Management guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
on inspecting and terminating active sign-ins. The UI does not claim to identify
physical devices, collect IP addresses, or invent browser fingerprints. Remote
Surface access stops at the authentication boundary; browser execution stops
when its host detects revocation or loses its bounded lease. Failed runtime
cleanup remains eligible for the existing durable cleanup sweep.

Passwords are salted Argon2id hashes, using 19 MiB, two iterations and one lane.
The work factor meets the current minimum from the
[OWASP Password Storage Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html),
read through Aegis during implementation. Password work runs outside the node
mutex with bounded concurrency. Random 256-bit session tokens are stored only as
hashes; sessions expire after seven days. The account database lives under
`BABEL_STORE_ROOT/auth/`, separate from public protocol events and exports.

The Astro account dialog supports registration, sign-in, inspection of the full
identity ID, active-session management, password changes, and server-backed
sign-out. It stores the session in tab-scoped
`sessionStorage`, partitioned by API origin. Reload restores it through the server;
closing the tab requires signing in again. Disabled browser storage leaves the
session in memory for that page. Passwords are never persisted by the client.

Local personalization/history keys are partitioned by API origin and account
identity, separately from the session token. Signing out restores the anonymous
namespace, not another account's preferences. Legacy unpartitioned history is
not silently assigned to a new account. This is logical separation inside the
same browser origin, not protection against an attacker with local browser access
or same-origin script execution.

The password-change form states that every sign-in will end before submission.
Passwords are cleared from inputs on submission, dialog close, and account
change; they are not persisted. A missing acknowledgment does not prove the
change failed. The UI offers an explicit sign-in check with the new password,
then the previous password if needed, rather than automatically retrying a
credential mutation. Late completions cannot clear a replacement account.

Old browser `babel.frontend.author.v1` IDs are not login credentials and are not
silently adopted. Seeded and CLI-created identities do not automatically acquire
password accounts. Existing posts remain readable. Account recovery must not be
implemented by trusting a handle or public identity ID.

## RPC And Embedded Applications

The host alone owns the authenticated HTTP transport. It refuses credentialed
requests to other origins and HTTP redirects. Embedded Surfaces receive their
scoped bridge, not the session token. The SDK host replaces caller-supplied
Object/session/identity/grant bindings and refuses host-only operations before
dispatch: direct authoring, permission changes, identity creation, administration,
and host lifecycle mutations are not delegated to a game or portal.

Capability-backed SDK operations still require server authorization. A viewer's
session ownership does not authorize using another viewer's consent grants or
signing arbitrary content. HTTP policy must also enforce this independently of
the SDK, since a malicious client can bypass a browser library.

The public read path remains available without a session. The account form is
required for publication and interactive Surface sessions. Ordinary Settings
shows public protocol catalogs, not operator metrics or private event history.
Administrative routes require a separate `BABEL_OPERATOR_TOKEN` configured on
the server; never put this token in the frontend build or a Surface.

## Release Work Still Open

This boundary is not a declaration of full production readiness. Deployment
still needs HTTPS, appropriate network-level abuse protection, credential/key
backup and recovery procedures, compromised-password screening, additional
authentication factors, security activity/alerts, and a complete authorization
regression audit. Password recovery is still absent; session-management controls
are not a recovery mechanism. The current account storage
requires Unix private-file permissions. Native node/storage access remains a
trusted operator capability. Public multi-node operation, semantic-provider
integration, and the remaining social workflows are separate requirements in
`production-readiness.md`.
