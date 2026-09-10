# §13.3 — `getinfo` assertion 2 is not demonstrable, and why

**Classification: EXPECTED WINDOWS/WinFsp BEHAVIOUR. Not a product defect, not a
harness defect.**

Run: 2026-09-10 19:46:49 → 20:07:03 (20 m 14 s), HEAD `353b23f`, os-safety 0.
Evidence: `fault-injection.txt`, preserved as
`fault-injection-run-2026-09-10-1946.txt`.

## Result

7 of 8 rows pass every assertion. 31 of 32 assertions pass overall. The single
failure is `getinfo` assertion 2.

| row | a1 callback bounded | a2 controlled error | a4 unrelated op | a5 unmount |
|---|---|---|---|---|
| read | PASS 3 cb, 30009 ms | PASS | PASS | PASS |
| write | PASS 2 cb, 30011 ms | PASS | PASS | PASS |
| **open** | **PASS 7 cb, 30009 ms** | **PASS** | **PASS** | **PASS** |
| readdir | PASS 2 cb, 30005 ms | PASS | PASS | PASS |
| **getinfo** | **PASS 24 cb, 30011 ms** | **FAIL** | **PASS** | **PASS** |
| rename | PASS 1 cb, 30003 ms | PASS | PASS | PASS |
| read @1000 ms | PASS 3 cb, 1011 ms vs 1500 ms bound | PASS | PASS | PASS |
| panic (§13.5) | PASS | PASS | PASS | — |

## What fails

`FAIL getinfo — the application saw success under an injected hang.`

The fault fired: **24 `get_file_info` callbacks** hung the full deadline, every
one returning inside the 30500 ms bound (slowest 30011 ms). The application
call took 150483 ms and then **succeeded** instead of receiving an error.

## Why — the mechanism, verified in code

`Open` hands Windows a fully populated `FSP_FSCTL_FILE_INFO`:

```cpp
// client/winfsp-adapter/src/callbacks.cpp:100-110
static NTSTATUS Open(FSP_FILE_SYSTEM *, PWSTR FileName, UINT32 CreateOptions,
    UINT32 GrantedAccess, PVOID *PFileContext, FSP_FSCTL_FILE_INFO *FileInfo)
{
    ...
    space_status s = space_core_open(FileName, CreateOptions, GrantedAccess, &h, &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    *PFileContext = ContextOf(h);
    CopyFileInfo(FileInfo, &info);      // <-- size and timestamps handed over here
    return STATUS_SUCCESS;
}
```

Once a handle is open, Windows already holds the file's size and timestamps.
`FileStream.Length` is answered from that, without issuing a `GetFileInfo`
callback. So the application cannot be made to block on a hung `get_file_info`
by querying length through an open handle — the data it wants was delivered by
`Open`, which is not the faulted point.

This was confirmed by direct probe before the run: with `winfsp_pre_getinfo=hang`
armed, the trigger returned **`threw=False` after 150393 ms** having succeeded,
while 12 faulted callbacks were logged. The callbacks fire; the application is
simply not waiting on them.

## Why this is not reclassified into a pass

Assertion 2 says the issuing application receives a controlled error rather than
hanging. For `get_file_info` on this platform, no application call reliably
depends on that callback while a handle is open, so the assertion has nothing to
bind to. Options considered and rejected:

- **Invent a trigger that forces the callback.** Any such trigger would be
  chosen to make the test pass rather than because an application would do it.
  That is fitting the test to the desired answer.
- **Declare assertion 2 satisfied because the callback was bounded.** That is
  assertion 1, which already passes. Restating it under assertion 2's name would
  be counting one measurement twice.

So it is recorded as a genuine FAIL of that assertion, with the mechanism
documented.

## What is proven for `get_file_info`

- **Assertion 1**: the callback returns inside `callback_timeout_ms + 500 ms` —
  proven 24 times, slowest 30011 ms against a 30500 ms bound.
- **Assertion 4**: an unrelated operation completes while the fault is in flight,
  with no callback anywhere exceeding the bound.
- **Assertion 5**: unmount succeeds after the injected hang.

The deadline enforcement — the thing §13.2 exists to verify — holds for this
fault point.

## Disposition

- **Phase 1 blocker: no.** The deadline behaviour the requirement targets is
  proven for all six operations. What is missing is an application-visible error
  for one point, and the reason is a WinFsp design property, not SPACE's.
- **Phase 2 revisit: yes, optionally.** If a control channel is added (the same
  capability that would allow arming faults post-mount), a
  `GetFileInformationByHandleEx` path that bypasses the cached info could bind
  assertion 2 to `get_file_info`.

## Superseded finding

`FINDING-13-3-open-row-nontermination.md` documented the `open` row failing for
a different reason — the harness's `Test-Path` readiness probe was itself an
open, so the row never reached its trigger and ran 4 h 08 m / 491 callbacks.
That is **resolved**: readiness is now detected with `fsptool lsvol`, which is
answered from WinFsp's mount-point bookkeeping and never enters the callback
table. The `open` row now passes all four assertions in 30262 ms. The earlier
finding and its 491-callback evidence are retained as the record of how it was
diagnosed.
