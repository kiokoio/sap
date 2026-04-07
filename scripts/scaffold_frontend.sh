#!/usr/bin/env bash
set -euo pipefail

# ─── Usage ───────────────────────────────────────────────────────────────────
# ./scripts/scaffold_frontend.sh <service-dir> [app-name]
#
# Bootstraps the app with npm create vite (default 6.0.0, template svelte-ts),
# then npx storybook@<version> init (default 8.6.14, not latest) for Storybook deps.
# create-vite 9 ships @sveltejs/vite-plugin-svelte@7; Storybook 8.6 only peers
# plugin-svelte ^2–5 — use 6.x here, or set SCAFFOLD_NPM_LEGACY_PEER_DEPS=1 with
# create-vite 9. Node/.nvmrc does not fix ERESOLVE peer conflicts.
#
# Examples:
#   ./scripts/scaffold_frontend.sh service-observability
#   ./scripts/scaffold_frontend.sh service-observability observability-app
# ─────────────────────────────────────────────────────────────────────────────

readonly CREATE_VITE_VERSION="6.0.0"

scaffold_usage() {
    echo "Usage: $0 <service-dir> [app-name]"
    echo "  service-dir : directory relative to repo root (e.g. service-observability)"
    echo "  app-name    : package.json name (default: <service-dir minus 'service-'>-app)"
    echo ""
    echo "Optional env: CREATE_VITE_VERSION (default 6.0.0), STORYBOOK_VERSION (default 8.6.14),"
    echo "             SCAFFOLD_NPM_LEGACY_PEER_DEPS=1"
}

scaffold_die() {
    echo "Error: $*" >&2
    exit 1
}

scaffold_ensure_repo_root() {
    local scriptpath
    scriptpath="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
    cd "${scriptpath}/.." || scaffold_die "could not cd to repo root"
}

scaffold_web_dir_for_service() {
    local service_dir=$1
    echo "${service_dir}/frontend/web"
}

scaffold_validate_web_dir_empty() {
    local web_dir=$1
    if [[ -d "$web_dir" ]] && [[ -n "$(ls -A "$web_dir" 2>/dev/null)" ]]; then
        scaffold_die "${web_dir} already contains files. Remove them first or pick a different target."
    fi
}

# Storybook packages come from `npx storybook@<STORYBOOK_VERSION> init` (see scaffold_run_storybook_init).
# Do not pin vite or @sveltejs/vite-plugin-svelte here: they must match create-vite's
# template (e.g. create-vite@6 → Vite 6 + plugin-svelte 5). Pinning older majors breaks npm.
scaffold_define_dependency_arrays() {
    SCAFFOLD_DEV_DEPENDENCIES=(
        "@testing-library/jest-dom@^6.9.1"
        "@testing-library/svelte@^5.2.8"
        "@testing-library/user-event@^14.6.1"
        "@types/node@^24.4.0"
        "@vitest/coverage-v8@^2.1.1"
        "jsdom@^25.0.1"
        "svelte@^5.41.1"
        "svelte-check@^4.0.0"
        "typescript@^5.0.0"
        "vitest@^2.1.9"
    )

    SCAFFOLD_DEPENDENCIES=(
        "vite-tsconfig-paths@^5.1.4"
        "zod@^4.1.8"
    )
}

# Pinned CLI version (e.g. 8.6.14).
scaffold_run_storybook_init() {
    local web_dir=$1
    local storybook_version=$2
    echo ""
    echo "Running npx storybook@${storybook_version} init --yes --no-dev --builder vite --package-manager npm..."
    (
        cd "${web_dir}"
        if [[ "${SCAFFOLD_NPM_LEGACY_PEER_DEPS:-}" == "1" ]]; then
            export npm_config_legacy_peer_deps=true
        fi
        npx --yes "storybook@${storybook_version}" init \
            --yes \
            --no-dev \
            --builder vite \
            --package-manager npm \
            --disable-telemetry
    )
}

scaffold_run_npm_create_vite() {
    local web_dir=$1
    echo ""
    echo "Running npm create vite@${CREATE_VITE_VERSION} (template: svelte-ts, not SvelteKit)..."
    (
        cd "${web_dir}"
        npm create "vite@${CREATE_VITE_VERSION}" . -- --template svelte-ts --no-interactive
    )
}

# Optional: SCAFFOLD_NPM_LEGACY_PEER_DEPS=1 appends --legacy-peer-deps
scaffold_npm_install_in_dir() {
    local web_dir=$1
    local message=$2
    shift 2
    local legacy=()
    if [[ "${SCAFFOLD_NPM_LEGACY_PEER_DEPS:-}" == "1" ]]; then
        legacy+=(--legacy-peer-deps)
    fi
    echo ""
    echo "${message}"
    (
        cd "${web_dir}"
        npm install --no-fund --no-audit "${legacy[@]}" "$@"
    )
}

scaffold_print_next_steps() {
    local web_dir=$1
    local app_name=$2
    echo ""
    echo "Done! Frontend scaffolded at ${web_dir}"
    echo ""
    echo "Next steps:"
    echo "  npm run dev -w ${app_name}          # start dev server"
    echo "  npm run storybook -w ${app_name}    # start storybook"
    echo "  npm run test -w ${app_name}         # run tests"
    echo "  npm run build -w ${app_name}        # production build"
}

scaffold_main() {
    if [[ $# -lt 1 ]]; then
        scaffold_usage
        exit 1
    fi

    scaffold_ensure_repo_root

    local service_dir=$1
    local default_name="${service_dir#service-}-app"
    local app_name="${2:-$default_name}"
    local web_dir
    web_dir="$(scaffold_web_dir_for_service "${service_dir}")"

    echo "Scaffolding Svelte 5 + TypeScript (Vite, not SvelteKit) at ${web_dir} (package: ${app_name})"

    scaffold_validate_web_dir_empty "${web_dir}"
    scaffold_define_dependency_arrays

    mkdir -p "${web_dir}"

    scaffold_run_npm_create_vite "${web_dir}"

    # ─── Create directories ─────────────────────────────────────────────────
    mkdir -p \
        "${web_dir}/src/components" \
        "${web_dir}/src/core" \
        "${web_dir}/src/pages" \
        "${web_dir}/src/stories" \
        "${web_dir}/.storybook"

    # (1) Install Vite template deps, (2) Storybook CLI adds packages + config, (3) remaining scaffold deps.
    scaffold_npm_install_in_dir "${web_dir}" "Installing template dependencies..."

    local storybook_version="${STORYBOOK_VERSION:-8.6.14}"
    scaffold_run_storybook_init "${web_dir}" "${storybook_version}"

    scaffold_npm_install_in_dir "${web_dir}" "Installing scaffold packages (dev + prod, batched)..." \
        --save-dev "${SCAFFOLD_DEV_DEPENDENCIES[@]}" \
        --save "${SCAFFOLD_DEPENDENCIES[@]}"

    scaffold_print_next_steps "${web_dir}" "${app_name}"
}

scaffold_main "$@"
