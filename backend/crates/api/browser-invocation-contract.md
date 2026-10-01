# Browser invocation wire contract

Backend workstream contract, September 30, 2026. Parent owns generated schema refresh.

RPC methods: `babble.clipboard.write` accepts `{text:string}`;
`babble.fullscreen.enter` accepts `{target_hint?:string|null,navigation_ui?:"auto"|"hide"|"show"}`.
Normalized fullscreen payload always has target_hint (null or string) and navigation_ui (default auto).
Old v1 methods return UnsupportedVersion.

Endpoints:
- POST `/invocations/v1/browser/prepare`: existing PrepareInvocationRequest.
- POST `/invocations/v1/browser/{id}/decision`: `{decision:"allow_once"|"deny"}`.
- POST `/invocations/v1/browser/{id}/dispatch`: `{}`.
- POST `/invocations/v1/browser/{id}/ack`: `{dispatch_id:Hash,result:BrowserInvocationResult}`.
- GET `/invocations/v1/browser/{id}/status`.
- POST `/invocations/v1/browser/{id}/cancel`: `{}`.

Same authentication and exact source document headers as social invocations.
BrowserInvocationResponse has the same common fields as InvocationResponse plus:
`result: BrowserInvocationResult|null` and
`execution_ticket: {dispatch_id:Hash,executor:"babble.browser.v1"}|null`.
Only the first successful dispatch response has an execution_ticket. Running/Unknown
dispatch_id is identity only: retries/status NEVER authorize another native call.

BrowserInvocationResult:
- `{kind:"clipboard_write",written:true}`
- `{kind:"fullscreen_enter",entered:true}`
- `{kind:"failed",code:"not_allowed"|"unavailable"|"context_lost"|"native_error"}`

Ack exact retries are idempotent; changed result or dispatch ID conflicts. Failed
ack yields failed state with the typed result retained. RPC initial consent uses
PERMISSION_REQUIRED.details.invocation with BrowserInvocationResponse.
