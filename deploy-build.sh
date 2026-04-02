#!/bin/bash
# QRAFT-CUSTOM: Self-hosted build wrapper with verification gates
# Usage: ./deploy-build.sh --desktop

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── GATE 1: Pre-build env var completeness ───────────────────────────────────
echo "🔍 [Gate 1] Checking required env vars..."

# Self-hosted environment overrides
export VK_SHARED_API_BASE="http://10.8.97.88:3000"
export VITE_VK_SHARED_API_BASE="http://10.8.97.88:3000"
export VK_SHARED_RELAY_API_BASE="http://10.8.97.88:8082"

# Check required self-hosted env vars (optional analytics vars like POSTHOG/SENTRY are skipped)
REQUIRED_VARS=("VK_SHARED_API_BASE" "VK_SHARED_RELAY_API_BASE")
MISSING=0
for var in "${REQUIRED_VARS[@]}"; do
    if [[ -z "${!var:-}" ]]; then
        echo "  ❌ MISSING: $var"
        MISSING=1
    else
        echo "  ✅ $var = ${!var}"
    fi
done

if [[ $MISSING -eq 1 ]]; then
    echo "❌ [Gate 1] FAILED — add missing vars above before building"
    exit 1
fi
echo "✅ [Gate 1] All env vars present"

# ── Build ────────────────────────────────────────────────────────────────────
echo "🏠 QRAFT self-hosted build: VK_SHARED_API_BASE=${VK_SHARED_API_BASE}"

echo "🔨 Building web app..."
# SENTRY_UPLOAD=false disables sentry-vite-plugin source map upload for local builds
(cd "$SCRIPT_DIR/packages/local-web" && SENTRY_UPLOAD=false npm run build)

echo "🖥️  Building Tauri desktop app (.app only)..."
# Fix updater endpoint placeholder for local builds
TAURI_CONF="$SCRIPT_DIR/crates/tauri-app/tauri.conf.json"
node -e "
  const fs = require('fs');
  const conf = JSON.parse(fs.readFileSync('$TAURI_CONF', 'utf8'));
  // Disable updater plugin entirely for local builds (no signing key available)
  delete conf.plugins.updater;
  if (conf.bundle) conf.bundle.createUpdaterArtifacts = false;
  fs.writeFileSync('$TAURI_CONF', JSON.stringify(conf, null, 2) + '\n');
"
# --bundles app: build .app only, skip bundle_dmg.sh (DMG created manually via hdiutil in Gate 2)
(cd "$SCRIPT_DIR" && cargo tauri build --bundles app)
git -C "$SCRIPT_DIR" checkout -- "$TAURI_CONF"

# ── GATE 2: Post-build binary verification (desktop only) ───────────────────
if [[ "$*" == *"--desktop"* ]]; then
    echo "🔍 [Gate 2] Verifying binary URLs..."
    APP_DIR="$SCRIPT_DIR/target/release/bundle/macos/Vibe Kanban.app"
    BINARY=$(find "$APP_DIR" -name "vibe-kanban-tauri" -type f 2>/dev/null | head -1)

    if [[ -z "$BINARY" ]]; then
        echo "❌ [Gate 2] FAILED — binary not found at $APP_DIR"
        exit 1
    fi

    SELF_HOSTED_COUNT=$(strings "$BINARY" | grep -c "10.8.97.88" || true)
    SAAS_COUNT=$(strings "$BINARY" | grep -c "api.vibekanban.com" || true)

    echo "  10.8.97.88 occurrences   : $SELF_HOSTED_COUNT  (required: ≥1)"
    echo "  api.vibekanban.com count : $SAAS_COUNT  (required: 0)"

    if [[ $SELF_HOSTED_COUNT -lt 1 ]]; then
        echo "❌ [Gate 2] FAILED — self-hosted URL not baked in. VK_SHARED_API_BASE may not have reached the compiler."
        exit 1
    fi
    if [[ $SAAS_COUNT -gt 0 ]]; then
        echo "❌ [Gate 2] FAILED — SaaS URL still in binary!"
        exit 1
    fi

    echo "✅ [Gate 2] Binary verified — creating DMG..."

    # Create DMG now (timing-safe: binary is verified before DMG creation)
    DMG_PATH="$SCRIPT_DIR/target/release/bundle/macos/VibeKanban-self-hosted.dmg"
    rm -f "$DMG_PATH"
    hdiutil create -volname "Vibe Kanban" \
        -srcfolder "$APP_DIR" \
        -ov -format UDZO \
        "$DMG_PATH" 2>&1

    # Verify DMG internal binary too
    DMG_MOUNT=$(mktemp -d)
    hdiutil attach "$DMG_PATH" -mountpoint "$DMG_MOUNT" -nobrowse -quiet
    DMG_BINARY=$(find "$DMG_MOUNT" -name "vibe-kanban-tauri" -type f | head -1)
    DMG_SELF=$(strings "$DMG_BINARY" | grep -c "10.8.97.88" || true)
    DMG_SAAS=$(strings "$DMG_BINARY" | grep -c "api.vibekanban.com" || true)
    hdiutil detach "$DMG_MOUNT" -quiet
    rm -rf "$DMG_MOUNT"

    if [[ $DMG_SELF -lt 1 || $DMG_SAAS -gt 0 ]]; then
        echo "❌ [Gate 2] DMG internal binary mismatch! (10.8.97.88=$DMG_SELF, saas=$DMG_SAAS)"
        exit 1
    fi

    echo "✅ [Gate 2] DMG verified: $DMG_PATH"
    echo ""
    echo "📦 Ready to distribute: $DMG_PATH"
fi
