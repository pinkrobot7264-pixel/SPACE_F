# SPACE Phase 1 — Requirement Traceability Matrix

Source of truth: `SPACE_Phase_1_Execution_Manual_FINAL.md`.

Status vocabulary: `TODO` · `IMPLEMENTING` · `TESTING` · `FAILING` · `FIXING` · `PASS` · `BLOCKED` · `DEFERRED`.

**No row may be marked PASS without evidence.**

| ID | Manual § | Requirement | Implementation | Test | Evidence | Status |
|---|---|---|---|---|---|---|
| R-S1-1 | §1.2 ADR-0007 | `build.rs` owns C++ adapter; CMake retired off build path | | | | TODO |
| R-S1-2 | §1.2 ADR-0013 | `panic = "abort"` removed from release profile | `Cargo.toml:60-63` | `cargo build --release` | release build OK 11m42s; Phase 0 report amendment A1 | PASS |
| R-S1-3 | §1.2 ADR-0007..0015 | Nine ADR documents written | `docs/decisions/ADR-0007..0015` | n/a (prose) | 9 files on disk | PASS |
| R-S1-4 | §1.3 | 9 new `ErrorCode`s added and classified | `contracts/src/errors.rs` | `the_nine_phase_1_codes_are_present`, `all_slice_covers_every_variant` | 104 tests green | PASS |
| R-S1-5 | §1.5 | `scripts/os-safety-check.ps1` exists and runs | `scripts/os-safety-check.ps1` | manual run | all 5 checks PASS, exit 0 | PASS |
| R-S1-6 | §1.6 | Phase-2+ tech absent from Phase 1 | | | | TODO |
| R-S2-1 | §2.2 | `space_core.h` C ABI header complete | | | | TODO |
| R-S2-2 | §2.3 | `guard()`: state check, `catch_unwind`, poison, NTSTATUS | | | | TODO |
| R-S2-3 | §2.3 | `wstr()` length-capped UTF-16 conversion | | | | TODO |
| R-S2-4 | §2.3 | `cleanup`/`close` bypass guard, no-op when poisoned | | | | TODO |
| R-S2-5 | §2.4 | `build.rs` compiles adapter, delay-loads winfsp-x64 | | | | TODO |
| R-S2-6 | §2.5 | Struct layout asserted both sides (64 bytes, offset 16) | | | | TODO |
| R-S2-7 | §2.5 | String/nullability boundary tests | | | | TODO |
| R-S2-8 | §2.5 | Panic model tests pass debug AND release | | | | TODO |
| R-S2-9 | §2.5 | Timeout → `STATUS_IO_TIMEOUT`; no `NetworkTimeout` in core | | | | TODO |
| R-S3-1 | §3.1 | Identity model documented; 5 identifiers | | | | TODO |
| R-S3-2 | §3.1 | `index_number` monotonic, not slab-derived | | | | TODO |
| R-S3-3 | §3.1 | Handle/cursor encoding `(gen<<32)\|(idx+1)` | | | | TODO |
| R-S3-4 | §3.2 | `Vfs` trait + `OpCtx` + `VfsDiagnostics`; version = 1 | | | | TODO |
| R-S3-5 | §3.3 | `fs-semantics.md` complete | | | | TODO |
| R-S3-6 | §3.3.7 | `VfsPath` rejects every listed form | | | | TODO |
| R-S3-7 | §3.4 | `resource-limits.md`; L1–L10 with config keys | | | | TODO |
| R-S3-8 | §3.5 | `vfs-invariants.md`; 21 invariants | | | | TODO |
| R-S3-9 | §3.5 | Checker read-only, non-repairing, deterministic | | | | TODO |
| R-S3-10 | §3.6 | `concurrency.md`; deadline-bounded lock acquisition | | | | TODO |
| R-S3-11 | §3.7 | Checker proven to FIRE for each invariant class | | | | TODO |
| R-S4-1 | §4.1 | `host.cpp` mount with deterministic volume params | | | | TODO |
| R-S4-2 | §4.1 | Unmount ordering: stop → remove → delete | | | | TODO |
| R-S4-3 | §4.2 | Minimal callback table (`GetVolumeInfo`) | | | | TODO |
| R-S4-4 | §4.3 | `client/main` wiring; main-thread-only shutdown | | | | TODO |
| R-S4-5 | §4.4 | `S:` appears in Explorer; os-safety clean | | | | TODO |
| R-S5-1 | §5.1 | `MemNode` / `MemVfsState` / `MemVfs` model | | | | TODO |
| R-S5-2 | §5.2 | Generational handle + cursor tables; L7/L8 | | | | TODO |
| R-S5-3 | §5.3 | `check_invariants()` implemented | | | | TODO |
| R-S5-4 | §5.4 | Handle-table test matrix incl. generation wraparound | | | | TODO |
| R-S6-1 | §6.1 | Default security descriptor held in Rust | | | | TODO |
| R-S6-2 | §6.2 | `GetSecurityByName` buffer protocol | | | | TODO |
| R-S6-3 | §6.3 | FILETIME conversion; allocation_size rounding | | | | TODO |
| R-S7-1 | §7.1 | Create options honoured | | | | TODO |
| R-S7-2 | §7.2 | Cleanup/Close bookkeeping placement | | | | TODO |
| R-S7-3 | §7.2 | INV-NS-6 path-traversal test (2-part) | | | | TODO |
| R-S8-1 | §8 | read/write with overflow + EOF semantics | | | | TODO |
| R-S8-2 | §8.1 | Full read/write matrix across 6 sizes | | | | TODO |
| R-S8-3 | §8.1 | read-after-write property test with seed | | | | TODO |
| R-S9-1 | §9.1 | `ReadDirectory` adapter: buffer-full, NULL marker, resume | | | | TODO |
| R-S9-2 | §9.2 | Explorer evidence, 10 steps | | | | TODO |
| R-S10-1 | §10.1 | Rename/delete/metadata test rows | | | | TODO |
| R-S10-2 | §10.1 | Share access is WinFsp-owned | | | | TODO |
| R-S11-1 | §11.1 | Conformance suite, 11 modules | | | | TODO |
| R-S11-2 | §11.2 | Suite runs with no WinFsp; separate CI job | | | | TODO |
| R-S11-3 | §11.5 | `Capabilities` exactly one flag; both variants tested | | | | TODO |
| R-S11-4 | §11.6 | Per-step invariant checking | | | | TODO |
| R-S11-5 | §11.7 | Error-model assertions | | | | TODO |
| R-S12-1 | §12.1 | `request_id` on every boundary log line | | | | TODO |
| R-S12-2 | §12.2 | Log level discipline | | | | TODO |
| R-S12-3 | §12.3 | No file content in logs (asserted) | | | | TODO |
| R-S12-4 | §12.4 | Four-column translation matrix | | | | TODO |
| R-S13-1 | §13.1 | Phase 1 fault set + 8 fault points | | | | TODO |
| R-S13-2 | §13.1 | `CorruptBytes` not armable | | | | TODO |
| R-S13-3 | §13.2 | Bounded callback under Hang, 7 assertions | | | | TODO |
| R-S13-4 | §13.4 | Near-miss delay succeeds | | | | TODO |
| R-S13-5 | §13.5 | Invariant-targeting faults incl. Panic row | | | | TODO |
| R-S14-1 | §14.1 | Six fuzz targets, ≥30 min each | | | | TODO |
| R-S14-2 | §14.2 | Property tests; seeds printed | | | | TODO |
| R-S14-3 | §14.3 | Limit & limit+1 with state comparison | | | | TODO |
| R-S15-1 | §15.1 | Clean shutdown in 5 states | | | | TODO |
| R-S15-2 | §15.2 | Kill-while-mounted × 20, 3 states | | | | TODO |
| R-S15-3 | §15.3 | `stale-mount-recovery.md` runbook | | | | TODO |
| R-S16-1 | §16.1 | 200 mount/unmount cycles | | | | TODO |
| R-S16-2 | §16.2 | 30-min I/O stress | | | | TODO |
| R-S16-3 | §16.3 | Windows compatibility matrix | | | | TODO |
| R-S16-4 | §16.4 | ProcMon write-confinement evidence | | | | TODO |
| R-S16-5 | §16.5 | 4-hour soak with thresholds | | | | TODO |
| R-S17-1 | §17.1 | `collect-evidence.ps1` | | | | TODO |
| R-S17-2 | §17.2–17.7 | Six exit gates satisfied | | | | TODO |
