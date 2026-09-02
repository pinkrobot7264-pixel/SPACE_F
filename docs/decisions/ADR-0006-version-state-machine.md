# ADR-0006: Version state machine

- Status: Accepted
- Date: 2026-09-02

## Context

Each committed file state is an immutable version. Building a version involves
several steps (write chunks, build manifest, validate, publish). A reader must
never see a half-built version, and a crash mid-build must not corrupt history.

## Decision

A `Version` has exactly two states and one legal transition
(`contracts::model::VersionState`, `contracts::validate::inv10`):

```
Candidate  --commit-->  Committed
```

- `Candidate -> Committed` is the only permitted transition. `Committed` is
  terminal; `Committed -> Candidate` and self-transitions are rejected.
- A `Candidate` is invisible to readers. `get_current_version` returns
  `VERSION_NOT_FOUND` for anything not `Committed`.
- `commit` re-validates the full manifest (all 13 invariants) *before* flipping
  the state and pointing `File.current_version_id` at it. If validation fails,
  the version stays `Candidate`.
- Commit is idempotent: re-committing an already-`Committed` version returns
  success without change.
- Old versions are never mutated (invariant 12 keeps `parent_version_id` within
  one file).

## Consequences

- A crash during version build leaves at worst an uncommitted `Candidate`
  (guard rail #4).
- Phase 6 implements the atomic commit protocol to this state machine.

## Revisit when

A multi-writer conflict model (Phase 10) needs an intermediate "superseded" or
"rejected" state -- added as new terminal states, never by making `Committed`
mutable.
