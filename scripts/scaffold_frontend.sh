#!/usr/bin/env bash
set -euo pipefail

# ─── Usage ───────────────────────────────────────────────────────────────────
# ./scripts/scaffold_frontend.sh <service-dir> [app-name]
#
# Bootstraps the app with npm create vite (default 6.0.0, template svelte-ts),
# then npx storybook@<version> init (default 8.6.14, not latest) to scaffold config.
#
# Examples:
#   ./scripts/scaffold_frontend.sh basic-server
#   ./scripts/scaffold_frontend.sh basic-server basic-app
# ─────────────────────────────────────────────────────────────────────────────

readonly CREATE_VITE_VERSION="6.0.0"

scaffold_usage() {
    echo "Usage: $0 <service-dir> [app-name]"
    echo "  service-dir : directory relative to repo root (e.g. saps)"
    echo "  app-name    : package.json name (default: <service-dir minus 'service-'>-app)"
    echo ""
    echo "Optional env: CREATE_VITE_VERSION (default 6.0.0),"
    echo "             SCAFFOLD_NPM_LEGACY_PEER_DEPS=1 (template + scaffold npm installs)"
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

# Storybook packages are pinned separately in STORYBOOK_DEV_DEPENDENCIES.
# Do not pin vite or @sveltejs/vite-plugin-svelte here: they must match create-vite's
# template (e.g. create-vite@6 → Vite 6 + plugin-svelte 5). Pinning older majors breaks npm.
scaffold_define_dependency_arrays() {
    STORYBOOK_DEV_DEPENDENCIES=(
        "storybook@8.6.14"
        "@storybook/svelte@8.6.14"
        "@storybook/svelte-vite@8.6.14"
        "@storybook/addon-actions@8.6.14"
        "@storybook/addon-essentials@8.6.14"
        "@storybook/addon-interactions@8.6.14"
    )

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

scaffold_write_storybook_config() {
    local web_dir=$1
    cat >"${web_dir}/.storybook/main.ts" <<'SBMAIN'
import type { StorybookConfig } from '@storybook/svelte-vite';
import tsconfigPaths from 'vite-tsconfig-paths';

const config: StorybookConfig = {
  stories: ['../src/**/*.mdx', '../src/**/*.stories.@(svelte|ts)'],
  addons: ['@storybook/addon-essentials', '@storybook/addon-interactions'],
  framework: {
    name: '@storybook/svelte-vite',
    options: {},
  },
  async viteFinal(viteConfig) {
    const { mergeConfig } = await import('vite');
    return mergeConfig(viteConfig, {
      plugins: [tsconfigPaths()],
    });
  },
};

export default config;
SBMAIN

    cat >"${web_dir}/.storybook/preview.ts" <<'SBPREVIEW'
import type { Preview } from '@storybook/svelte';
import '../src/index.css';

const customViewports = {
  mobileSmall: {
    name: 'Mobile S (320x568)',
    styles: { width: '320px', height: '568px' },
    type: 'mobile',
  },
  mobile: {
    name: 'Mobile M (375x667)',
    styles: { width: '375px', height: '667px' },
    type: 'mobile',
  },
  tablet: {
    name: 'Tablet (768x1024)',
    styles: { width: '768px', height: '1024px' },
    type: 'tablet',
  },
  laptop: {
    name: 'Laptop (1024x768)',
    styles: { width: '1024px', height: '768px' },
    type: 'desktop',
  },
  desktop: {
    name: 'Desktop (1440x900)',
    styles: { width: '1440px', height: '900px' },
    type: 'desktop',
  },
  iphoneSE: {
    name: 'iPhone SE (320x449)',
    styles: { width: '320px', height: '449px' },
    type: 'mobile',
  },
  commonAndroid: {
    name: 'Common Android (360x649)',
    styles: { width: '360px', height: '649px' },
    type: 'mobile',
  },
  iphoneSE3: {
    name: 'iPhone SE (3rd) (375x547)',
    styles: { width: '375px', height: '547px' },
    type: 'mobile',
  },
  iphone15: {
    name: 'iPhone 15 (393x659)',
    styles: { width: '393px', height: '659px' },
    type: 'mobile',
  },
  iphone15Plus: {
    name: 'iPhone 15 Plus (430x739)',
    styles: { width: '430px', height: '739px' },
    type: 'mobile',
  },
  ipadMini6: {
    name: 'iPad Mini (6th) (744x1026)',
    styles: { width: '744px', height: '1026px' },
    type: 'tablet',
  },
  ipad10: {
    name: 'iPad (10th) (820x1073)',
    styles: { width: '820px', height: '1073px' },
    type: 'tablet',
  },
  ipadPro129: {
    name: 'iPad Pro (12.9") (1024x1259)',
    styles: { width: '1024px', height: '1259px' },
    type: 'tablet',
  },
  macBookAir13: {
    name: 'MacBook Air (13") (1280x715)',
    styles: { width: '1280px', height: '715px' },
    type: 'desktop',
  },
  macBookAir15: {
    name: 'MacBook Air (15") (1440x815)',
    styles: { width: '1440px', height: '815px' },
    type: 'desktop',
  },
  macBookPro14: {
    name: 'MacBook Pro (14") (1512x865)',
    styles: { width: '1512px', height: '865px' },
    type: 'desktop',
  },
  macBookPro16: {
    name: 'MacBook Pro (16") (1728x1000)',
    styles: { width: '1728px', height: '1000px' },
    type: 'desktop',
  },
  iMac24: {
    name: 'iMac (24") (2240x1156)',
    styles: { width: '2240px', height: '1156px' },
    type: 'desktop',
  },
  studioDisplay: {
    name: 'Studio Display (2560x1336)',
    styles: { width: '2560px', height: '1336px' },
    type: 'desktop',
  },
  studioDisplayHalf: {
    name: 'Studio Display, half (1278x1336)',
    styles: { width: '1278px', height: '1336px' },
    type: 'desktop',
  },
  proDisplayXDR: {
    name: 'Pro Display XDR (3008x1588)',
    styles: { width: '3008px', height: '1588px' },
    type: 'desktop',
  },
};

const preview: Preview = {
  parameters: {
    actions: { argTypesRegex: '^on[A-Z].*' },
    controls: {
      matchers: { color: /(background|color)$/i, date: /Date$/ },
    },
    viewport: {
      viewports: customViewports,
    },
  },
};

export default preview;
SBPREVIEW
}

scaffold_install_storybook_deps() {
    local web_dir=$1
    scaffold_npm_install_in_dir "${web_dir}" "Installing Storybook 8.6.14 packages..." \
        --save-dev "${STORYBOOK_DEV_DEPENDENCIES[@]}"
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

scaffold_write_minimal_app_files() {
    local web_dir=$1
    cat >"${web_dir}/src/App.svelte" <<'APPSVELTE'
<script lang="ts">
  let message = $state('Hello from Saps');
</script>

<main data-cmp="App">
  <h1>{message}</h1>
</main>
APPSVELTE

    cat >"${web_dir}/src/index.css" <<'INDEXCSS'
:root {
  --font-sans: 'Inter', system-ui, -apple-system, 'Segoe UI', Roboto, Arial,
    sans-serif;
  --color-white: #ffffff;
  --color-black: #000000;
}

*,
*::before,
*::after {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html,
body {
  height: 100%;
  font-family: var(--font-sans);
  background: var(--color-black);
  color: var(--color-white);
}

#app {
  height: 100%;
}
INDEXCSS

    cat >"${web_dir}/src/main.ts" <<'MAINTS'
import { mount } from 'svelte';
import App from './App.svelte';
import './index.css';

mount(App, {
  target: document.getElementById('app')!,
});
MAINTS

    cat >"${web_dir}/src/stories/App.stories.ts" <<'APPSTORY'
import type { Meta, StoryObj } from '@storybook/svelte';
import App from '../App.svelte';

const meta = {
  title: 'App/Placeholder',
  component: App,
} satisfies Meta<App>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
APPSTORY
}

scaffold_write_project_configs() {
    local web_dir=$1
    local display_name=$2
    cat >"${web_dir}/tsconfig.json" <<'TSCONFIG'
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "lib": ["ESNext", "DOM"],
    "strict": true,
    "allowJs": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "forceConsistentCasingInFileNames": true,
    "allowSyntheticDefaultImports": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "types": [
      "vitest/globals",
      "svelte",
      "vite/client",
      "node",
      "@testing-library/jest-dom"
    ],
    "baseUrl": ".",
    "paths": {
      "src/*": ["./src/*"],
      "root/*": ["./*"],
      "@components/*": ["src/components/*"],
      "@core/*": ["src/core/*"],
      "@pages/*": ["src/pages/*"],
      "@stories/*": ["src/stories/*"]
    }
  },
  "include": ["index.html", "vite.config.ts", "vite-setup.ts", "src"],
  "exclude": ["node_modules"]
}
TSCONFIG

    cat >"${web_dir}/svelte.config.js" <<'SVELTECONFIG'
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
};
SVELTECONFIG

    cat >"${web_dir}/index.html" <<INDEXHTML
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>${display_name}</title>
    <meta name="viewport" content="width=device-width,initial-scale=1" />

    <!-- Google Fonts: Inter -->
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link
      href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap"
      rel="stylesheet"
    />
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
INDEXHTML
}

