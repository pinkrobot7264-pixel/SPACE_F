# SPACE domain schemas and invariants (M0.7)

The types are `contracts::model`; the checks are `contracts::validate`, one
function per invariant, named with its number. **No invariant lives only in
code** -- if you read only this document you know every rule a manifest obeys.

## Objects

### `File`
`file_id: FileId` · `parent_id: DirectoryId` · `name: String` ·
`current_version_id: Option<VersionId>` · `created_at` · `modified_at`.
`current_version_id` is `None` until the first commit and thereafter references
a `Committed` version of this file (invariant 11).

### `Version`
`version_id: VersionId` · `file_id: FileId` ·
`parent_version_id: Option<VersionId>` · `manifest_id: ManifestId` ·
`state: VersionState` · `created_at`.
State is `Candidate` or `Committed`; see ADR-0006 and invariant 10.

### `Chunk`
`chunk_id: ChunkId` · `size: u64`. The id **is** the BLAKE3 content address
(ADR-0002); there is no separate hash field.

### `ManifestEntry`
`logical_offset: u64` · `length: u64` · `chunk_id: ChunkId` ·
`chunk_offset: u64`. One contiguous logical byte range served from a slice of
one chunk.

### `Manifest`
`manifest_id: ManifestId` · `total_size: u64` · `chunk_count: u64` ·
`entries: Vec<ManifestEntry>` · `chunks: Vec<Chunk>` (metadata for every
*distinct* chunk referenced). Within-file dedup is legal: two entries may name
the same chunk.

## Invariants

`validate_manifest` runs the structural ones (1-9, 13). 10-12 are cross-object.

| # | Name | Rule |
|---|---|---|
| 1 | `inv01_entries_sorted` | entries are ordered by `logical_offset` |
| 2 | `inv02_entries_contiguous` | no gap between an entry's end and the next entry's start |
| 3 | `inv03_no_overlap` | an entry's end never passes the next entry's start |
| 4 | `inv04_starts_at_zero` | a non-empty file's first entry starts at offset 0 |
| 5 | `inv05_lengths_sum_to_total` | `sum(entry.length) == total_size`, computed in checked arithmetic (a hostile manifest can overflow `u64`) |
| 6 | `inv06_count_matches` | `chunk_count == entries.len()` |
| 7 | `inv07_lengths_nonzero` | every entry has `length > 0` |
| 8 | `inv08_chunk_offsets_within_chunk` | `chunk_offset + length <= chunk.size` for the referenced chunk, checked arithmetic |
| 9 | `inv09_chunk_ids_wellformed` | every `chunk_id` parses as `b3:<64 hex>`; every entry's chunk has metadata in `chunks`; no unreferenced chunk metadata |
| 10 | `validate_version_transition` | the only legal `VersionState` move is `Candidate -> Committed` |
| 11 | `validate_file_current_version` | `File.current_version_id`, if set, references a `Committed` version of the same file |
| 12 | `validate_version_parent` | `Version.parent_version_id`, if set, belongs to the same `file_id` |
| 13 | `inv13_empty_file_is_valid` | `total_size == 0` **is valid** and requires zero entries and `chunk_count == 0` |

Every invariant has a one-pass / one-fail test in
`contracts::validate::tests`. Adding an invariant means: a numbered function
here, a row in this table, and both tests.
