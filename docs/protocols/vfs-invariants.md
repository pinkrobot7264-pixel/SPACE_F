# SPACE VFS invariants (Phase 1 §3.5)

Part of the versioned contract (ADR-0011).

An invariant holds **after every operation, successful or failed**. Semantics
(`fs-semantics.md`) say what a call *returns*; invariants say the filesystem is
still **well-formed afterwards**. The second is what catches a Phase 2 namespace
bug.

## The checker is read-only

`check_invariants()` **observes and reports**. It never mutates, never repairs,
never allocates node state, and never resolves a violation it finds.

**A diagnostic that repairs is a diagnostic that hides bugs.**

It must be safe to call at any point in any test, and calling it must not change
any subsequent result. This is proven by a snapshot-comparison test: run it
against a fixture, compare state byte-for-byte before and after (§3.7).

---

## Identity — INV-ID

| ID | Invariant | Checked by |
|---|---|---|
| INV-ID-1 | Every live node has a unique `NodeId`, stable across rename, move and unlink | checker |
| INV-ID-2 | Every live node has a unique `index_number`, never reused within a process lifetime | checker |
| INV-ID-3 | A freed or stale `HandleId`/`CursorId` never resolves; after slot reuse the old value fails the generation check and does not reach the new object | conformance + fuzz |
| INV-ID-4 | Every live `HandleId` is unique and refers to exactly one live node | checker |
| INV-ID-5 | No identifier's value derives from a memory address; `0` is never a valid `HandleId` or `CursorId` | code rule + conformance |

**INV-ID-3 is the strongest safety property in Phase 1 and the entire
justification for ADR-0008.**

---

## Namespace — INV-NS

| ID | Invariant | Checked by |
|---|---|---|
| INV-NS-1 | The root exists, is a directory, and has no parent | checker |
| INV-NS-2 | Every live **linked** non-root node has exactly one parent and appears exactly once in that parent's child map. **Unlinked nodes are exempt** (fs-semantics §2) | checker |
| INV-NS-3 | The parent chain from any linked node reaches the root in at most L3 steps — no cycles | checker |
| INV-NS-4 | No directory is its own ancestor | checker |
| INV-NS-5 | No two children of one directory have colliding folded names | checker |
| INV-NS-6 | No operation resolves to, reads, or writes anything outside the SPACE namespace | conformance (§7.2) + ProcMon (§16.4) |

**INV-NS-6 has two halves deliberately:** a logical assertion proving the
resolver, and external evidence proving the process. You cannot assert "no
writes outside the boundary" from inside your own process.

---

## File state — INV-FS

| ID | Invariant | Checked by |
|---|---|---|
| INV-FS-1 | `file_size <= allocation_size`, and `allocation_size` is a multiple of the allocation unit | checker |
| INV-FS-2 | A node's `open_count` equals the number of live handles referencing it; it never underflows | checker |
| INV-FS-3 | A closed handle performs no I/O and no metadata mutation; every attempt is `InvalidHandle` | conformance |
| INV-FS-4 | An unlinked node remains usable through existing handles, is unreachable by path, and is reclaimed exactly at the `Close` that brings `open_count` to zero | checker + conformance |
| INV-FS-5 | Every state transition outside fs-semantics §1 is rejected and leaves state unchanged. **A panic is not a transition — it poisons** (ADR-0013a) | conformance |

---

## Enumeration — INV-DIR

Duplicate-entry and self-containment live in INV-NS-5 and INV-NS-4 and are not
restated.

| ID | Invariant | Checked by |
|---|---|---|
| INV-DIR-1 | An enumeration of a directory with N children terminates within N + 2 entries for any buffer size | conformance + property |
| INV-DIR-2 | Marker-chained enumeration returns each entry exactly once — no duplicates, no omissions — across any sequence of buffer sizes | property (§14.2) |
| INV-DIR-3 | A closed or never-allocated `CursorId` resolves to a controlled error, never to another directory's state | conformance + fuzz |

---

## Resources — INV-RES

| ID | Invariant | Checked by |
|---|---|---|
| INV-RES-1 | For every limit in `resource-limits.md`, `used <= limit` at all times | checker |
| INV-RES-2 | Exceeding a limit produces the specified error and never a panic, an abort, or an unbounded allocation | conformance + fuzz |
| INV-RES-3 | An operation that fails on a limit leaves state exactly as before — no partial write, no orphaned handle, no half-created node | conformance |

**INV-RES-3 matters more than the error code.**

---

## Considered and excluded

- *"Malformed input cannot corrupt internal state"* — that is INV-ID/NS/FS
  holding after fuzzed input, which is exactly how §14.1 asserts it. A separate
  rule would create two definitions of one requirement.
- *"Handle counts never negative"* — folded into INV-FS-2 (unsigned type,
  checked decrement).
- **Content integrity, checksums, durability — Phases 3 and 5.** Phase 1 has no
  authoritative second copy, so such an invariant would be **unfalsifiable
  here**. Claiming it would put fake integrity coverage in the exit gate.

---

## Mechanism

```rust
pub struct InvariantViolation { pub id: &'static str, pub detail: String }
```

- Compiled under the existing `fault-injection` feature (**no second flag**).
- Called by the conformance harness after **every** operation (§11.6) and by the
  fuzz harness after every operation in a sequence (§14.1).
- **Never called on the Windows-facing path in a release build**: it is
  O(nodes) and would make a 65,536-entry directory unusable.
- Every violation reports its invariant ID, so a failure names the rule.
- On violation **in a debug build**, the filesystem enters POISONING
  (ADR-0013a). It does not attempt repair.

## The checker must fire

A checker that never fires is not a checker. §3.7 constructs a structure
violating each of INV-NS-1, NS-2, NS-3, NS-5, ID-2, ID-4, FS-1, FS-2 and RES-1
and asserts the correct ID is reported.
