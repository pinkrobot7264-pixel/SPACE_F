# Open issue — `paths.log_dir` ignored, and the directory sink loses lines at exit

**Status: OPEN.** Two related defects, recorded together because neither can be
fixed alone. Both are contract defects independent of performance: a documented,
validated config key that does nothing is a defect whether or not anyone
notices.

A third defect in the same area — `logging.level` being ignored — **was** fixed
(commit `0f0658c`) and is not part of this issue.

## Defect 1 — `paths.log_dir` is never consulted

`config.toml` declares `paths.log_dir`, the loader validates it, and
`config.example.toml` documents it. The client never reads it. The sink is
chosen in `client/core/src/startup.rs::run` from the `SPACE_CLIENT_LOG_DIR`
environment variable alone:

```rust
let log_dir = std::env::var("SPACE_CLIENT_LOG_DIR").ok().map(PathBuf::from);
let sink = match &log_dir { Some(d) => LogSink::Directory(d), None => LogSink::Stderr };
```

With that variable unset — which is how every harness runs — the sink is
`LogSink::Stderr`.

## Defect 2 — the directory sink drops buffered lines at process exit

`LogSink::Directory` wraps the file appender in `tracing_appender::non_blocking`,
whose `WorkerGuard` must be dropped to drain the queue. The guard is parked in

```rust
static GUARD: OnceLock<Option<tracing_appender::non_blocking::WorkerGuard>>
```

and Rust does not drop statics at process exit, so the worker is never drained.
The tail of every run is lost.

**Measured**, same client, same config, only the sink differing:

| shutdown | mount released | bytes written to sink | `clean shutdown` line present |
|---|---|---|---|
| graceful (Ctrl-C) | yes | 1,250 | **no** |
| forced kill | yes | 820 | no (correct — nothing runs after a Class B death) |

The graceful row is the defect. That line is emitted at INFO on the ADR-0013a
teardown path and is present when the sink is stderr.

## Why defect 1 is not fixed on its own

Honouring `paths.log_dir` was implemented, measured, and reverted. Routing the
default sink to the configured directory makes every run lose its tail, and
`scripts/mount-stress.ps1` proves the graceful path *by* finding
`clean shutdown` in the client's log. Shipping defect 1's fix alone would have:

- turned all 100 graceful cycles of §16.1 into false failures, and
- moved certification onto a sink that silently truncates every run.

The second consequence is the serious one. A test harness that reads a lossy log
cannot distinguish "the filesystem did not do it" from "the log did not record
it", which is the same class of problem as the fake-graceful harness this
project has already been bitten by.

## What a correct fix looks like

Both together:

1. Give `logging` a `flush()` that takes the guard and drops it — the guard
   needs to move from `OnceLock` to something that permits taking, e.g.
   `Mutex<Option<WorkerGuard>>`.
2. Call it on the clean-shutdown path in `client/main`, after the last log line
   and before the process returns.
3. Then consult `cfg.paths.log_dir` for the default sink, keeping
   `SPACE_CLIENT_LOG_DIR` as an override.
4. Re-run §16.1 and §15.1, whose evidence depends on reading the client log.

## Impact on Phase 1 certification

None of the Phase 1 acceptance criteria depend on `paths.log_dir`. The stderr
sink is synchronous and does not lose the tail of a run, so all current evidence
was collected on the lossless path. §12.1 and §12.2 evidence — `request_id` on
every boundary line, level discipline — is collected at debug level over stderr
and is unaffected.

The reason this is recorded rather than fixed now: it is a change to the logging
path that forces re-running the two harnesses that read client logs, late in
certification, for no gain against any manual requirement. It should be fixed as
a pair, deliberately, with those re-runs budgeted.
