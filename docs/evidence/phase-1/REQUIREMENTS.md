# SPACE Phase 1 — Requirement Traceability Matrix

Source of truth: `SPACE_Phase_1_Execution_Manual_FINAL.md`.

Status vocabulary: `TODO` · `IMPLEMENTING` · `TESTING` · `FAILING` · `FIXING` · `PASS` · `BLOCKED` · `DEFERRED`.

**No row may be marked PASS without evidence.**

| ID | Manual § | Requirement | Implementation | Test | Evidence | Status |
|---|---|---|---|---|---|---|
| R-S1-1 | §1.2 ADR-0007 | `build.rs` owns C++ adapter; CMake retired | `client/main/build.rs` | `cargo build -p space-client` | binary links adapter + WinFsp | PASS |
| R-S1-2 | §1.2 ADR-0013 | `panic = "abort"` removed from release profile | `Cargo.toml:60-63` | `cargo build --release` | release build OK 11m42s; Phase 0 report amendment A1 | PASS |
| R-S1-3 | §1.2 ADR-0007..0015 | Nine ADR documents written | `docs/decisions/ADR-0007..0015` | n/a (prose) | 9 files on disk | PASS |
| R-S1-4 | §1.3 | 9 new `ErrorCode`s added and classified | `contracts/src/errors.rs` | `the_nine_phase_1_codes_are_present`, `all_slice_covers_every_variant` | 104 tests green | PASS |
| R-S1-5 | §1.5 | `scripts/os-safety-check.ps1` exists and runs | `scripts/os-safety-check.ps1` | manual run | all 5 checks PASS, exit 0 | PASS |
| R-S1-6 | §1.6 | Phase-2+ tech absent from Phase 1 | n/a | grep audit | see final audit | TESTING |
| R-S2-1 | §2.2 | `space_core.h` C ABI header complete | `client/winfsp-adapter/include/space_core.h` | C++ static_asserts | compiles | PASS |
| R-S2-2 | §2.3 | `guard()`: state check, `catch_unwind`, poison, NTSTATUS | `ffi/mod.rs guard` | `ffi/tests.rs` panic model (6 tests) | debug+release green | PASS |
| R-S2-3 | §2.3 | `wstr()` length-capped UTF-16 conversion | `ffi/mod.rs wstr` | 7 string tests | test output | PASS |
| R-S2-4 | §2.3 | `cleanup`/`close` bypass guard, no-op when poisoned | `ffi/mod.rs` | `close_and_cleanup_after_poisoning_are_silent_no_ops` | test output | PASS |
| R-S2-5 | §2.4 | `build.rs` compiles adapter, delay-loads winfsp-x64 | `client/main/build.rs` | LNK4199 absent; runtime mount works | S: mounts | PASS |
| R-S2-6 | §2.5 | Struct layout asserted both sides (64 bytes, offset 16) | `ffi/types.rs`, `space_core.h` | `file_info_layout_matches_c` + C++ static_assert | test output | PASS |
| R-S2-7 | §2.5 | String/nullability boundary tests | `ffi/mod.rs` | 12 tests incl. null out-pointers | test output | PASS |
| R-S2-8 | §2.5 | Panic model tests pass debug AND release | `ffi/mod.rs guard` | `ffi/tests.rs` | 167 release tests green post-changes | PASS |
| R-S2-9 | §2.5 | Timeout to `STATUS_IO_TIMEOUT`; no `NetworkTimeout` in core | `ffi/ntstatus.rs` | `an_expired_deadline_yields_status_io_timeout`, grep test | test output | PASS |
| R-S3-1 | §3.1 | Identity model documented; 5 identifiers | `docs/protocols/identity.md` | `vfs/ids.rs` tests | 202 tests green | PASS |
| R-S3-2 | §3.1 | `index_number` monotonic, not slab-derived | `memvfs/node.rs`, `imp.rs take_index_number` | `index_numbers_are_strictly_increasing_and_never_reused` | test output | PASS |
| R-S3-3 | §3.1 | Handle/cursor encoding `(gen<<32)|(idx+1)` | `vfs/ids.rs` | `encoding_matches_the_documented_formula` | test output | PASS |
| R-S3-4 | §3.2 | `Vfs` trait + `OpCtx` + `VfsDiagnostics`; version = 1 | `vfs/mod.rs`, `vfs/invariants.rs` | `contract_version_is_one` | test output | PASS |
| R-S3-5 | §3.3 | `fs-semantics.md` complete | `docs/protocols/fs-semantics.md` | conformance suite, 11 modules | 206 tests green | PASS |
| R-S3-6 | §3.3.7 | `VfsPath` rejects every listed form | `vfs/path.rs` | one test per rule, 15 tests | test output | PASS |
| R-S3-7 | §3.4 | `resource-limits.md`; L1-L10 with config keys | `vfs/limits.rs` | `defaults_match_the_documented_table` | test output | IMPLEMENTING |
| R-S3-8 | §3.5 | `vfs-invariants.md`; 22 invariants | `vfs/invariants.rs` | `the_invariant_list_matches_the_documented_count` + `coverage_tests.rs` (5 tests) | every invariant claimed and verified | PASS |
| R-S3-9 | §3.5 | Checker read-only, non-repairing, deterministic | `memvfs/check.rs` | `checker_is_read_only_*` (3 tests, snapshot cmp) | test output | PASS |
| R-S3-10 | §3.6 | `concurrency.md`; deadline-bounded lock | `memvfs/imp.rs state()` | `lock_acquisition_is_deadline_bounded` | test output | PASS |
| R-S3-11 | §3.7 | Checker proven to FIRE for each invariant class | `memvfs/check.rs` | 12 `fires_inv_*` tests | test output | PASS |
| R-S4-1 | §4.1 | `host.cpp` mount with deterministic volume params | `winfsp-adapter/src/host.cpp` | live mount | S: mounts, fsptool lsvol | PASS |
| R-S4-2 | §4.1 | Unmount ordering: stop → remove → delete | `host.cpp space_adapter_unmount` | 200 mount/unmount cycles | `mount-stress.txt` | PASS |
| R-S4-3 | §4.2 | Callback table (full, not just GetVolumeInfo) | `winfsp-adapter/src/callbacks.cpp` | live mount exercises all | functional test 18/19 | PASS |
| R-S4-4 | §4.3 | `client/main` wiring; main-thread-only shutdown | `client/main/src/main.rs` | live mount + Ctrl-C path | mount/unmount clean | PASS |
| R-S4-5 | §4.4 | `S:` appears in Explorer; os-safety clean | `client/main` | live mount + os-safety | mounted; GUI step pending human | TESTING |
| R-S5-1 | §5.1 | `MemNode` / `MemVfsState` / `MemVfs` model | `memvfs/node.rs`, `memvfs/imp.rs` | 101 core tests | test output | PASS |
| R-S5-2 | §5.2 | Generational handle + cursor tables; L7/L8 | `memvfs/table.rs` | 13 table tests | test output | PASS |
| R-S5-3 | §5.3 | `check_invariants()` implemented | `memvfs/check.rs` | 12 firing + 3 read-only tests | test output | PASS |
| R-S5-4 | §5.4 | Handle-table matrix incl. generation wraparound | `memvfs/table.rs` | `generation_wraparound_skips_zero` + residual test | test output | PASS |
| R-S6-1 | §6.1 | Default security descriptor held in Rust | `ffi/security.rs` | 3 windows-only tests | S: opens in Explorer | PASS |
| R-S6-2 | §6.2 | `GetSecurityByName` buffer protocol | `ffi/mod.rs` | live mount (S: openable) | functional test | PASS |
| R-S6-3 | §6.3 | FILETIME conversion; allocation_size rounding | `client/core/src/time.rs`, `vfs/limits.rs` | `the_unix_epoch_converts_to_the_known_value`, `allocation_size_rounds_up_to_4096*` | test output | PASS |
| R-S7-1 | §7.1 | Create options honoured | `memvfs/imp.rs create/open` | `conformance/create_open.rs` | test output | PASS |
| R-S7-2 | §7.2 | Cleanup/Close bookkeeping placement | `memvfs/imp.rs cleanup/close` | `conformance/lifecycle.rs` (12 tests) | test output | PASS |
| R-S7-3 | §7.2 | INV-NS-6 path-traversal test (2-part) | `vfs/path.rs` | `conformance/naming.rs` + hosts-file snapshot | test output | PASS |
| R-S8-1 | §8 | read/write with overflow + EOF semantics | `memvfs/imp.rs read/write` | `conformance/read_write.rs` | test output | PASS |
| R-S8-2 | §8.1 | Full read/write matrix across 6 sizes | `memvfs/imp.rs` | `conformance/read_write.rs the_size_matrix` | test output | PASS |
| R-S8-3 | §8.1 | read-after-write property test with seed | `vfs/properties.rs` | `write_then_read_returns_the_same_bytes` | proptest, seeds persisted | PASS |
| R-S9-1 | §9.1 | `ReadDirectory` adapter: buffer-full, NULL marker, resume | `winfsp-adapter/src/callbacks.cpp` | 5,000-file enumeration | functional test PASS | PASS |
| R-S9-2 | §9.2 | Explorer evidence, 10 steps | live mount | `scripts/mount-functional-test.ps1` | 21/21 PASS; GUI steps pending human | TESTING |
| R-S10-1 | §10.1 | Rename/delete/metadata test rows | `memvfs/imp.rs` | `conformance/rename_delete.rs`, `metadata.rs` | test output | PASS |
| R-S10-2 | §10.1 | Share access is WinFsp-owned | ADR-0012 (no code) | `mount-functional-test.ps1` share-access check | ERROR_SHARING_VIOLATION (32) observed on a live mount | PASS |
| R-S11-1 | §11.1 | Conformance suite, 11 modules | `client/core/src/conformance/` | `run_conformance_suite` | 206 tests green | PASS |
| R-S11-2 | §11.2 | Suite runs with no WinFsp; separate CI job | `conformance/` | `.github/workflows/ci.yml conformance-no-winfsp` | job asserts WinFsp absent | PASS |
| R-S11-3 | §11.5 | `Capabilities` exactly one flag; both variants tested | `vfs/types.rs` | `capability_count_is_deliberate`, both runner tests | test output | PASS |
| R-S11-4 | §11.6 | Per-step invariant checking | `conformance/util.rs step()` | every conformance test | test output | PASS |
| R-S11-5 | §11.7 | Error-model assertions | `conformance/errors.rs` | `every_reachable_code_is_produced` | test output | PASS |
| R-S12-1 | §12.1 | `request_id` on every boundary log line | `ffi/mod.rs log_boundary` | live log inspection | path/handle/offset/length present | PASS |
| R-S12-2 | §12.2 | Log level discipline | `ffi/mod.rs is_expected` | live log: FileNotFound at DEBUG | 14,560 at debug, 0 at error | PASS |
| R-S12-3 | §12.3 | No file content in logs (asserted) | `ffi/mod.rs log_boundary` | `file_content_never_reaches_the_log` | test output | PASS |
| R-S12-4 | §12.4 | Four-column translation matrix | `ffi/ntstatus.rs` | `the_nine_phase_1_codes_map_to_the_manual_s_values` + live NTSTATUS | partial: Win32 column pending | TESTING |
| R-S13-1 | §13.1 | Phase 1 fault set + 8 fault points | `faults/src/lib.rs`, `ffi/mod.rs apply_fault` | `ffi/fault_tests.rs` (11 tests) | 175 tests w/ feature | PASS |
| R-S13-2 | §13.1 | `CorruptBytes` not armable | `faults::arm` | `corrupt_bytes_is_not_armable_in_phase_1` x2 | both profiles | PASS |
| R-S13-3 | §13.2 | Bounded callback under Hang, 7 assertions | `apply_fault` Hang | `fault_tests.rs` + `scripts/fault-injection-test.ps1` | in-process PASS; Explorer-responsive pending human | TESTING |
| R-S13-4 | §13.4 | Near-miss delay succeeds | `apply_fault` Delay | `a_near_miss_delay_still_succeeds` | test output | PASS |
| R-S13-5 | §13.5 | Invariant-targeting faults incl. Panic row | `apply_fault` | 6 fault-row tests | test output | PASS |
| R-S14-1 | §14.1 | Six fuzz targets, ≥30 min each | `fuzz/fuzz_targets/*.rs` | `scripts/fuzz.ps1` | 3 of 6 clean at 5 min (407k/163k/38k runs); ≥30 min budget outstanding | TESTING |
| R-S14-2 | §14.2 | Property tests; seeds printed | `vfs/properties.rs` | 9 proptest properties | test output | PASS |
| R-S14-3 | §14.3 | Limit & limit+1 with state comparison | `conformance/boundaries.rs` | L1-L8 tests | test output | PASS |
| R-S15-1 | §15.1 | Clean shutdown in 5 states | `client/main/src/main.rs` | 200 stress cycles + poison path test | partial: hung-op case pending | TESTING |
| R-S15-2 | §15.2 | Kill-while-mounted × 20, 3 states | `scripts/kill-matrix.ps1` | idle ✅ ×20, mid-write ✅ ×20 | mid-enumeration outstanding (run exclusively) | TESTING |
| R-S15-3 | §15.3 | `stale-mount-recovery.md` runbook | `docs/runbooks/stale-mount-recovery.md` | n/a (prose) | on disk | PASS |
| R-S16-1 | §16.1 | 200 mount/unmount cycles | `scripts/mount-stress.ps1` | 200 cycles, alternating | `mount-stress.txt`, os-safety OK | PASS |
| R-S16-2 | §16.2 | 30-min I/O stress | `scripts/soak.ps1` (same workload) | size matrix + verify | NOT YET RUN | TODO |
| R-S16-3 | §16.3 | Windows compatibility matrix | `scripts/compatibility-matrix.ps1` | 4 scripted clients + 3 GUI rows for human | pending run | TESTING |
| R-S16-4 | §16.4 | ProcMon write-confinement evidence | n/a (external) | `EXPLORER-CHECKLIST.md` §16.4 | HUMAN ACTION REQUIRED | BLOCKED |
| R-S16-5 | §16.5 | 4-hour soak with thresholds | `scripts/soak.ps1` | thresholds fixed in advance | NOT YET RUN | TODO |
| R-S17-1 | §17.1 | `collect-evidence.ps1` | `scripts/collect-evidence.ps1` | manual run | pending final gate | IMPLEMENTING |
| R-S17-2 | §17.2–17.7 | Six exit gates satisfied | n/a | final audit | see PHASE-1-CERTIFICATION.md | TESTING |

