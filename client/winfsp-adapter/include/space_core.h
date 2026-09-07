/* SPACE Phase 1 -- the C ABI between the WinFsp adapter and the Rust core.
 *
 * This header is the whole surface (manual section 2.2). The boundary rules
 * (section 2.1) that a caller must honour:
 *
 *   1. Representation. Only fixed-width scalars and pointers cross.
 *   2. Ownership.     Every buffer is caller-allocated and caller-owned. Rust
 *                     never returns memory that C++ must free, and never
 *                     retains a caller pointer past the call.
 *   3. Lifetime.      Pointer parameters are valid only for the duration of the
 *                     call. The ONE exception is space_dir_entry.name -- see
 *                     the note at that field.
 *   4. Nullability.   Every pointer parameter is non-null unless marked
 *                     optional. Null where non-null is required returns
 *                     STATUS_INVALID_PARAMETER; it never dereferences.
 *   5. Strings.       UTF-16, NUL-terminated. Rust scans with a hard length cap
 *                     (L1), validates UTF-16, and copies. Invalid encoding ->
 *                     STATUS_OBJECT_NAME_INVALID. Missing terminator ->
 *                     STATUS_NAME_TOO_LONG.
 *   6. Lengths.       All lengths are uint32_t and are validated against
 *                     max_io_bytes BEFORE any allocation or indexing.
 *   7. Layout.        Every shared struct's size and field offsets are asserted
 *                     on both sides -- see the static_asserts below and the
 *                     matching Rust tests in client/core/src/ffi/types.rs.
 *   8. Identifiers.   Opaque uint64_t, generational, never pointer-derived.
 *                     0 is never valid.
 *   9. Errors.        Every fallible entry point returns NTSTATUS. No
 *                     out-of-band channel. The void entry points cannot fail
 *                     by contract.
 *  10. Panics.        Contained by catch_unwind in every build profile
 *                     (ADR-0013). No unwind ever reaches a C++ frame.
 *  11. Deadlines.     Every entry point is bounded (ADR-0009), including lock
 *                     acquisition.
 */

#pragma once
#include <stdint.h>
#include <stddef.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint32_t space_status;     /* an NTSTATUS value */
typedef uint64_t space_handle;     /* opaque, generational; 0 is never valid */
typedef uint64_t space_cursor;     /* opaque, generational; 0 is never valid */
#define SPACE_INVALID_HANDLE ((space_handle)0)
#define SPACE_INVALID_CURSOR ((space_cursor)0)

typedef struct {
    uint32_t file_attributes;
    uint32_t reparse_tag;          /* always 0 in Phase 1 */
    uint64_t allocation_size;
    uint64_t file_size;
    uint64_t creation_time;        /* Windows FILETIME, 100ns since 1601 */
    uint64_t last_access_time;
    uint64_t last_write_time;
    uint64_t change_time;
    uint64_t index_number;         /* Windows-visible identity; see section 3.1 */
} space_file_info;                 /* 64 bytes; asserted on both sides */

typedef struct { uint64_t total_size; uint64_t free_size; } space_volume_info;

typedef struct {
    /* BORROWED: valid until the next space_core_dir_next on THIS cursor.
     * This is the one documented exception to ownership rule 2. Copy the name
     * out before calling dir_next again. */
    const wchar_t*  name;
    space_file_info info;
} space_dir_entry;

/* ---- layout assertions (boundary rule 7) ----
 * Layout drift otherwise surfaces as garbage file sizes and takes days to
 * trace. The Rust side asserts the identical numbers. */
