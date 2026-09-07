# ADR-0008 — Identifiers are opaque and generational, never pointers

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §3.1

## Context

The WinFsp `FileContext` is a `PVOID` that the kernel hands back to us on every
callback. The obvious implementation — store a raw pointer to a heap object —
turns any stale, duplicated or hostile context value into a dereference of freed
memory. That is a use-after-free reachable from any process that can open a file
on `S:`.

## Decision

**No identifier that crosses the FFI is, or derives from, a memory address.**

`HandleId` and `CursorId` are `u64` values carrying a generation:

```
(generation as u64) << 32 | (index as u64 + 1)
```

The `+ 1` guarantees `0` is never a valid identifier. Resolution checks, in
order: index within range → slot live → generation matches. Any mismatch is
`InvalidHandle` / `InvalidParameter` — never a panic, never a dereference.

`NodeId` is internal and never crosses the FFI, but is also generational (slab
index + generation) for the same reason.

`index_number` — the only identifier Windows sees — is **not** derived from a
slab index, because slab indices are reused after free and Windows treats
`index_number` as a file identity key. It comes from a monotonic `u64` counter
that is never reused within a process lifetime.

## Consequences

- A stale identifier fails validation instead of aliasing a live object. This is
  the strongest safety property in Phase 1 (INV-ID-3).
- Slot reuse is safe and cheap; no quarantine or deferred free is needed.
- Generation wraparound at `u32::MAX` must skip generation 0 so a wrapped slot
  cannot collide with a never-allocated one. Contrived, tested anyway.

## Enforcement

INV-ID-1 … INV-ID-5. Conformance suite (`handles` module), the handle-table unit
tests (§5.4), and the `fuzz_handle` / `fuzz_cursor` targets, which feed arbitrary
`u64` values to `resolve` and `free`.