scaffold_write_vite_setup() {
    local web_dir=$1
    cat >"${web_dir}/vite-setup.ts" <<'VITESETUP'
import { vi } from 'vitest';
import '@testing-library/jest-dom/vitest';

import { afterEach } from 'vitest';
import { cleanup } from '@testing-library/svelte';

// Ensure the DOM is cleaned between tests
afterEach(() => {
  cleanup();
});

// Global mocks that apply to all tests
global.fetch = vi.fn();

// Mock browser environment APIs
Object.defineProperty(window, 'location', {
  value: {
    href: 'http://localhost:3000',
    origin: 'http://localhost:3000',
    pathname: '/',
    search: '',
    hash: '',
  },
  writable: true,
});

// Mock localStorage
const localStorageMock = {
  getItem: vi.fn(),
  setItem: vi.fn(),
  removeItem: vi.fn(),
  clear: vi.fn(),
  length: 0,
  key: vi.fn(),
};
global.localStorage = localStorageMock as any;

// Mock sessionStorage
global.sessionStorage = localStorageMock as any;
VITESETUP
}

scaffold_update_package_json() {
    local web_dir=$1
    local app_name=$2
    (
        cd "${web_dir}"
        export SCAFFOLD_APP_NAME="${app_name}"
        node <<'NODE'
const fs = require('fs');
const pkgPath = 'package.json';
const appName = process.env.SCAFFOLD_APP_NAME;
const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));