#ifdef __cplusplus
static_assert(sizeof(space_file_info) == 64, "space_file_info layout drift");
static_assert(offsetof(space_file_info, file_size) == 16, "field offset drift");
static_assert(offsetof(space_file_info, file_attributes) == 0, "field offset drift");
static_assert(offsetof(space_file_info, reparse_tag) == 4, "field offset drift");
static_assert(offsetof(space_file_info, allocation_size) == 8, "field offset drift");
static_assert(offsetof(space_file_info, creation_time) == 24, "field offset drift");
static_assert(offsetof(space_file_info, last_access_time) == 32, "field offset drift");
static_assert(offsetof(space_file_info, last_write_time) == 40, "field offset drift");
static_assert(offsetof(space_file_info, change_time) == 48, "field offset drift");
static_assert(offsetof(space_file_info, index_number) == 56, "field offset drift");
static_assert(sizeof(space_volume_info) == 16, "space_volume_info layout drift");
#endif

/* ---- lifecycle ---- */
space_status space_core_start(const wchar_t* config_path);
space_status space_core_stop(void);

/* ---- volume ---- */
space_status space_core_get_volume_info(space_volume_info* out);

/* ---- open / close ---- */
space_status space_core_create(const wchar_t* path, uint32_t create_options,
                               uint32_t granted_access, uint32_t file_attributes,
                               uint64_t allocation_size,
                               space_handle* out_handle, space_file_info* out_info);
space_status space_core_open(const wchar_t* path, uint32_t create_options,
                             uint32_t granted_access,
                             space_handle* out_handle, space_file_info* out_info);
space_status space_core_overwrite(space_handle h, uint32_t file_attributes,
                                  uint8_t replace_attributes, uint64_t allocation_size,
                                  space_file_info* out_info);
void         space_core_cleanup(space_handle h, const wchar_t* path /*optional, may be NULL*/,
                                uint32_t flags);
void         space_core_close(space_handle h);

/* ---- io ---- */
space_status space_core_read(space_handle h, void* buffer, uint64_t offset,
                             uint32_t length, uint32_t* out_transferred);
space_status space_core_write(space_handle h, const void* buffer, uint64_t offset,
                              uint32_t length, uint8_t write_to_eof, uint8_t constrained_io,
                              uint32_t* out_transferred, space_file_info* out_info);
space_status space_core_flush(space_handle h /*0 = whole volume*/, space_file_info* out_info);

/* ---- info ---- */
space_status space_core_get_file_info(space_handle h, space_file_info* out_info);
space_status space_core_set_basic_info(space_handle h, uint32_t attrs,
                                       uint64_t ctime, uint64_t atime,
                                       uint64_t wtime, uint64_t chtime,
                                       space_file_info* out_info);
space_status space_core_set_file_size(space_handle h, uint64_t new_size,
                                      uint8_t set_allocation, space_file_info* out_info);
space_status space_core_get_security_by_name(const wchar_t* path,
                                             uint32_t* out_attrs /*optional, may be NULL*/,
                                             void* sd_buf /*optional, may be NULL*/,
                                             size_t* sd_size);

/* ---- namespace mutation ---- */
space_status space_core_can_delete(space_handle h, const wchar_t* path);
space_status space_core_rename(space_handle h, const wchar_t* path,
                               const wchar_t* new_path, uint8_t replace_if_exists);

/* ---- directory enumeration (cursor scoped to ONE ReadDirectory call) ---- */
space_status space_core_dir_open(space_handle h, const wchar_t* pattern /*may be NULL*/,
                                 const wchar_t* marker /*may be NULL*/, space_cursor* out_cursor);
space_status space_core_dir_next(space_cursor cursor, space_dir_entry* out_entry,
                                 uint8_t* out_has_more);
void         space_core_dir_close(space_cursor cursor);

/* ---- adapter entry points, implemented in C++ and called from Rust ---- */

/* Mount at MountPoint with DispatcherThreads dispatcher threads (ADR-0010).
 * 0 means "let WinFsp choose". */
space_status space_adapter_mount(const wchar_t* mount_point, uint32_t dispatcher_threads);

/* MUST be called only from the main thread -- see ADR-0013a.
 * FspFileSystemStopDispatcher waits for dispatcher threads to drain, including
 * the caller, so calling this from a dispatcher thread is a self-deadlock. */
void         space_adapter_unmount(void);

#ifdef __cplusplus
}
#endif
