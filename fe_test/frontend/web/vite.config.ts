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
  test: {
    environment: 'jsdom',
    include: ['./src/**/*.{test,spec}.{js,ts}'],
    globals: true,
    setupFiles: ['./vite-setup.ts'],
  },
});
