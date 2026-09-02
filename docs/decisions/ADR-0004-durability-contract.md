# ADR-0004: Durability contract

- Status: Accepted
- Date: 2026-09-02

## Context

Writes must be crash-safe before they reach the cloud. The client needs a
precise statement of what an acknowledged write guarantees, so Phase 5 can build
a WAL and recovery against a fixed target rather than a vague intention.

## Decision

Durable local state is recoverable. An interrupted write must recover to the
last valid committed state; queued-but-not-committed work either resumes or is
discarded, and is never observable as a committed version. Specifically:

- A write is acknowledged only after local durable state (WAL + chunk staging)
  has been persisted according to `config.write.wal_fsync_policy`
  (`always` in production; `never` is test-only and refused in release builds).
- Committed chunks are immutable and are never overwritten; a modify writes new
  chunks.
- The upload queue and its progress are persisted so an interrupted upload
  resumes.
- A new version becomes authoritative only after its manifest validates and the
  commit completes (see ADR-0006). A crash can leave at worst an uncommitted
  `Candidate`; it can never corrupt a `Committed` version (guard rails #4, #5).

## Consequences

- Phase 5 builds WAL, upload-queue persistence, dirty-data bounds and startup
  recovery to this contract.
- Repeated crash injection (Phase 5, Phase 9) must not produce silent data loss
  or an invalid committed version.

## Revisit when

Performance work needs a weaker default fsync policy, which would require an
explicit, documented durability-vs-throughput setting rather than a silent
change.
