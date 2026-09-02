# SPACE Development Machine Setup

## Environment

SPACE development and testing are performed inside the Windows 11 development VM.

- OS: Windows 11 Pro, 64-bit
- Architecture: x64
- C: drive: NTFS
- C:\SPACE is located on the VM's own virtual disk
- C:\SPACE is NOT located on a VirtualBox Shared Folder
- C:\SPACE is NOT located inside OneDrive
- S: is reserved for filesystem mount testing

## Required Directory Layout

The workspace uses:

C:\SPACE\
├── src\
├── secrets\
├── runtime\
│   ├── logs\
│   ├── crash\
│   ├── cache\
│   │   ├── bytes\
│   │   └── chunks\
│   ├── durable\
│   │   ├── wal\
│   │   ├── upload-queue\
│   │   └── sync-state\
│   └── temp\
└── test-data\
    ├── generated\
    ├── corruption\
    ├── exports\
    └── fixtures\

Only C:\SPACE\src is a Git repository.

Runtime and test-data are disposable.

Secrets must never enter Git.

## Windows Defender

An exclusion was added for the SPACE working directory.

Administrator PowerShell:

Add-MpPreference -ExclusionPath "C:\SPACE"

Verify:

Get-MpPreference | Select-Object -ExpandProperty ExclusionPath

The exclusion prevents Defender file locking from interfering with filesystem and chunk-publish testing.

## Crash Dumps

Local application crash dumps are configured at:

HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps

Configuration:

- DumpFolder = C:\SPACE\runtime\crash
- DumpCount = 20
- DumpType = 2

Administrator PowerShell:

$k = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps"
New-Item -Path $k -Force | Out-Null
New-ItemProperty -Path $k -Name "DumpFolder" -Value "C:\SPACE\runtime\crash" -PropertyType ExpandString -Force
New-ItemProperty -Path $k -Name "DumpCount" -Value 20 -PropertyType DWord -Force
New-ItemProperty -Path $k -Name "DumpType" -Value 2 -PropertyType DWord -Force

Windows Startup and Recovery settings:

- Write debugging information: Kernel memory dump
- Automatically restart: disabled

## Git

Git is installed and configured for Windows filesystem development.

Required configuration:

git config --global core.autocrlf false
git config --global core.longpaths true
git config --global init.defaultBranch main

Current identity:

- user.name = pinkrobot7264-pixel
- user.email = pinkrobot7264@gmail.com

Verify:

git config --global --list

`core.longpaths=true` is mandatory because later tests exercise paths exceeding the traditional 260-character Windows path limit.

## Rust

Rust is installed through rustup.

Required toolchain:

- stable
- x86_64-pc-windows-msvc

Installed Rust version:

- rustc 1.98.0
- cargo 1.98.0

Rust binaries are available through:

C:\Users\betha\.cargo\bin

Verify:

rustc --version
cargo --version
rustup show

## Cargo Nextest

The workspace test runner is cargo-nextest.

Installed version:

cargo-nextest 0.9.143

Verify:

cargo nextest --version

## Visual Studio Build Tools

The C++ build environment uses Visual Studio 2022 Build Tools.

Required components:

- Desktop development with C++
- MSVC v143 x64/x86 build tools
- Windows 11 SDK
- C++ CMake tools for Windows

The Windows Driver Kit is not required.

Use an x64 Developer Command Prompt so that:

where cl.exe
where link.exe
cmake --version
ninja --version

resolve correctly.

## WinFsp

WinFsp is installed with:

- Core
- Developer

Current installed version:

WinFsp 2.1

Required Developer files:

C:\Program Files (x86)\WinFsp\inc\winfsp\winfsp.h
C:\Program Files (x86)\WinFsp\lib\winfsp-x64.lib

Verify:

fsptool-x64.exe ver

The C++ adapter links against:

C:\Program Files (x86)\WinFsp\lib\winfsp-x64.lib

## WinFsp Reference Filesystem

The WinFsp MEMFS sample was used to verify the Windows/WinFsp environment before implementing SPACE filesystem behavior.

S: was successfully mounted and tested for:

- directory creation
- file creation
- read/write
- rename
- copy
- delete
- nested directories
- process termination and restart behavior

MEMFS is only an environment/reference test. It is not the SPACE filesystem implementation.

## Virtual Machine Safety

The development VM provides the OS-safety boundary for filesystem experimentation.

Do not place C:\SPACE on a VirtualBox Shared Folder.

Shared folders use filesystem redirector semantics that are not suitable for validating Windows filesystem behavior.

All filesystem testing must use the guest's own virtual disk.

