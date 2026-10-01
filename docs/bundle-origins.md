# Bundle Origin Browser Experiment

On September 30, 2026, installed Aegis 0.1.0 (protocol 1, macOS arm64)
successfully loaded random `a-<nonce>.localhost` and `b-<nonce>.localhost`
mounts through **one IPv4 loopback listener**. The parent used `127.0.0.1`
at the **same port**. Per-mount ports were unnecessary in this environment.
This supports random single-label `*.localhost` origins for Aegis preview;
it does not establish support in other browsers or deployments.

[The self-contained fixture](../tests/bundle-origin-browser.mjs) starts its own
HTTP server and Aegis runtime (default `127.0.0.1:17892`, override with
`BABBLE_ORIGIN_AEGIS_ADDR`), refuses occupied/reserved browser ports, and stops
both in `finally`. It only controls the browser with the installed Aegis CLI.
No production code, SDK contract, build, account credentials, upstream proxy,
DNS override, or certificate changes are involved. Cookies below are synthetic
test values. Request logs and browser observations are captured in trace stdout
and a printed temporary `observations.json` path.

## Observed Results

- Parent and both mounts reported `isSecureContext: true`. Transferred-port
  events carried the assigned nonopaque origins. `allow-scripts allow-same-origin`
  frames could not read the parent DOM; `frameElement` was hidden. This sandbox
  mode is appropriate only when the executable origin differs from the host.
- Identical fixture bytes executed under both mount names. Markers proved
  nested relative imports, a module cycle (`a:b:a`), and a dynamic import.
  Computed CSS proved relative stylesheet imports; request logs showed the
  relative PNG asset. CSP `self` blocked external imports, inline code and eval;
  undeclared module paths returned non-executable 404 responses.
- Exact-origin/exact-frame admission completed child-created port acceptance,
  confirmation and RPC. Early RPC did not dispatch. An external first document
  executed and offered a port but was rejected before admission. An unknown
  first document loaded without an offer. Same-origin and external replacements
  could not obtain another port; replacement load closed the original port.
- Removing the source check admitted a real same-origin sibling and dispatched
  RPC. Removing the origin check admitted a real external first document and
  dispatched RPC. These negative controls demonstrate the guard assertions
  detect their targeted omissions; they are deliberately faulty fixture hosts.
- A and B independently wrote/read localStorage and IndexedDB; B initially saw
  neither A value, and A retained its value after B wrote. Embedded `SameSite=Lax`
  cookie writes were unreadable. Separate **top-level positive controls** wrote
  and retained host cookies for both mounts, with matching HTTP Cookie headers
  on revisits. Neither mount received the other's cookie. Attempts to set
  `Domain=.localhost` were rejected, including in the top-level controls.
- Raw HTTP checks rejected unknown/suffix-spoofed/missing-port Hosts, unknown
  paths, query MIME overrides, traversal encodings, aliases and API paths.
  Errors were plain text with `nosniff` and restrictive CSP, without redirects.

## Validation

Use the actual Fozzy engine: on this machine it is `~/.cargo/bin/fozzy`;
`~/.local/bin/fozzy` instead identifies itself as the FozzyLang compiler.
Strict doctor (five runs, seed 42), strict deterministic test, and 20 scripted
fuzz runs passed for `tests/bundle-origin.fozzy.json`. These checks validate
the process scenario contract, **not browser execution or browser scheduling**.

The real host scenario is `tests/bundle-origin-host.fozzy.json`, run with
`--det --seed 42 --proc-backend host --fs-backend host --http-backend host`.
The final recorded evidence is
[`bundle-origin-host-verified.fozzy`](../artifacts/bundle-origin-host-verified.fozzy).
Strict trace verification, replay and CI passed. Replay consumes recorded host
observations; it does not launch the browser again. Report/artifact inspection
was also performed. Distributed `explore` explicitly rejects these step scenarios.
Shrinking the retained first failure preserved its single failing process step.

Earlier traces retain two harness failures: an invalid expectation that embedded
Lax cookies would work, and an unknown error document's repeated load events
closing the source-negative-control port. Cookie controls now run top-level;
source controls use a stable executable frame on the other mount origin.

## Remaining Production Work

This is an **origin-strategy experiment, not executable-byte verification**.
It does not test the production verifier, signed manifest, immutable snapshot,
SDK mount descriptor, acquisition protections, or complete execution containment.
The bridge here is a test fixture and does not certify the production bridge.

Production still needs a content domain separate from application/API domains,
wildcard DNS and valid wildcard TLS (or equivalent per-mount provisioning), exact
Host/SNI-to-mount routing, finite immutable file inventories, fail-closed unknown
hosts/paths, and deployed CSP/Permissions-Policy consistent with sandboxing.
Configure explicit parent origins, no credential forwarding/proxy routes,
unpredictable mount identifiers, mount expiry and storage/quota cleanup.

Do not extrapolate `.localhost` cookie rejection to ordinary wildcard content
domains: shared parent-domain cookies need a separately validated policy. Ports
alone also do not isolate cookies because cookie scope has no port component;
an ephemeral-port fallback would still need cookie controls and listener cleanup.
Never fall back to the application origin or accept opaque `null` origins.
