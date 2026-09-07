# SPACE Phase 1 — certification report

**Status: NOT READY.** Phase 1 is substantially complete and its core contract
is proven, but several manual-mandated activities have not been run, and some
cannot be run without a human. The detail is below, item by item. This document
is written to be checkable by someone who does not trust it.

- Commit: see `git-commit.txt` in the evidence directory
- Branch: `phase/1-winfsp`
- Requirement matrix: `REQUIREMENTS.md` (75 rows)

---

## 1. Summary of verdicts

| Area | Verdict | Basis |
|---|---|---|
| Manual requirements | **PARTIAL** | 59 of 75 matrix rows PASS with evidence; 16 outstanding, itemised in §4 |
| Implementation | **PASS** | full stack builds and mounts; `S:` serves Windows |
| Build | **PASS** | debug + release, zero warnings, clippy clean workspace-wide |
| Unit tests | **PASS** | 262 default profile, 175 with `fault-injection`, 146 in release |
| Integration tests | **PASS** | mount functional test 19/19 against a live `S:` |
| Conformance | **PASS** | 11 modules, both capability variants, no WinFsp required |
| Property tests | **PASS** | 9 proptest properties, seeds persisted |
| Fuzzing | **IN PROGRESS** | 6 targets build and run under libFuzzer + ASan; ≥30 min each pending |
| Fault injection | **PASS (in-process)** | full §13.5 matrix; the Windows half needs a human for Explorer |
| Concurrency | **PASS** | deadline-bounded lock proven; §3.6 prediction confirmed in-process |
| Resource limits | **PASS** | L1–L8 at limit and limit+1 with state comparison |
| Windows compatibility | **PARTIAL** | 4 scripted clients pass; Explorer/Notepad/7-Zip need a human |
| Crash recovery | **IN PROGRESS** | kill matrix: idle ✅, mid-write ✅, mid-enumeration running |
| Stress / soak | **PARTIAL** | 200 mount/unmount cycles PASS; 4-hour soak not yet run |
| Invariant validation | **PASS** | checker proven to fire (12 tests) and proven read-only (3 tests) |
| Evidence | **PARTIAL** | collector written; several artifacts not yet produced |
| Documentation | **PASS** | 5 protocol docs, 9 ADRs, 1 runbook, all matching the implementation |

**Final verdict: NOT READY.** Not because something is known to be broken, but
because the manual's exit criteria include activities that have not happened
yet. Declaring PASS now would be exactly the "declare something complete
without evidence" failure the manual forbids.

---

## 2. What was proven, and how

### The contract (the actual point of Phase 1)

The `Vfs` trait, the semantics, the limits, the invariants and the concurrency
model are written down in `docs/protocols/` and are executable: the conformance
suite runs against any implementation, with no WinFsp and no mount, in about a
second. A CI job runs it on a runner where WinFsp is **asserted absent**, so
the property is enforced rather than claimed.

The Phase 1 → Phase 2 acceptance test — *replace `MemVfs`, change no adapter
code, rewrite no semantic tests, suite passes unmodified* — is structurally
ready.

### The invariant checker

It fires (12 tests construct a violating structure per checker branch and
assert the reported ID) and it is read-only (3 tests, including one that proves
it does not "helpfully repair" a violation it finds, by snapshot comparison).

One design change was needed to make this true: `allocation_size` was a
computed `max(rounded, explicit)`, which made INV-FS-1 true by construction and
its checker branch unreachable. Allocation is now a stored property the
implementation maintains, so the invariant is falsifiable and can catch a
Phase 2 bug.

### The panic model

`catch_unwind` at every entry point, in **both profiles** — the release test
run is what makes ADR-0013's `panic = "unwind"` amendment meaningful rather
than decorative. Contained, logged with its `request_id`, converted to
`STATUS_INTERNAL_ERROR`, poisons, and refuses subsequent callbacks without
touching state. Proven from a dispatcher thread, with a timing assertion, so
the ADR-0013a no-deadlock claim is measured rather than argued.

### The deadline

`a_hang_returns_within_the_callback_deadline` bounds it;
`the_bound_scales_with_the_configured_timeout` proves the bound is real rather
than an artefact of fast operations; `a_near_miss_delay_still_succeeds` proves
it does not fire early — which §13.4 rightly calls out as being as much a
defect as never firing.

---

## 3. Defects found and fixed

Recorded because the manual asks for the loop to be visible, and because two of
these were only findable through a real mount.

