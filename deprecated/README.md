# Babel Protocol

Babel Protocol is an experimental social media concept built around contextual truth, evidence chains, reputation, and community consensus. The original proof of concept explores a network where posts are not only ranked by engagement, but also by provenance, source support, confidence votes, and evolving community validation.

This repository is currently a rough, partially working archaeological POC. It contains useful conceptual material and some compilable pieces, but the services are not integrated into a running product yet.

## Core Idea

Traditional social feeds optimize for attention. Babel's thesis is that a social protocol can instead make context and epistemic confidence first-class:

- Posts can carry evidence, context, citations, and author notes.
- Users can vote on engagement separately from truth/confidence.
- Reputation is derived from constructive participation, evidence quality, and historical accuracy.
- Content ranking balances relevance, authenticity, engagement, recency, and serendipity.
- Consensus state can evolve over time instead of treating moderation or fact-checking as a one-time verdict.

The older implementation guide in [implementation.md](implementation.md) captures the intended architecture in more detail.

## Repository Map

```text
.
|-- backend/          Go API server, SQLite persistence, websocket hub, models
|-- algorithms/       Python algorithm experiments for analysis, ranking, consensus, moderation
|-- hashgraph/        Java hashgraph/ledger experiment
|-- babel-frontend/   Next.js UI prototype (currently untracked in git)
|-- implementation.md Original architecture and product-flow notes
|-- pyproject.toml    Older/root Python package metadata
|-- go.mod            Placeholder/root Go module with no packages
```

## Current Status

### Backend

The Go backend compiles:

```bash
cd backend
go test ./...
```

There are no Go tests yet. The API server defines routes for content, votes, comments, context, consensus updates, reputation updates, analytics, direct messages, and websocket notifications.

Important gaps:

- `NewDBManager` opens SQLite but does not apply `schema.sql` or migrations.
- Several handlers are placeholders, including content retrieval, user profile/reputation, content analytics, and trending content.
- Authentication is a placeholder that accepts any non-empty `Authorization` header and sets `user_id` to `"placeholder"`.
- Database code and schema have drifted. Some queries reference columns or tables that do not exist in the current schema, such as `truth_score`, `visibility_score`, `certainty_level`, and an `evidence` table.
- Vote types in Go models include `upvote`, `downvote`, `affirm`, `deny`, `engage`, and `unengage`, but the SQLite schema only allows `true`, `false`, and `uncertain`.
- The content manager and algorithm clients are not wired into `main.go`; handlers mostly talk directly to the database.
- Redis/Postgres config exists, but the backend currently uses local SQLite and in-memory cache pieces.

### Algorithms

The Python directory contains sketches for:

- content analysis
- recommendation
- source-of-truth consensus
- community moderation
- engagement analytics
- temporal scoring

Current state:

```bash
python3 -m compileall -q algorithms
```

fails with:

```text
IndentationError: unexpected indent (algorithms/recommendation.py, line 134)
```

Other notable gaps:

- The FastAPI endpoints in `algorithms/main.py` do not match the Go clients, which call paths like `/analyze`, `/analyze/batch`, `/validate`, and `/consensus`.
- `AlgorithmInterface` method signatures do not match how `main.py` calls them.
- Some algorithm methods referenced by the interface are missing or have changed signatures.
- The saved test report under `algorithms/test/realtest/ALGORITHM_TEST_RESULTS.md` shows low historical accuracy for moderation, recommendations, and consensus despite the tests completing.
- The root `pyproject.toml`, `algorithms/pyproject.toml`, and `algorithms/requirements.txt` do not fully agree on dependencies.

### Frontend

`babel-frontend/` is a Next.js prototype with a carousel-based post UI, confidence/engagement controls, and shadcn-style UI components.

Current state:

```bash
cd babel-frontend
npm install
```

fails because `react-day-picker@8.10.1` expects `date-fns` `^2.28.0 || ^3.0.0`, while the app pins `date-fns@4.1.0`.

The frontend is also currently untracked in git. It appears to be a newer design prototype rather than part of the original committed POC.

### Hashgraph

`hashgraph/` is a Java experiment for transactions, signatures, and simple consensus ordering.

Current state:

- Maven is not installed on this machine.
- Java runtime is not installed on this machine.
- Test files are empty.
- The implementation is a local ledger/hashgraph sketch, not an integrated network or production consensus layer.

### Fozzy / Deterministic Tests

Fozzy is available on this machine, but this repository does not currently contain `.fozzy` scenarios or scenario fixtures. There is no deterministic system-test surface to run yet.

## What Works Today

- The Go backend packages compile.
- The data models show a fairly rich intended domain: content, votes, evidence, user reputation, consensus, direct messages, websocket events, and algorithm responses.
- The SQLite schema provides a useful first draft of core storage concepts.
- The frontend demonstrates a direction for the social UI interaction model.
- The algorithm code captures useful heuristics and conceptual scoring dimensions.

## What Does Not Work Yet

- There is no end-to-end working app.
- The backend does not initialize its database schema.
- The backend and algorithm service APIs do not agree.
- The Python algorithm package does not compile.
- The frontend dependencies do not install cleanly.
- The Java hashgraph experiment cannot be built in the current local environment.
- There are no real integration tests, deterministic scenarios, or CI checks.

## Recommended Rebuild Path

1. Define the product kernel.
   Start with posts, context/evidence, confidence votes, engagement votes, user reputation, and a ranked feed. Defer full decentralization until the local loop works.

2. Normalize the domain model.
   Create one canonical schema for `User`, `Content`, `Vote`, `Evidence`, `ConsensusState`, and `ReputationScore`. Make Go structs, DB tables, API payloads, and frontend types agree.

3. Make the backend real but small.
   Use Go + SQLite first. Add migrations on startup, proper repository methods, real CRUD handlers, and a simple feed endpoint.

4. Turn algorithms into a library or one service.
   Decide whether Python is a separate FastAPI service or an offline/library scoring layer. Then align endpoints and payloads with the backend.

5. Repair the frontend dependency graph.
   Downgrade `date-fns` to a compatible v3 release or upgrade `react-day-picker`, then wire the UI to the backend instead of demo data.

6. Add deterministic scenarios.
   Once there is a minimal product loop, add Fozzy scenarios for create-post, vote, add-evidence, recompute-consensus, and generate-feed.

7. Revisit decentralization.
   After the local product loop works, decide whether the hashgraph layer should be revived, replaced, or reframed as an append-only evidence/event log.

## Development Notes

Backend compile check:

```bash
cd backend
go test ./...
```

Python compile check:

```bash
python3 -m compileall -q algorithms
```

Frontend install/build check:

```bash
cd babel-frontend
npm install
npm run build
```

Fozzy is available, but scenarios need to be created before deterministic test runs are meaningful.

## Near-Term Target

The next clean implementation milestone should be a local single-node Babel:

- create users
- create posts
- attach context/evidence
- cast engagement and confidence votes
- calculate a deterministic consensus score
- generate a ranked feed
- show the loop in the frontend

That would give the project a stable spine before reintroducing more advanced protocol, reputation, and decentralization ideas.
