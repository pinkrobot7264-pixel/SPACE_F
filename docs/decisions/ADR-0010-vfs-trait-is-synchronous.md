# ADR-0010 — The `Vfs` trait is synchronous; dispatcher threads bound concurrency

- **Status:** Accepted
- **Phase:** 1
- **Manual:** Phase 1 §1.2, §3.2, §3.6

## Context

Phase 2 replaces `MemVfs` with a real VFS; Phase 4 puts a network behind it. An
`async` trait would look more future-proof, at the cost of `async_trait`
boxing, a runtime dependency in the core, and an interface that WinFsp's
synchronous callback model cannot use without blocking anyway.

## Decision

**The `Vfs` trait is synchronous.** A future network-backed implementation
blocks on a runtime internally, so the interface survives Phase 4 unchanged.

The consequence must not stay implicit: **the WinFsp dispatcher thread count is
the concurrency ceiling for every backing store.** It is therefore a configured
bound, not a default:

```toml
[client]
dispatcher_threads = 4     # 0 = WinFsp default; explicit value caps concurrency
```

## Consequences

- L10 ("concurrent callbacks") is `config.client.dispatcher_threads`, enforced
  by WinFsp itself, which queues beyond it.
- Cursor limit L8 can be small (256) because concurrent enumerations are bounded
  by dispatcher threads, not by client behaviour.
- Phase 4 must size `dispatcher_threads` against network latency, and that is a
  configuration decision with a name, not an accident of WinFsp defaults.

## Enforcement

`space_adapter_mount(MountPoint, DispatcherThreads)` takes the value from
config; `client/main` passes `cfg.client.dispatcher_threads`.
