# Object Permissions

The host exposes permission review from an executable Object's action menu and
its Surface toolbar. Review is a native modal outside the embedded document.
Opening it stops the local Surface. Reading permissions never issues a grant.
The account label, capability ID/version, exact declared scope and server decision
remain visible before approval. Host-denied and unavailable requests cannot be
approved through this panel.

## Mutation Boundary

The controller binds each review to its Object and originating login. Closing,
changing accounts or selecting another review invalidates older results. The
host's authenticated transport checks that login before every request. Embedded
code receives neither the account token nor access to this dialog.

Allow submits the selected declaration unchanged. Revoke attempts every active
approved grant matching that exact declaration, not other scopes. Duplicate
submissions are blocked while a mutation is pending. The existing browser
controller follows an uncertain response with inspection rather than an
automatic write retry. Failed reconciliation removes mutation
controls until a fresh read succeeds. Concurrent approvals surviving revocation
are reported as remaining access.

The UI never reopens execution automatically. Open Object is a separate user
action, enabled only when the latest decisions are granted or explicitly support
lazy one-use invocation consent. Runtime admission
still rechecks the current server policy; a UI snapshot does not confer authority.

## Durable Consent Retries

Authenticated `babel.capabilities.grant.v1` and
`babel.capabilities.revoke.v1` require an envelope `idempotency_key`. REST
`POST /capabilities/grants` and `POST /capabilities/revocations` opt in with one
`Idempotency-Key` header; requests without it retain their existing behavior.
Keys must be nonblank and at most 256 bytes. Configured CORS origins may send
the header. This backend support does not add browser retry controls.

The signed consent event and private retry receipt commit in the same bounded
publication journal. A matching retry, including after restart, returns the
original event without creating another grant or revocation. The response's
`grants` field is the authenticated actor's **current projection**, not a saved
response snapshot. Retrying an old approval after revocation returns that old
approval event alongside its currently revoked grant; it does not restore
access. Retrying an old revocation after a new approval includes the new grant
and does not revoke it. Consumers must use current inspection/runtime admission
to determine access, not the historical event alone.

RPC keys are scoped by actor, binding origin and bound Object; REST keys use a
separate actor-scoped namespace shared by the two consent endpoints. Reusing
a key with a different method or payload conflicts (REST 409, RPC `CONFLICT`).
Changing between REST and RPC starts a different retry namespace. Changing the
binding origin changes the namespace only for RPC; REST keys do not include
origin. Authentication, actor ownership and host-only consent boundaries are
checked again on retries.
An uncertain outcome must be retried with its original key and intent.

The store remains single-writer. Committed journal recovery requires reopening
the node after a recovery-required error. Receipt retention/GC and deployment
recovery drills remain open; these guarantees are local durable retries, not
cross-node exactly-once delivery.

## Revocation And Running Sessions

Admission retains the exact grant chosen for each declared capability. A local
revocation retires every running Surface session depending on that grant before
the mutation returns. Imported consent events trigger the same reconciliation;
a foreign actor's invalid revocation does not affect the grant owner's session.
Another approval for the same scope never substitutes inside an existing
session. Other viewers' grants and unrelated Objects remain independent.

Permission reconciliation also runs under the API execution lock and during
the existing background session cleanup. Expired or missing admitted grants
retire sessions. Terminal sessions cannot execute, renew, reactivate, or write
checkpoints. Their admission plans remain available as historical audit data,
not a current authority snapshot. Fresh approval requires a fresh session.

The browser removes a remotely revoked iframe when its normal heartbeat fails
(renewal is scheduled at most 15 seconds apart). This is bounded lease-based
propagation, not instantaneous push cancellation. Already delivered JavaScript
can continue locally until the host processes that failure; further broker
calls and gateway resource requests are denied by the server. Disconnected or
stalled requests are bounded by the existing heartbeat watchdog. Gateway
snapshots and account ownership records are reclaimed by background cleanup.

## Browser Action Confirmation

The Astro Surface host executes `babel.clipboard.write.v2` and
`babel.fullscreen.enter.v2` through durable one-use invocation consent. V1 calls
are rejected. Allow once obtains approval and the sole dispatch ticket, then a
separate Copy once or Enter fullscreen gesture invokes the native operation.
Clipboard confirmation shows the exact text as text, not markup.
Fullscreen applies only to the trusted Object panel, including its exit controls;
an application's `target_hint` never selects an element in the host document.
Native browser permission or activation failures return RPC errors. Success is
delivered to the embedded app only after native completion and its durable
acknowledgement. Acknowledgements are host-reported, not server-observed proof.

Only one browser or social consent request may be outstanding for a mounted Object. Requests expire
within the bridge deadline, at most 30 seconds. Cancellation, account changes,
lease loss, hiding the page or closing the Object remove pending prompts. Closing
also exits fullscreen owned by this host. A clipboard write already admitted by
the browser cannot be rolled back. Raw HTTP cannot operate a browser. The first
winning dispatch response alone admits one native attempt; status and retries
never recreate that authority. Lost acknowledgement retries the same outcome,
not the effect. See the [invocation contract](invocation-consent.md#native-browser-methods).

These two methods and the four social mutations admit declared available
capabilities lazily without reusable grants. Permission management shows Ask
every time and cannot create a durable approval for them. Historical grants
cannot substitute for an invocation decision.

## Open Security Requirements

The five remaining external `ask_each_time` methods (payments, generation,
transcription, camera and microphone) still lack the durable executor contract.
Their action descriptors and historical grant behavior are not completed effects
or proof of one-time consent. Sensitive AskOnce lazy prompting also remains open.

Revocation propagation between independent federated nodes still depends on
delivery of the signed event; this is not a cross-node push guarantee. Revoking
consent cannot undo an already completed external action or erase copies of
delivered bytes. The browser application picker now supports
[capability declarations](application-authoring.md#declared-permissions), but
declaration is not consent, runtime admission or proof that an external provider
executor exists.
