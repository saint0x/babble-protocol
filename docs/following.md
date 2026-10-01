# Following People

Person-following is a private, authenticated host workflow, not an Object graph
edge. Existing object-bound `social.follow` capabilities still follow Objects;
they do not grant an embedded Surface access to the account's followed people.
Private follow records are not published as public events or discovery edges.
Private means account-scoped access control and restricted local files, not
end-to-end encryption: the node operator can read the private database.

## HTTP Contract

All routes require an active user bearer session. The server derives the acting
identity from that session; clients cannot choose an actor in the body or query.
Responses use `Cache-Control: no-store`.

| Route | Purpose |
| --- | --- |
| `GET /social/following/{identity_id}` | Current state and revision for this viewer/target pair. |
| `PUT /social/following/{identity_id}` | Set the desired state using a revision and retry key. |
| `GET /social/following?limit=20&cursor=...` | Private list of followed identities and authoritative handles. |
| `GET /feed/following?limit=20&cursor=...&search=...` | Chronological Objects authored by followed identities only. |

Mutation body:

```json
{
  "following": true,
  "expected_revision": 0,
  "idempotency_key": "unique-request-key"
}
```

An absent pair has revision zero and `following: false`. Actual state changes
increment the revision. A stale revision or reuse of a key for another intent
returns 409. Retrying the same key and complete request returns its original
result without overwriting newer state. Fetch current state after mutation: an
old successful receipt can describe an earlier revision. Storage unavailability
returns 503, not a false conflict. Retain the same intent and key after an
uncertain timeout or storage failure. Self-follow is rejected.

## Reading And Pagination

Pages accept limits from 1 through 50, defaulting to 20. Search accepts at most
256 bytes without control characters. Cursors are opaque bookmarks, bound to the
viewer, view, query, limit, and snapshot. Changed follow membership or new Objects
invalidate the feed snapshot with 409; restart at page one. Backdated imports
also invalidate it. A repeated import of an existing Object does not.

The feed merges per-author indexes in descending `(created_at, id)` order. It
never substitutes discovery results for an empty list. There is no private-feed
reranking; explicit local author/term exclusions may hide posts without changing
the remaining order or transmitting local history.

Search examines complete text payloads, not a truncated prefix. One request
examines at most 10,000 indexed Objects and 16 MiB of text, allowing one complete
Object to ensure progress. An empty filtered page can therefore have a next
cursor. Clients must continue until `next_cursor` is null, not until the page is
short or empty. Output has a 16 MiB serialized-Object budget, also allowing one
Object for progress. Public profile pages share that output bound. The browser's
response parser allows 32 MiB, including envelope overhead, and rejects larger
responses.

## Persistence And Limits

The private SQLite store commits signed follow actions, materialized pair state,
snapshot versions, and signed retry receipts transactionally. Reopen verifies
records against signing-key history. This does not protect against rollback of
an entire store by a privileged operator; there is no external integrity anchor.

The active-follow limit is 10,000 identities per account. Retry keys are bounded
to 256 visible ASCII bytes. Action and receipt history is retained indefinitely;
deleting it would break retry guarantees. A retention policy, operational size
monitoring, load testing, and backup/recovery drills remain deployment work.
Keep one serving node per store, its directory private, and its HTTP listener
behind the deployment controls described in the readiness ledger.

## Frontend

Public author profiles provide Follow/Following with pending, retry, and conflict
states. The account's own profile hides the control. Guests can open sign-in but
cannot read or mutate private following state. The Following selector uses the
existing rounded swipe deck, with explicit pagination and a separate private
people list. Account changes cancel pending views and discard previous-account
results. These workflows do not add public follower counts or notifications.