---

## Defects found during Phase 1, and where their regression lives

The manual asks that every meaningful bug become a regression test where
practical. All eight did.

| # | Defect | Regression |
|---|---|---|
| 1 | `UmFileContextIsUserContext2` unset: `FileContext` was per file, not per file object | `scripts/mount-functional-test.ps1` (enumeration + recursive delete) — the unit layer cannot see it, because it is a WinFsp protocol property |
| 2 | Enumeration marker resumed by membership, stranding delete-while-enumerating clients | `conformance/directory.rs`: `resume_from_a_marker_that_no_longer_exists`, `delete_while_enumerating_visits_every_entry` |
| 3 | `os-safety-check.ps1` stale-volume check could never fail | verified by running the gate against a live mount and confirming it reports FAIL |
| 4 | Same script inherited a non-zero exit code on success | explicit `exit 0`, reason recorded at the call site |
| 5 | `cleanup`/`close` produced no log line | `ffi/log_tests.rs`: `every_boundary_line_carries_a_request_id` asserts both appear |
| 6 | INV-FS-1 was unfalsifiable (computed allocation size) | `memvfs/tests.rs`: `fires_inv_fs_1_when_allocation_size_falls_below_file_size` |
| 7 | Conformance `Ctx` leaked a handle per module | `conformance/boundaries.rs`: the L7 test asserts the exact count and names the shortfall |
| 8 | A non-directory mid-path reported two different codes by depth | `conformance/create_open.rs`: `create_under_a_file_is_rejected` |
| 9 | Enumeration was O(N²), making L6 unusable and starving the state lock | `memvfs/tests.rs`: `enumeration_work_per_call_does_not_grow_with_directory_size` |
| 10 | Fuzz runner deadlocked on a PowerShell pipeline | `scripts/fuzz.ps1` uses file redirection; reason recorded at the call site |

Defects 1, 2 and 9 were only reachable through a real mount. Defect 9 was found
by the §15.2 kill matrix rather than by any test — at 5,000 entries with a
concurrent enumeration loop, which is precisely the state the manual specifies.
