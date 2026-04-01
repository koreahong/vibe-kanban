#!/bin/bash
# QRAFT-CUSTOM: Self-hosted build wrapper
# Overrides SaaS defaults with self-hosted config, then delegates to upstream build script.
# Usage: ./deploy-build.sh --desktop  (or --remote, same flags as local-build.sh)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Self-hosted environment overrides
# upstream local-build.sh sets "https://api.vibekanban.com" as default,
# but these take precedence because they are exported BEFORE the script runs.
export VK_SHARED_API_BASE="http://10.8.97.88:3000"
export VITE_VK_SHARED_API_BASE="http://10.8.97.88:3000"

echo "🏠 QRAFT self-hosted build: VK_SHARED_API_BASE=${VK_SHARED_API_BASE}"
exec "$SCRIPT_DIR/local-build.sh" "$@"
