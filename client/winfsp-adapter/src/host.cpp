/* The WinFsp host (manual section 4.1).
 *
 * This file owns mount and unmount and nothing else. Filesystem meaning lives
 * in Rust (ADR-0001, ADR-0007); the adapter is the Windows/WinFsp mechanics
 * only, and makes no decisions of its own.
 */

#include <winfsp/winfsp.h>
#include "space_core.h"

static FSP_FILE_SYSTEM *g_FileSystem = nullptr;
extern FSP_FILE_SYSTEM_INTERFACE g_SpaceInterface;

extern "C" space_status space_adapter_mount(const wchar_t *MountPoint, uint32_t DispatcherThreads)
{
    /* FspLoad locates winfsp-x64.dll through its registry entry. The DLL lives
     * in C:\Program Files (x86)\WinFsp\bin, not System32, so the binary is
     * delay-loaded (see build.rs) and this call is what resolves it. Without
     * the pair, the build is clean and the launch fails. */
    NTSTATUS Result = FspLoad(nullptr);
    if (!NT_SUCCESS(Result)) return (space_status)Result;

    FSP_FSCTL_VOLUME_PARAMS VolumeParams = {};
    VolumeParams.SectorSize                  = 4096;
    VolumeParams.SectorsPerAllocationUnit    = 1;
    VolumeParams.VolumeSerialNumber          = 0x53504143;   /* 'SPAC' */

    /* Note 1 -- no kernel-side caching of file information. Every query reaches
     * the VFS, so a test that writes and immediately stats sees the new size. A
     * non-zero timeout makes size and timestamp assertions intermittently
     * stale, and intermittent Phase 1 failures cost far more than the extra
     * round trips. */
    VolumeParams.FileInfoTimeout             = 0;

    VolumeParams.CaseSensitiveSearch         = 0;
    VolumeParams.CasePreservedNames          = 1;
    VolumeParams.UnicodeOnDisk               = 1;
    VolumeParams.PersistentAcls              = 1;

    /* Note 2 -- Cleanup is posted for EVERY file object. With 1, cleanup is
     * suppressed for unmodified opens, which makes it useless for per-open
     * bookkeeping and produces the "why didn't cleanup fire" confusion.
     * Determinism is worth more than the optimisation here. */
    VolumeParams.PostCleanupWhenModifiedOnly = 0;

    /* Note 3 -- the cache manager flushes and purges at cleanup, so data
     * written through one handle is visible to a subsequent open without
     * depending on cache timing. */
    VolumeParams.FlushAndPurgeOnCleanup      = 1;

    /* Note 4 -- REQUIRED by the handle model, and absent from the manual's
     * section 4.1 listing.
     *
     * fs-semantics section 1 states: "Each successful Create/Open corresponds
     * to one kernel FILE OBJECT, and to exactly one FileContext -- one
     * HandleId", and "Close is called exactly once per Create/Open".
     *
     * That is NOT WinFsp's default. With this flag clear, the user-mode
     * FileContext is the FSD's UserContext, which is per *file* (the FsContext
     * / FileNode), shared by every file object open on that file. Opening one
     * directory twice then produces: two Opens (two HandleIds from us), but
     * every subsequent callback carries only the FIRST context; the first
     * Cleanup/Close frees it while the second file object is still enumerating,
     * and that enumeration then fails with STATUS_INVALID_HANDLE.
     *
     * Observed exactly that way: PowerShell's Get-ChildItem opens the directory
     * twice and failed with "The handle is invalid" on every non-empty
     * directory, while cmd's `dir`, .NET's Directory.GetFiles and `ls` -- all of
     * which open once -- succeeded.
     *
     * Setting UserContext2 makes the FileContext per file object, which is what
     * the contract above describes and what makes one-HandleId-per-Open sound.
     * WinFsp's own memfs avoids the issue differently, by using a
     * reference-counted FileNode as the context; that model is incompatible with
     * ADR-0008's opaque generational handles, so this flag is the right fix. */
    VolumeParams.UmFileContextIsUserContext2 = 1;

    wcscpy_s(VolumeParams.FileSystemName,
             sizeof VolumeParams.FileSystemName / sizeof(WCHAR), L"SPACE");

    Result = FspFileSystemCreate(const_cast<PWSTR>(L"" FSP_FSCTL_DISK_DEVICE_NAME),
                                 &VolumeParams, &g_SpaceInterface, &g_FileSystem);
    if (!NT_SUCCESS(Result)) return (space_status)Result;

    /* Section 3.6, layer 1. Retained because it is the strategy Phase 10 will
     * tune, and changing it later should not be the first time it is
     * exercised. */
    FspFileSystemSetOperationGuardStrategy(
        g_FileSystem, FSP_FILE_SYSTEM_OPERATION_GUARD_STRATEGY_COARSE);

    Result = FspFileSystemSetMountPoint(g_FileSystem, const_cast<PWSTR>(MountPoint));
    if (!NT_SUCCESS(Result)) {
        FspFileSystemDelete(g_FileSystem);
        g_FileSystem = nullptr;
        return (space_status)Result;
    }

    Result = FspFileSystemStartDispatcher(g_FileSystem, DispatcherThreads);   /* ADR-0010 */
    if (!NT_SUCCESS(Result)) {
        FspFileSystemRemoveMountPoint(g_FileSystem);
        FspFileSystemDelete(g_FileSystem);
        g_FileSystem = nullptr;
        return (space_status)Result;
    }

    return (space_status)STATUS_SUCCESS;
}

/* MUST be called only from the main thread -- see ADR-0013a.
 *
 * FspFileSystemStopDispatcher waits for dispatcher threads to drain, INCLUDING
 * the caller, so calling this from a dispatcher thread is a self-deadlock. The
 * poison path signals the main thread precisely to avoid it.
 *
 * The ordering below is load-bearing: stop dispatcher, then remove the mount
 * point, then delete. Removing the mount point while the dispatcher still
 * serves requests is how a drive letter lingers after process exit. */
extern "C" void space_adapter_unmount(void)
{
    if (!g_FileSystem) return;
    FspFileSystemStopDispatcher(g_FileSystem);
    FspFileSystemRemoveMountPoint(g_FileSystem);
    FspFileSystemDelete(g_FileSystem);
    g_FileSystem = nullptr;
}
