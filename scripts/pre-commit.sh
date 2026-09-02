#!/bin/sh
# SPACE secret-scan pre-commit hook. Installed to .git/hooks/pre-commit by
# scripts/bootstrap.ps1. Blocks a commit that stages a probable secret.
gitleaks protect --staged --redact --no-banner
if [ $? -ne 0 ]; then
    echo "COMMIT BLOCKED: gitleaks found a potential secret."
    exit 1
fi
