#!/bin/sh
# SPACE secret-scan pre-commit hook. Installed to .git/hooks/pre-commit by
# scripts/bootstrap.ps1. Blocks a commit that stages a probable secret.
#
# Resolves gitleaks without relying on the calling shell's PATH: PATH first,
# then the standard winget shim/package locations. gitleaks is a documented
# Phase 0 install (dev-machine-setup.md -> "Secret scanning").

gitleaks_bin() {
    if command -v gitleaks >/dev/null 2>&1; then
        command -v gitleaks
        return 0
    fi
    _local="${LOCALAPPDATA:-$USERPROFILE/AppData/Local}"
    for c in \
        "$_local/Microsoft/WinGet/Links/gitleaks.exe" \
        "$HOME/AppData/Local/Microsoft/WinGet/Links/gitleaks.exe" \
        "/c/ProgramData/chocolatey/bin/gitleaks.exe"; do
        [ -x "$c" ] && printf '%s\n' "$c" && return 0
    done
    _pkg=$(find "$_local/Microsoft/WinGet/Packages" -maxdepth 3 -iname 'gitleaks.exe' 2>/dev/null | head -n 1)
    [ -n "$_pkg" ] && printf '%s\n' "$_pkg" && return 0
    return 1
}

GITLEAKS=$(gitleaks_bin)
if [ -z "$GITLEAKS" ]; then
    echo "COMMIT BLOCKED: gitleaks not found."
    echo "  Install it:  winget install -e --id Gitleaks.Gitleaks"
    echo "  Then re-run: .\\scripts\\bootstrap.ps1"
    exit 1
fi

"$GITLEAKS" protect --staged --redact --no-banner
if [ $? -ne 0 ]; then
    echo "COMMIT BLOCKED: gitleaks found a potential secret."
    exit 1
fi

# cargo fmt --check, same command CI runs as its first gated step.
#
# This gate exists because formatting drift once reached CI and FAILED the
# windows job at step 7, which skipped Lint, Build, Test, Release build, the
# C++ adapter build and the secret scan -- so a whole push produced no
# evidence about any of them. A trivial defect that hides six real checks is
# worth blocking locally.
if command -v cargo >/dev/null 2>&1; then
    if ! cargo fmt --all -- --check >/dev/null 2>&1; then
        echo "COMMIT BLOCKED: cargo fmt --all -- --check fails."
        echo "  Run:  cargo fmt --all"
        cargo fmt --all -- --check 2>&1 | head -20
        exit 1
    fi
fi
