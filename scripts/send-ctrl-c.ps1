# Deliver CTRL_C_EVENT to another process, without signalling the caller.
#
# GenerateConsoleCtrlEvent signals every process attached to the console, so
# calling it in-process would also signal the script doing the calling -- which
# in a test harness means the harness dies instead of the subject. This script
# is therefore meant to be run as an ISOLATED CHILD PowerShell: it frees its own
# console, attaches to the target's, disables its own handler, signals, and
# exits.
#
# Used by scripts/shutdown-states.ps1 (section 15.1) and scripts/mount-stress.ps1
# (section 16.1), both of which need a genuinely GRACEFUL shutdown -- the
# ADR-0013a main-thread teardown path -- as distinct from a force kill, which is
# the Class B process-death path. A harness that force-kills in both branches is
# not testing two paths, it is testing one and mislabelling half the results.
#
# Exit code 0 = the signal was delivered. 1 = could not attach to the console.

param([Parameter(Mandatory = $true)][int]$TargetPid)

Add-Type -Namespace SpaceW -Name Kernel -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool AttachConsole(uint dwProcessId);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool FreeConsole();
[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr HandlerRoutine, bool Add);
[DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint dwCtrlEvent, uint dwProcessGroupId);
'@

[void][SpaceW.Kernel]::FreeConsole()
if ([SpaceW.Kernel]::AttachConsole([uint32]$TargetPid)) {
    # TRUE adds a NULL handler, which means "ignore Ctrl-C in this process".
    [void][SpaceW.Kernel]::SetConsoleCtrlHandler([IntPtr]::Zero, $true)
    [void][SpaceW.Kernel]::GenerateConsoleCtrlEvent(0, 0)   # CTRL_C_EVENT, whole group
    Start-Sleep -Milliseconds 500
    [void][SpaceW.Kernel]::FreeConsole()
    exit 0
}
exit 1
