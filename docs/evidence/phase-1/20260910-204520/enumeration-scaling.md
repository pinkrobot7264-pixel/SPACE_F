# Enumeration scaling evidence

Measured on a live `S:` mount, from the boundary log's `duration_ms` field.

## The defect

`dir_open` snapshotted every child -- cloning all N names and `FileInfo`s -- on
**every** `ReadDirectory` call. WinFsp issues roughly `N / 33` such calls, so a
full listing was O(N^2).

Because section 3.6 serialises everything behind a single state lock, a large
listing did not merely take a long time itself: it **starved every other
operation** for its duration.

Observed before the fix:

| Symptom | Measurement |
|---|---|
| `dir_open` latency, 5,000-entry directory | up to 13 ms, growing with N |
| `Get-ChildItem` on a 5,000-entry root | did not return within 180 s |
| Concurrent file creation during a listing | fell below 2 files/sec |
| One kill-matrix iteration | 53 minutes without completing |

## After the fix

The cursor holds a bounded window (1024 entries) and refills as `dir_next`
drains, seeking with `BTreeMap::range` (O(log N)).

| Directory size | `dir_open` calls | max `duration_ms` |
|---|---|---|
| ~22,000 entries (cumulative run) | 507 | **3 ms** |

Distribution across those 507 calls: 309 at 0 ms, 191 at 1 ms, 6 at 2 ms,
1 at 3 ms.

**The shape is the result**: cost per call is flat from 1,000 to 22,000 entries,
rather than growing with directory size. That is what makes L6's documented
65,536-entry limit usable.

## Why this was not caught by a unit test first

The conformance suite enumerates directories of 40-60 entries, where O(N^2) and
O(N) are indistinguishable. It surfaced under the section 15.2 kill matrix, at
5,000 entries with a concurrent enumeration loop -- which is exactly the
situation the manual specifies that state for.

A shape assertion now guards it in
`client/core/src/vfs/memvfs/tests.rs::enumeration_work_per_call_does_not_grow_with_directory_size`.
