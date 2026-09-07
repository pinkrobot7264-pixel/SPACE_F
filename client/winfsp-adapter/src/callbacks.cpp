/* The WinFsp callback table (manual sections 4.2, 9.1).
 *
 * Every callback is a thin translation: unpack WinFsp's arguments, call one
 * space_core_* entry point, pack the result. **The adapter performs no logging
 * and makes no decisions** -- it has nothing to say, because everything it
 * knows it got from the call it is about to make (section 12.1). The request_id
 * is minted at the first Rust frame, which is the honest origin of the trace.
 *
 * FileContext holds the space_handle directly. It is an opaque generational
 * u64, never a pointer (ADR-0008), so a stale or forged context fails
 * validation in Rust instead of dereferencing freed memory.
 */

#include <winfsp/winfsp.h>
#include "space_core.h"

/* WinFsp hands us PVOID FileContext; the handle is a u64. On x64 these are the
 * same width, which the static_assert pins rather than assumes. */
static_assert(sizeof(void *) == sizeof(space_handle),
              "FileContext must be wide enough to carry a space_handle");

static inline space_handle HandleOf(PVOID FileContext)
{
    return (space_handle)(uintptr_t)FileContext;
}

static inline PVOID ContextOf(space_handle h)
{
    return (PVOID)(uintptr_t)h;
}

static void CopyFileInfo(FSP_FSCTL_FILE_INFO *Dst, const space_file_info *Src)
{
    Dst->FileAttributes = Src->file_attributes;
    Dst->ReparseTag     = Src->reparse_tag;
    Dst->AllocationSize = Src->allocation_size;
    Dst->FileSize       = Src->file_size;
    Dst->CreationTime   = Src->creation_time;
    Dst->LastAccessTime = Src->last_access_time;
    Dst->LastWriteTime  = Src->last_write_time;
    Dst->ChangeTime     = Src->change_time;
    Dst->IndexNumber    = Src->index_number;
    Dst->HardLinks      = 0;
    Dst->EaSize         = 0;
}

/* ---- volume ---------------------------------------------------------- */

static NTSTATUS GetVolumeInfo(FSP_FILE_SYSTEM *, FSP_FSCTL_VOLUME_INFO *VolumeInfo)
{
    space_volume_info vi{};
    space_status s = space_core_get_volume_info(&vi);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    VolumeInfo->TotalSize = vi.total_size;
    VolumeInfo->FreeSize  = vi.free_size;
    VolumeInfo->VolumeLabelLength = 0;
    return STATUS_SUCCESS;
}

/* ---- security -------------------------------------------------------- */

/* The callback with the unusual contract (section 6.2). Three parts are
 * routinely got wrong, and getting the third wrong produces an S: that Explorer
 * can see but not open, with no useful error anywhere:
 *
 *   1. File does not exist -> STATUS_OBJECT_NAME_NOT_FOUND. Normal, not an
 *      error worth logging at error level. Create depends on it.
 *   2. PFileAttributes may be NULL. Check before writing.
 *   3. If SecurityDescriptor is NULL the caller wants only the size: write it
 *      and return success. If the buffer is too small, write the required size
 *      and return STATUS_BUFFER_OVERFLOW.
 *
 * All three live in Rust; this is the pass-through. */
static NTSTATUS GetSecurityByName(FSP_FILE_SYSTEM *, PWSTR FileName,
    PUINT32 PFileAttributes, PSECURITY_DESCRIPTOR SecurityDescriptor, SIZE_T *PSecurityDescriptorSize)
{
    return (NTSTATUS)space_core_get_security_by_name(
        FileName, (uint32_t *)PFileAttributes,
        (void *)SecurityDescriptor, (size_t *)PSecurityDescriptorSize);
}

/* ---- open / create / close ------------------------------------------- */