pkg.name = appName || pkg.name;
pkg.private = true;
pkg.scripts = pkg.scripts || {};
pkg.scripts.dev = 'vite dev --host 0.0.0.0';
pkg.scripts.build = 'vite build';
pkg.scripts.check = 'svelte-check --tsconfig ./tsconfig.json';
pkg.scripts.test = 'vitest';
pkg.scripts['test:unit'] = 'vitest';
pkg.scripts.storybook = 'storybook dev -p 6006';
pkg.scripts['build-storybook'] = 'storybook build';

fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');
NODE
    )
}

scaffold_write_vite_config() {
    local web_dir=$1
    cat >"${web_dir}/vite.config.ts" <<'VITECONFIG'
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tsconfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  plugins: [svelte(), tsconfigPaths()],
  publicDir: false,
  server: { host: '0.0.0.0' },
  build: {
    outDir: 'public',
    emptyOutDir: false,
    sourcemap: true,
    cssCodeSplit: false,
    rollupOptions: {
      input: 'index.html',
      output: {
        entryFileNames: 'bundle.js',
        chunkFileNames: 'chunk-[hash].js',
        assetFileNames: (assetInfo) => {
          if ((assetInfo.name || '').endsWith('.css'))
            return 'bundle.css';
          return 'assets/[name][extname]';
        },
      },
    },
  },
  // @ts-expect-error Vitest augments Vite config with `test`
  test: {
    environment: 'jsdom',
    include: ['./src/**/*.{test,spec}.{js,ts}'],
    globals: true,
    setupFiles: ['./vite-setup.ts'],
  },
});
VITECONFIG
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
    local display_name
    display_name="$(echo "${app_name}" | sed 's/-/ /g' | sed 's/\b\(.\)/\u\1/g')"
    local web_dir
    web_dir="$(scaffold_web_dir_for_service "${service_dir}")"

    echo "Scaffolding Svelte 5 + TypeScript (Vite, not SvelteKit) at ${web_dir} (package: ${app_name})"

    scaffold_validate_web_dir_empty "${web_dir}"
    scaffold_define_dependency_arrays

    mkdir -p "${web_dir}"

    scaffold_run_npm_create_vite "${web_dir}"
    scaffold_update_package_json "${web_dir}" "${app_name}"

    # Must run before scaffold_write_minimal_app_files: that writes src/stories/App.stories.ts,
    # and bash cannot create intermediate dirs via `>` redirection (set -e would abort the
    # whole script, so src/lib would never be removed and Sap dirs would never be created).
    # ─── Remove Vite template src/lib ───
    rm -rf "${web_dir}/src/lib"
    rm -f "${web_dir}/src/app.css"

    # ─── Create Sap template directories ─────────────────────────────────────────────────
    mkdir -p \
        "${web_dir}/src/components" \
        "${web_dir}/src/core" \
        "${web_dir}/src/pages" \
        "${web_dir}/src/stories" \
        "${web_dir}/.storybook"

    scaffold_write_minimal_app_files "${web_dir}"
    scaffold_write_project_configs "${web_dir}" "${display_name}"
    scaffold_write_vite_setup "${web_dir}"

    # (1) Install Vite template deps, (2) write Storybook config,
    # (3) install pinned Storybook deps, (4) remaining scaffold deps.
    scaffold_npm_install_in_dir "${web_dir}" "Installing template dependencies..."

    scaffold_write_storybook_config "${web_dir}"
    scaffold_install_storybook_deps "${web_dir}"
    scaffold_write_vite_config "${web_dir}"

    scaffold_npm_install_in_dir "${web_dir}" "Installing scaffold packages (dev + prod, batched)..." \
        --save-dev "${SCAFFOLD_DEV_DEPENDENCIES[@]}" \
        --save "${SCAFFOLD_DEPENDENCIES[@]}"

    scaffold_print_next_steps "${web_dir}" "${app_name}"
}

scaffold_main "$@"
