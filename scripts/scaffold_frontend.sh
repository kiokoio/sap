#!/usr/bin/env bash
set -euo pipefail

# ─── Usage ───────────────────────────────────────────────────────────────────
# ./scripts/scaffold_frontend.sh <service-dir> [app-name]
#
# Bootstraps the app with: npm create vite@6.0.0 . -- --template svelte-ts
# (vanilla Vite + Svelte 5 + TypeScript, not SvelteKit). create-vite is pinned
# so @storybook/svelte-vite@8.x peer deps stay satisfied; then this script adds
# Storybook, Vitest, path aliases, and the Kioko src layout.
#
# Examples:
#   ./scripts/scaffold_frontend.sh service-observability
#   ./scripts/scaffold_frontend.sh service-observability observability-app
# ─────────────────────────────────────────────────────────────────────────────

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <service-dir> [app-name]"
    echo "  service-dir : directory relative to repo root (e.g. service-observability)"
    echo "  app-name    : package.json name (default: <service-dir minus 'service-'>-app)"
    exit 1
fi

SCRIPTPATH="$(
    cd "$(dirname "$0")"
    pwd -P
)"
cd "$SCRIPTPATH/.."

SERVICE_DIR="$1"
DEFAULT_NAME="${SERVICE_DIR#service-}-app"
APP_NAME="${2:-$DEFAULT_NAME}"

# Capitalise first letter for HTML title
DISPLAY_NAME="$(echo "$APP_NAME" | sed 's/-/ /g' | sed 's/\b\(.\)/\u\1/g')"

# ─── Validate ────────────────────────────────────────────────────────────────

WEB_DIR="${SERVICE_DIR}/frontend/web"

# Check if web dir already has files (allow empty dir or non-existent)
if [[ -d "$WEB_DIR" ]] && [[ -n "$(ls -A "$WEB_DIR" 2>/dev/null)" ]]; then
    echo "Error: ${WEB_DIR} already contains files. Remove them first or pick a different target."
    exit 1
fi

echo "Scaffolding Svelte + TypeScript (Vite, not SvelteKit) at ${WEB_DIR} (package: ${APP_NAME})"

# ─── Initialize from official Vite + Svelte + TS template ───────────────────

mkdir -p "${WEB_DIR}"
echo ""
echo "Running npm create vite@latest (template: svelte-ts, not SvelteKit)..."
(
    cd "${WEB_DIR}"
    npm create vite@latest . -- --template svelte-ts --no-interactive
)

# ─── Install ─────────────────────────────────────────────────────────────────

echo ""
echo "Running npm install..."
(
    cd "${WEB_DIR}"
    npm install
)

echo ""
echo "Done! Frontend scaffolded at ${WEB_DIR}"
echo ""
echo "Next steps:"
echo "  npm run dev -w ${APP_NAME}          # start dev server"
echo "  npm run storybook -w ${APP_NAME}    # start storybook"
echo "  npm run test -w ${APP_NAME}         # run tests"
echo "  npm run build -w ${APP_NAME}        # production build"