static NTSTATUS Create(FSP_FILE_SYSTEM *, PWSTR FileName, UINT32 CreateOptions,
    UINT32 GrantedAccess, UINT32 FileAttributes, PSECURITY_DESCRIPTOR /*SecurityDescriptor*/,
    UINT64 AllocationSize, PVOID *PFileContext, FSP_FSCTL_FILE_INFO *FileInfo)
{
    /* Phase 1 has one fixed security descriptor (section 6.1), so the one
     * supplied per-create is deliberately ignored. */
    space_handle h = SPACE_INVALID_HANDLE;
    space_file_info info{};
    space_status s = space_core_create(FileName, CreateOptions, GrantedAccess,
                                       FileAttributes, AllocationSize, &h, &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    *PFileContext = ContextOf(h);
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

static NTSTATUS Open(FSP_FILE_SYSTEM *, PWSTR FileName, UINT32 CreateOptions,
    UINT32 GrantedAccess, PVOID *PFileContext, FSP_FSCTL_FILE_INFO *FileInfo)
{
    space_handle h = SPACE_INVALID_HANDLE;
    space_file_info info{};
    space_status s = space_core_open(FileName, CreateOptions, GrantedAccess, &h, &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    *PFileContext = ContextOf(h);
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

static NTSTATUS Overwrite(FSP_FILE_SYSTEM *, PVOID FileContext, UINT32 FileAttributes,
    BOOLEAN ReplaceFileAttributes, UINT64 AllocationSize, FSP_FSCTL_FILE_INFO *FileInfo)
{
    space_file_info info{};
    space_status s = space_core_overwrite(HandleOf(FileContext), FileAttributes,
                                          ReplaceFileAttributes ? 1 : 0, AllocationSize, &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

/* Cleanup is per FILE OBJECT, and with PostCleanupWhenModifiedOnly = 0 it is
 * posted for every one. The VFS unlinks here when FspCleanupDelete is set --
 * this is the only signal Windows gives that a delete should take effect. */
static VOID Cleanup(FSP_FILE_SYSTEM *, PVOID FileContext, PWSTR FileName, ULONG Flags)
{
    space_core_cleanup(HandleOf(FileContext), FileName, (uint32_t)Flags);
}

/* Close is the only callback guaranteed to fire for every Create/Open, so it is
 * where open_count and the handle slot are released. */
static VOID Close(FSP_FILE_SYSTEM *, PVOID FileContext)
{
    space_core_close(HandleOf(FileContext));
}

/* ---- io -------------------------------------------------------------- */

static NTSTATUS Read(FSP_FILE_SYSTEM *, PVOID FileContext, PVOID Buffer,
    UINT64 Offset, ULONG Length, PULONG PBytesTransferred)
{
    uint32_t transferred = 0;
    space_status s = space_core_read(HandleOf(FileContext), Buffer, Offset,
                                     (uint32_t)Length, &transferred);
    *PBytesTransferred = transferred;
    return (NTSTATUS)s;
}

static NTSTATUS Write(FSP_FILE_SYSTEM *, PVOID FileContext, PVOID Buffer,
    UINT64 Offset, ULONG Length, BOOLEAN WriteToEndOfFile, BOOLEAN ConstrainedIo,
    PULONG PBytesTransferred, FSP_FSCTL_FILE_INFO *FileInfo)
{
    uint32_t transferred = 0;
    space_file_info info{};
    space_status s = space_core_write(HandleOf(FileContext), Buffer, Offset,
                                      (uint32_t)Length,
                                      WriteToEndOfFile ? 1 : 0,
                                      ConstrainedIo ? 1 : 0,
                                      &transferred, &info);
    *PBytesTransferred = transferred;
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

static NTSTATUS Flush(FSP_FILE_SYSTEM *, PVOID FileContext, FSP_FSCTL_FILE_INFO *FileInfo)
{
    space_file_info info{};
    /* A NULL FileContext means "flush the whole volume"; the core takes 0 for
     * that case. */
    space_status s = space_core_flush(HandleOf(FileContext), &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    if (nullptr != FileContext) CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

/* ---- info ------------------------------------------------------------ */

static NTSTATUS GetFileInfo(FSP_FILE_SYSTEM *, PVOID FileContext, FSP_FSCTL_FILE_INFO *FileInfo)
{
    space_file_info info{};
    space_status s = space_core_get_file_info(HandleOf(FileContext), &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

static NTSTATUS SetBasicInfo(FSP_FILE_SYSTEM *, PVOID FileContext, UINT32 FileAttributes,
    UINT64 CreationTime, UINT64 LastAccessTime, UINT64 LastWriteTime, UINT64 ChangeTime,
    FSP_FSCTL_FILE_INFO *FileInfo)
{
    space_file_info info{};
    space_status s = space_core_set_basic_info(HandleOf(FileContext), FileAttributes,
                                               CreationTime, LastAccessTime,
                                               LastWriteTime, ChangeTime, &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

static NTSTATUS SetFileSize(FSP_FILE_SYSTEM *, PVOID FileContext, UINT64 NewSize,
    BOOLEAN SetAllocationSize, FSP_FSCTL_FILE_INFO *FileInfo)
{
    space_file_info info{};
    space_status s = space_core_set_file_size(HandleOf(FileContext), NewSize,
                                              SetAllocationSize ? 1 : 0, &info);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;
    CopyFileInfo(FileInfo, &info);
    return STATUS_SUCCESS;
}

/* ---- namespace mutation ---------------------------------------------- */

/* Phase 1 implements CanDelete only, not SetDelete -- a pure query is the
 * simplest thing to reason about. Recorded in fs-semantics section 1 so Phase 2
 * knows it is a choice, not an oversight. */
static NTSTATUS CanDelete(FSP_FILE_SYSTEM *, PVOID FileContext, PWSTR FileName)
{
    return (NTSTATUS)space_core_can_delete(HandleOf(FileContext), FileName);
}

static NTSTATUS Rename(FSP_FILE_SYSTEM *, PVOID FileContext, PWSTR FileName,
    PWSTR NewFileName, BOOLEAN ReplaceIfExists)
{
    return (NTSTATUS)space_core_rename(HandleOf(FileContext), FileName, NewFileName,
                                       ReplaceIfExists ? 1 : 0);
}

/* ---- directory enumeration (section 9.1) ------------------------------ */

static NTSTATUS ReadDirectory(FSP_FILE_SYSTEM *FileSystem, PVOID FileContext,
    PWSTR Pattern, PWSTR Marker, PVOID Buffer, ULONG Length, PULONG PBytesTransferred)
{
    (void)FileSystem;

    space_cursor cursor = SPACE_INVALID_CURSOR;
    space_status s = space_core_dir_open(HandleOf(FileContext), Pattern, Marker, &cursor);
    if (s != STATUS_SUCCESS) return (NTSTATUS)s;

    union {
        FSP_FSCTL_DIR_INFO D;
        UINT8 B[sizeof(FSP_FSCTL_DIR_INFO) + MAX_PATH * sizeof(WCHAR)];
    } Entry;
    space_dir_entry e{};
    uint8_t more = 1;
    NTSTATUS result = STATUS_SUCCESS;

    while (more) {
        s = space_core_dir_next(cursor, &e, &more);
        if (s != STATUS_SUCCESS) { result = (NTSTATUS)s; goto done; }
        if (!more) break;

        size_t nameLen = wcslen(e.name);
        if (nameLen > MAX_PATH) nameLen = MAX_PATH;   /* the union bounds the buffer */

        memset(&Entry.D, 0, sizeof Entry.D);
        Entry.D.Size = (UINT16)(sizeof(FSP_FSCTL_DIR_INFO) + nameLen * sizeof(WCHAR));
        CopyFileInfo(&Entry.D.FileInfo, &e.info);
        memcpy(Entry.D.FileNameBuf, e.name, nameLen * sizeof(WCHAR));

        /* Buffer full: stop and report SUCCESS. WinFsp will call again with
         * Marker set to the last name we returned. Returning an error here
         * silently truncates large directories -- correct at 50 entries and
         * broken at 5,000 is the classic symptom. */
        if (!FspFileSystemAddDirInfo(&Entry.D, Buffer, Length, PBytesTransferred))
            goto done;
    }

    /* The end-of-enumeration marker is MANDATORY, and is only sent when the
     * enumeration truly reached the end. Without it Explorer shows a directory
     * that never finishes loading. */
    FspFileSystemAddDirInfo(nullptr, Buffer, Length, PBytesTransferred);

done:
    /* A cursor is valid for one ReadDirectory call only (fs-semantics
     * section 8): opened, drained or abandoned, and closed within this call.
     * Resumption across calls is carried by Marker, never by a cursor. */
    space_core_dir_close(cursor);
    return result;
}

/* Deliberately NOT used: WinFsp's directory-buffer helpers
 * (FspFileSystemAcquireDirectoryBuffer / FillDirectoryBuffer /
 * ReadDirectoryBuffer). They snapshot the whole directory into adapter-owned
 * state, which would push per-file-object state into C++ and violate the
 * thin-adapter rule. Marker-based resume keeps all state in the VFS. Recorded
 * as a choice, not an omission. */

/* ---- the table ------------------------------------------------------- */

FSP_FILE_SYSTEM_INTERFACE g_SpaceInterface =
{
    /* GetVolumeInfo        */ GetVolumeInfo,
    /* SetVolumeLabel       */ nullptr,
    /* GetSecurityByName    */ GetSecurityByName,
    /* Create               */ Create,
    /* Open                 */ Open,
    /* Overwrite            */ Overwrite,
    /* Cleanup              */ Cleanup,
    /* Close                */ Close,
    /* Read                 */ Read,
    /* Write                */ Write,
    /* Flush                */ Flush,
    /* GetFileInfo          */ GetFileInfo,
    /* SetBasicInfo         */ SetBasicInfo,
    /* SetFileSize          */ SetFileSize,
    /* CanDelete            */ CanDelete,
    /* Rename               */ Rename,
    /* GetSecurity          */ nullptr,
    /* SetSecurity          */ nullptr,
    /* ReadDirectory        */ ReadDirectory,
    /* ResolveReparsePoints */ nullptr,
    /* GetReparsePoint      */ nullptr,
    /* SetReparsePoint      */ nullptr,
    /* DeleteReparsePoint   */ nullptr,
    /* GetStreamInfo        */ nullptr,
    /* GetDirInfoByName     */ nullptr,
    /* Control              */ nullptr,
    /* SetDelete            */ nullptr,
};
