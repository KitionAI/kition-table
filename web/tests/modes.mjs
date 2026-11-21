// The editor in each display mode (@kition/sdk/editor's EditorFrame), through the development host:
// node tests/modes.mjs [chromium|webkit]. The editor has no frame tabs, so its mode button
// offers only Focus, and Menus, Single line, and Classic draw the same.
import { chromium, webkit } from 'playwright'
import { createServer } from 'vite'
import { runModeSmoke } from '@kition/sdk/dev-host/mode-smoke'

await runModeSmoke({
  kit: 'kit.table',
  config: new URL('../vite.config.ts', import.meta.url).pathname,
  browser: process.argv[2] === 'webkit' ? webkit : chromium,
  createServer,
})