## Future Large Workload

The Phase 11 500 GB workload will not fit on the current VM disk.

Before Phase 11, choose one of:

1. Attach a second large virtual disk.
2. Run Phase 11 on bare metal.
3. Use the sparse/streaming fixture generation approach defined by the project.

This decision must be revisited before Phase 11.

## Repository

Repository location:

C:\SPACE\src\space

The repository contains the SPACE Rust workspace and C++ WinFsp adapter.

## PostgreSQL 18

Install it during Phase 0 setup so the bootstrap is complete, even though **no
Phase 0 code uses it** -- the cloud metadata store is in-memory behind the
`MetadataStore` trait. PostgreSQL is first *used* in **Phase 8** (backend +
schema). Installing it now means Phase 8 does not start with an install.

1. Download the EDB installer from
   <https://www.postgresql.org/download/windows/> -- latest **18.x** (not 19,
   which is beta into late 2026).
2. Installer choices: components = Server + pgAdmin 4 + Command Line Tools
   (untick Stack Builder); port `5432`; locale `C` (deterministic sort order in
   tests); a strong superuser password saved to
   `C:\SPACE\secrets\postgres-superuser.txt` (that path is git-ignored).
3. Put `psql` on PATH:

   ```powershell
   [Environment]::SetEnvironmentVariable(
     "Path", $env:Path + ";C:\Program Files\PostgreSQL\18\bin", "User")
   ```

4. Open a **new** PowerShell window, then `psql --version` (expect `psql (PostgreSQL) 18.x`).

`verify-env.ps1` checks `psql --version` matches `1[89]\.` and fails the
environment check if PostgreSQL is missing.

## Python 3.12

A supporting tool for repository helper scripts (no Phase 0 crate depends on
it). Install during Phase 0 setup:

```powershell
winget install -e --id Python.Python.3.12   # tick "Add python.exe to PATH"
```

Open a new shell, then `python --version` (expect `3.12.x` or newer).
`verify-env.ps1` checks `python --version` matches `3\.1[2-9]`.

> If the Microsoft-Store `python.exe` alias intercepts the command, disable it
> under *Settings -> Apps -> Advanced app settings -> App execution aliases*.

## Secret scanning (gitleaks)

Install during Phase 0 setup:

```powershell
winget install -e --id Gitleaks.Gitleaks
```

winget places a `gitleaks` shim in `%LOCALAPPDATA%\Microsoft\WinGet\Links`,
which winget adds to the user PATH. Open a new shell, then `gitleaks version`.

`scripts/bootstrap.ps1` re-checks for `gitleaks`, installs it via winget if
missing, refreshes PATH in-session, and installs `scripts/pre-commit.sh` to
`.git/hooks/pre-commit`. The hook resolves the `gitleaks` binary itself (PATH,
then the winget shim/package locations), so it works regardless of the shell
that runs `git commit`. It runs `gitleaks protect --staged` and blocks a commit
that stages a probable secret. CI additionally runs `gitleaks detect`.
`verify-env.ps1` checks `gitleaks version`.

## Build

From:

C:\SPACE\src\space

Run:

.\scripts\bootstrap.ps1
.\scripts\build.ps1

The build script verifies:

- Rust workspace debug build
- Rust workspace release build
- CMake configuration
- WinFsp discovery
- C++ adapter build

## Test

Run:

.\scripts\test.ps1

The test script verifies:

- Rust formatting
- Clippy with warnings denied
- cargo-nextest workspace tests (contracts, config, logging, faults, object
  store conformance, generators, corruptor, metadata store, cloud API, client
  core, integration harness, request-id cross-process propagation, and the E2E
  happy + negative contract chains including the real 32 MiB boundary sizes)
- the release fault-injection guard

One test is `#[ignore]`d: `space-generators streaming_full_gib` (a 1 GiB
ChaCha8 pass is slow in a debug build). Run it with
`cargo nextest run -p space-generators --run-ignored all` or in release.

## Host / VM settings (reproduced by this runbook)

- VirtualBox 7.x host; guest = Windows 11 Pro, EFI + Secure Boot + TPM 2.0.
- Host hypervisor (`bcdedit /set hypervisorlaunchtype off`) and Memory Integrity
  disabled so VirtualBox does not fall back to the slow compatibility path.
- 3D acceleration off; `C:\SPACE` on the guest's own virtual disk only.

## Phase 0 Rule

When rebuilding the development environment from a clean VM snapshot, follow this document rather than relying on memory.

If a required setup step is discovered that is not documented here, update this runbook before declaring the environment reproducible.