| # | Defect | How it was found | Fix |
|---|---|---|---|
| 1 | `UmFileContextIsUserContext2` unset, so WinFsp's `FileContext` was per **file**, not per **file object** | `Get-ChildItem` failed on every non-empty directory while `cmd dir` and `.NET GetFiles` worked | set the flag; documented in fs-semantics §1 as a Phase 2 obligation |
| 2 | Enumeration marker resumed by **membership**, so a marker deleted between calls stranded the enumeration | `Remove-Item -Recurse` deleted ~35 of 500 files then failed as non-empty | resume by **order** (`partition_point` over an order key); 2 regression tests in the conformance suite |
| 3 | `os-safety-check.ps1` grepped `lsvol` for `"SPACE"`, which fsptool never prints | noticed while reading real `lsvol` output | match the mount point; **verified the gate now fires** while mounted |
| 4 | Same script had no `exit 0`, so it inherited `fsptool`'s 433 and reported failure on a clean run | running it | explicit `exit 0`, with the reason at the call site |
| 5 | `cleanup`/`close` emitted no log line, hiding defect 2 | trying to trace a delete that left no trace | log the void entry points too |
| 6 | `INV-FS-1` was unfalsifiable (see §2) | writing the "checker must fire" tests | allocation became a stored property |
| 7 | Conformance `Ctx` leaked one handle per module | the L7 boundary test ran 9 handles short | fixed; the L7 test now asserts the **exact** count, so it doubles as a suite-wide leak detector |
| 8 | A non-directory mid-path reported two different codes depending on depth | conformance suite, first run | one rule: any parent-chain failure is `ObjectPathNotFound`; documented, since the manual does not name the case |

Defect 7 is worth noting twice: the leak detector caught a leak in the very
test that was added alongside it, which is the behaviour you want from a gate.

---

## 4. What is NOT done

Listed plainly. None of these are "nearly done".

### Requires a human (cannot be automated honestly)

| Item | Manual § | Why |
|---|---|---|
| Explorer's ten steps | §9.2 | GUI. Scripting a filesystem call and labelling it `Explorer` is an unearned PASS. |
| Explorer stays responsive under an injected hang | §13.2 pt 3 | a claim about the shell, observable only by a person |
| Notepad / 7-Zip compatibility rows | §16.3 | GUI clients |
| ProcMon write-confinement evidence | §16.4 | INV-NS-6's external half cannot be asserted from inside our own process |

All four are specified step by step in `EXPLORER-CHECKLIST.md`, including what
each step would expose if it failed.

### Not yet run

| Item | Manual § | Status |
|---|---|---|
| Fuzzing, ≥30 min × 6 targets | §14.1 | targets build and run; full budget in progress |
| Kill matrix, mid-enumeration state | §15.2 | idle and mid-write passed; third state running |
| 4-hour soak | §16.5 | harness written with thresholds fixed in advance; not run |
| 30-minute I/O stress | §16.2 | same harness; not run |
| Share-access layering | §10.1 | needs a two-process exclusive-open test |
| Four-column NTSTATUS matrix, Win32 column | §12.4 | first three columns asserted in-process; script written, not run |

### Known Phase 1 simplifications (deliberate, not defects)

Carried forward to Phase 2 exactly as the manual lists them: ASCII-only case
folding, a single global state lock, `pattern` ignored in enumeration,
`CanDelete` without `SetDelete`, and mutation-during-enumeration left undefined.

---

## 5. Deviations from the manual

Each is deliberate, and each is recorded at the point in the code where it
matters — not only here.

1. **`os-safety-check.ps1` differs from the listing in §1.5** in two ways: it
   matches the mount point rather than the string `SPACE` (fsptool prints the
   drive letter and device path, never the filesystem name, so the listing's
   check could never fail), and it exits 0 explicitly on success. Both are
   strengthenings; the first turned a gate that could not fail into one that
   does, verified by running it against a live mount.

2. **`VolumeParams.UmFileContextIsUserContext2 = 1`** is added to the §4.1
   volume parameters. Without it the §3.3.1 handle model is false. See
   fs-semantics §1.

3. **A non-directory in the middle of a path returns `ObjectPathNotFound`.**
   §3.3.3 does not name this case; it is decided and documented rather than
   left to the implementation.

4. **`Limits` and `PathLimits` are separate structs.** §11.5 fixes `Limits` at
   five fields because it is part of the conformance-suite signature Phase 2
   shares; L1–L3 therefore live in their own type rather than being bolted on.

5. **22 invariants, not 21.** The Phase 1 architecture review says 21; the
   enumerated §3.5 tables contain 22 rows. The tables are the normative text.

---

## 6. How to verify this independently

```powershell
cd C:\SPACE\src\space

cargo test --workspace                                  # 262 tests
cargo test -p space-client-core --release               # the ADR-0013 profile
cargo test -p space-client-core --features fault-injection
cargo clippy --workspace --all-targets                  # clean

cargo build -p space-client
.\target\debug\space-client.exe --config .\config.toml  # S: appears
.\scripts\mount-functional-test.ps1                     # 19/19
.\scripts\os-safety-check.ps1                           # after Ctrl-C
```

Everything above runs from a clean checkout on a machine with Rust, MSVC and
WinFsp. Nothing in it depends on state left behind by this session.
