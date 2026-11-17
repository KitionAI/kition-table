import path from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'
import autoprefixer from 'autoprefixer'
import tailwindcss from 'tailwindcss'
import { defineConfig } from 'vitest/config'
import tailwindConfig from './tailwind.config.ts'

const web = path.dirname(fileURLToPath(import.meta.url))

// The kit package is served from wherever a host puts it, so every asset path is relative.
export default defineConfig({
  base: './',
  plugins: [react()],
  css: { postcss: { plugins: [tailwindcss(tailwindConfig), autoprefixer()] } },
  resolve: {
    // One React for the page, also when KITION_LINK=1 links @kition/kit-editor from kition-sdk.
    dedupe: ['react', 'react-dom'],
    // ES module builds first, also in the specs, where Node would otherwise take a CommonJS build
    // that requires its own checkout's React.
    mainFields: ['browser', 'module', 'jsnext:main', 'jsnext', 'main'],
  },
  // kit-editor stays unbundled (it finds its engine worker next to it); the components it takes
  // from Tessera are bundled with their CommonJS dependencies (use-sync-external-store).
  optimizeDeps: {
    exclude: ['@kition/kit-editor', '@kition/kit-host', '@kition/kit-ui-protocol', '@kition/kit-dev-host'],
    include: ['@kition/kit-editor > @kition/tessera-ui'],
  },
  worker: { format: 'es' },
  server: {
    port: 5189,
    fs: { allow: [web, path.join(web, '..', 'kit.json'), path.join(web, '..', '..', 'kition-sdk'), path.join(web, '..', '..', 'kition-tessera')] },
  },
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 4096,
    rollupOptions: { input: { index: path.join(web, 'index.html'), dev: path.join(web, 'dev.html') } },
  },
  // The editor's own specs (`pnpm test:unit`); tests/*.test.mjs drive the built engine from Node.
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    globals: true,
    css: false,
    testTimeout: 10_000,
    include: ['src/**/*.spec.ts', 'src/**/*.spec.tsx'],
    // Checkouts that KITION_LINK=1 links have a React of their own: transform them, so the dedupe
    // above gives the specs one React.
    server: { deps: { inline: [/kition-sdk/, /kition-tessera/] } },
  },
})
