#!/bin/bash
# QRAFT-CUSTOM: Verify all QRAFT patches survived a rebase/merge
# Usage: ./scripts/verify-qraft-patches.sh

set -e

EXPECTED_FILES=(
  "crates/jira/src/client.rs"
  "crates/jira/src/mapper.rs"
  "crates/remote/src/routes/jira.rs"
  "crates/remote/src/routes/jira_hooks.rs"
  "crates/server/src/routes/sessions/delete.rs"
  "crates/db/migrations/20260401000000_add_session_soft_delete.sql"
  "packages/web-core/src/shared/dialogs/settings/settings/McpTogglePanel.tsx"
  "deploy-build.sh"
)
EXPECTED_MARKERS=20

echo "=== QRAFT Patch Verification ==="
MISSING=0
for f in "${EXPECTED_FILES[@]}"; do
  if [ -f "$f" ]; then
    echo "  ✓ $f"
  else
    echo "  ✗ MISSING: $f"
    MISSING=$((MISSING + 1))
  fi
done

echo ""
ACTUAL=$(grep -rn "QRAFT-CUSTOM" --include="*.rs" --include="*.ts" --include="*.tsx" --include="*.sql" --include="*.sh" . 2>/dev/null | grep -v "/target/" | grep -v "node_modules/" | wc -l | tr -d ' ')
echo "QRAFT-CUSTOM markers found: $ACTUAL (expected ≥ $EXPECTED_MARKERS)"
if [ "$ACTUAL" -ge "$EXPECTED_MARKERS" ]; then
  echo "  ✓ Markers OK"
else
  echo "  ⚠ WARNING: Some markers may be missing!"
fi

echo ""
CUSTOM_COMMITS=$(git log --oneline upstream/main..HEAD 2>/dev/null | wc -l | tr -d ' ')
echo "Custom commits on top of upstream/main: $CUSTOM_COMMITS"
git log --oneline upstream/main..HEAD 2>/dev/null

echo ""
if [ "$MISSING" -eq 0 ] && [ "$ACTUAL" -ge "$EXPECTED_MARKERS" ]; then
  echo "✅ All QRAFT patches intact!"
else
  echo "❌ Verification failed — check missing files/markers above"
  exit 1
fi
