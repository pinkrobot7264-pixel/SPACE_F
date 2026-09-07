# ADR-0012 — Share access is WinFsp-owned

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §3.3.3, §10.1

## Context

Windows share-access semantics (`FILE_SHARE_READ` / `_WRITE` / `_DELETE` against
a requested `DesiredAccess`) are intricate, and getting them subtly wrong
produces application-visible misbehaviour that is very hard to attribute.

WinFsp's FSD already implements share-access checking in the kernel, correctly,
for every filesystem it hosts.

## Decision

**WinFsp's FSD performs share-access checking. SPACE does not.**

The core records `granted_access` per handle **for diagnostics only** and does
not enforce it.

## Consequences

- One fewer subsystem to get wrong, and the one we would have written could only
  have been less correct than the kernel's.
- Phase 1 tests **the layering, not our code**: an exclusive open followed by a
  second open must yield `STATUS_SHARING_VIOLATION`. If that assertion fails,
  the volume parameters or the create path are wrong — the test is a check on
  our wiring, and it is labelled as such.
- `SharingViolation` remains in the taxonomy because the boundary can observe it
  coming back from WinFsp, not because the VFS produces it.

## Enforcement

§10.1 share-access test, run through a real mount.
