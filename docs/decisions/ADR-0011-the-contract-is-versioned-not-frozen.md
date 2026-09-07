# ADR-0011 — The contract is versioned, not frozen

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §11.5, "Phase 1 → Phase 2 handoff"

## Context

Phase 1 exists to produce a contract Phase 2 implements. "Frozen" is the wrong
word: contracts do change, and a contract that cannot change gets worked around
instead of amended. What must not happen is a *silent* change.

## Decision

```rust
pub const VFS_CONTRACT_VERSION: u32 = 1;
```

The versioned contract is, exactly:

| Part | Location |
|---|---|
| the `Vfs` trait | §3.2 |
| the semantics document | §3.3 / `docs/protocols/fs-semantics.md` |
| the resource limits | §3.4 / `docs/protocols/resource-limits.md` |
| the invariants | §3.5 / `docs/protocols/vfs-invariants.md` |
| the concurrency model | §3.6 / `docs/protocols/concurrency.md` |
| the `Capabilities` set | §11.5 |

**Changing any of them requires a version bump and an ADR.**

## Consequences

- Adding a `Capabilities` flag is a contract change. The deliberately brittle
  `capability_count_is_deliberate` test (`size_of::<Capabilities>() == 1`)
  exists to make that impossible to do by accident.
- The Phase 1 → Phase 2 acceptance test is: replace `MemVfs` with the real VFS,
  change no adapter code, rewrite no semantic tests, and the conformance suite
  passes unmodified. If that cannot happen, Phase 1 froze the wrong contract.

## Enforcement

`VFS_CONTRACT_VERSION == 1` is asserted by test; `capability_count_is_deliberate`
guards the capability set.
