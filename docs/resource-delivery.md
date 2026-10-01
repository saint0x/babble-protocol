# Resource Admission And Delivery

This milestone hardens declared resource locations and the local blob gateway.
It does **not** verify arbitrary external executable bytes, redirects, or their
dependency trees. The [verified-bundle contract](verified-bundles.md) proposes
that separate, still-unimplemented boundary.

## Declared Locations

Authoring and runtime admission share a structured URL parser. Accepted locations
are canonical `babble://blobs/<hash>`, safe relative references, HTTPS, and HTTP
whose parsed host is loopback. Credentials, control characters, backslashes,
traversal, ambiguous path escapes, and malformed hashes fail admission. Blob
URIs must contain the exact declared lowercase BLAKE3 digest.

Executable admission requires integrity, URI, and compatible MIME to agree on
one declared resource. It checks all matching candidates, not only the first
same-hash resource. Authoring retains its existing duplicate-resource-hash
rejection; runtime also validates signed/imported Objects with aliases.

An entry matches its signed resource URI exactly. A Babble blob may also use a
recognized gateway route with the exact hash and one matching `media_type`
selector. Gateway URL shape does not authenticate that server or verify its
response. External URLs and safe relative references remain declarations, not
proof of acquired bytes. Loopback admission is a local preview facility, not an
SSRF-safe external acquisition policy.

## Local Blob Gateway

All three public reads use the same bounded, digest-verifying store operation:

- `GET /runtime/surfaces/blobs/{hash}` returns raw executable resource bytes.
- `GET /media/blobs/{hash}` returns the media descriptor and hex-encoded bytes.
- `babble.media.blob.get.v1` returns the equivalent RPC result.

The maximum buffered blob is **8 MiB**. JSON requests have a **16 MiB plus 64 KiB**
body ceiling, including their envelope and hex-encoded payload. This admits an
8 MiB upload with bounded envelope space; decoded payloads above 8 MiB still fail
before allocation. This is not streaming large-media support. JSON response
encoding consumes additional bounded memory beyond the raw blob buffer; the
raw-byte cap is not a whole-process memory limit.

Reads reject noncanonical hashes before lookup, inspect the opened regular file,
and reject oversized metadata before allocating the payload. A chunked read
checks for growth with at most one byte beyond the limit. Every successful read
recomputes the digest over the exact returned bytes. Oversized reads produce
HTTP 413 or RPC `QUOTA_EXCEEDED`; digest mismatches produce conflict, malformed
hashes produce invalid input, and missing canonical hashes produce not found.
Unsupported executable MIME is rejected before reading the file.

The native `FileStore::get_blob` / `LocalNode::media_blob` APIs retain explicit
unbounded buffering for trusted callers. Network handlers must not use them.
The store is private, trusted local storage: this is not protection against a
malicious filesystem, symlink replacement, device files, or stalled disk I/O.
There is no hard filesystem-operation deadline. Single-writer and deployment
constraints in the readiness ledger still apply.

## Verification

Native coverage includes canonical hash rejection, missing and tampered files,
empty/exact/next-byte bounds, an unbounded growth reader, reopen behavior,
nonregular files, a 1 TiB sparse-file rejection, and native reads above the public
ceiling. API tests exercise all public paths, including exact/next-byte bounds,
MIME rejection, CSP/nosniff headers, and stable HTTP/RPC failure mapping.

The live-stack fixture modifies only its disposable store. It verifies real
HTTP/RPC corruption and size rejection, restores the original bytes in `finally`,
checks their exact delivery, then exercises the actual Surface through Aegis.
It never changes the persistent preview store or signed Objects.

Current verification evidence and trace filenames are recorded in the
[readiness ledger](production-readiness.md). Scripted Fozzy checks cover process
orchestration; host traces execute the real Rust and browser tests. Neither is
an independent security audit or evidence of complete executable isolation.
