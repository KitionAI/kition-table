/**
 * The development page of the new grid (`dev/grid.html`, served by `pnpm dev`; not an entry of the
 * release build): opens a table file in the kit's engine and shows one view through `GridView`,
 * read-only. The performance and parity tests (`tests/perf.mjs`, `tests/parity.mjs`) drive it.
 *
 * Query: `file` (a .ktable URL, default the development sample), `table` and `view` (ids, default
 * the first table's first view), `theme` (`light` or `dark`), `brand` (`#rrggbb`), `hud=1` (a
 * small panel with frame and band timings, for looking at it by hand).
 *
 * `window.__ktableGrid` holds the view and the engine, `window.__ktableGridStats` the view's
 * timings: only this page has them. `body[data-ready]` is set once the axes are in.
 */
import { KitEngine } from '@kition/kit-editor'
import { engineSource } from './engineSource'
import { engineFontUrls } from './gridFonts'
import { GridView } from './gridView'

declare global {
  interface Window {
    __ktableGrid?: { view: GridView; engine: KitEngine }
    __ktableGridStats?: GridView['stats']
  }
}

const root = new URL('../', document.baseURI)
const params = new URL(location.href).searchParams

async function start(): Promise<{ view: GridView; engine: KitEngine }> {
  const fontsBase = new URL('fonts/', root).href
  const file = new URL(params.get('file') ?? 'sample/Sample.ktable', root)
  const bytes = await (await fetch(file)).arrayBuffer()
  const engine = await KitEngine.open(
    { kitId: 'kit.table', base: new URL('engine/', root).href, module: 'kition_table_kit', documentClass: 'TableDocument', fonts: engineFontUrls(fontsBase) },
    decodeURIComponent(file.pathname.split('/').pop() ?? 'Table.ktable'),
    bytes,
  )
  const answer = JSON.parse(await engine.invoke<string>('call', 'documents.get', '{}')) as { result: { tables: Array<{ id: number; views: Array<{ id: number }> }> } }
  const tables = answer.result.tables
  const tableId = Number(params.get('table') ?? tables[0]?.id)
  const table = tables.find((item) => item.id === tableId)
  const viewId = params.has('view') ? Number(params.get('view')) : (table?.views[0]?.id ?? null)
  const theme = params.get('theme') === 'dark' ? 'dark' : 'light'
  document.documentElement.style.background = theme === 'dark' ? '#18181b' : '#fff'
  const host = document.getElementById('grid') as HTMLElement
  const view = new GridView(host, engineSource(engine, { tableId, viewId, theme, brand: params.get('brand') }), { fontsBase, stats: true })
  window.__ktableGridStats = view.stats
  return { view, engine }
}

function hud(view: GridView) {
  const panel = document.createElement('pre')
  panel.style.cssText = 'position:fixed;right:8px;bottom:8px;margin:0;padding:6px 8px;font:11px/1.4 monospace;background:rgba(0,0,0,.72);color:#fff;border-radius:8px;pointer-events:none;z-index:10'
  document.body.append(panel)
  const p95 = (values: number[]) => (values.length ? [...values].sort((a, b) => a - b)[Math.floor(values.length * 0.95)] : 0)
  setInterval(() => {
    const stats = view.stats
    if (!stats) return
    const recent = stats.frames.slice(-120)
    const gaps = recent.slice(1).map((frame, index) => frame.at - recent[index].at)
    const requests = stats.requests.slice(-60)
    panel.textContent = [
      `frame p95 ${p95(gaps).toFixed(1)} ms`,
      `play p95 ${p95(recent.map((frame) => frame.playMs)).toFixed(2)} ms`,
      `skeleton ${recent.filter((frame) => frame.skeleton).length}/${recent.length}`,
      `band rtt p95 ${p95(requests.map((request) => request.ms)).toFixed(1)} ms`,
      `cache ${view.cache.count} bands ${(view.cache.bytes / 1024).toFixed(0)} KB`,
    ].join('\n')
  }, 500)
}

start()
  .then(async ({ view, engine }) => {
    window.__ktableGrid = { view, engine }
    await view.open()
    document.body.dataset.ready = 'true'
    if (params.get('hud') === '1') hud(view)
  })
  .catch((error: unknown) => {
    document.body.dataset.error = String(error)
    console.error(error)
  })
