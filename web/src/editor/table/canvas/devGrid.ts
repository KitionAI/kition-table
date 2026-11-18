/**
 * The development page of the new grid (`dev/grid.html`, served by `pnpm dev`; not an entry of the
 * release build): opens a table file in the kit's engine and shows one view through `GridView`,
 * with its interaction (`GridInteraction`: selection, keyboard, hover, menus' entry points). The
 * performance, parity and interaction tests (`tests/perf.mjs`, `tests/parity.mjs`,
 * `tests/gridInteraction.mjs`) drive it.
 *
 * The page stands in for the editor where the grid hands over: undo and redo, adding a record and
 * collapsing a group run on the engine and arrange again; the record and column menus are a plain
 * marker (`[data-ktable-grid-menu]`) at the grid's anchor, closed by Escape or a press elsewhere;
 * every handler call is logged to `window.__ktableGridEvents`.
 *
 * Query: `file` (a .ktable URL, default the development sample), `table` and `view` (ids, default
 * the first table's first view), `theme` (`light` or `dark`), `brand` (`#rrggbb`), `hud=1` (a
 * small panel with frame and band timings, for looking at it by hand).
 *
 * `window.__ktableGrid` holds the view and the engine, `window.__ktableGridStats` the view's
 * timings: only this page has them. `body[data-ready]` is set once the axes are in.
 */
import { KitEngine } from '@kition/kit-editor'
import { type EngineSource, engineSource } from './engineSource'
import { engineFontUrls } from './gridFonts'
import { GridView } from './gridView'
import { type ClientRect, type GridHandlers, GridInteraction } from './interaction'

/** One handler call of the grid, as the development page logs it. */
export type GridEvent = { name: string; detail: unknown }

declare global {
  interface Window {
    __ktableGrid?: { view: GridView; engine: KitEngine; interaction: GridInteraction; source: EngineSource }
    __ktableGridStats?: GridView['stats']
    __ktableGridEvents?: GridEvent[]
  }
}

const root = new URL('../', document.baseURI)
const params = new URL(location.href).searchParams

async function start(): Promise<{ view: GridView; engine: KitEngine; source: EngineSource }> {
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
  const source = engineSource(engine, { tableId, viewId, theme, brand: params.get('brand') })
  const view = new GridView(host, source, { fontsBase, stats: true })
  window.__ktableGridStats = view.stats
  return { view, engine, source }
}

/** A marker standing in for the editor's menu (`kind`) at a client point. */
function showMenu(kind: string, at: { x: number; y: number }, detail: string) {
  closeMenu()
  const menu = document.createElement('div')
  menu.dataset.ktableGridMenu = kind
  menu.textContent = detail
  menu.style.cssText = `position:fixed;left:${at.x}px;top:${at.y}px;padding:4px 8px;font:12px sans-serif;background:#fff;border:1px solid #ccc;border-radius:8px;z-index:20`
  document.body.append(menu)
}

function closeMenu() {
  for (const menu of document.querySelectorAll('[data-ktable-grid-menu]')) menu.remove()
}

/** The page's stand-ins for the editor's handlers. */
function handlers(engine: KitEngine, tableId: number, view: () => GridView, source: () => EngineSource): GridHandlers {
  const events: GridEvent[] = []
  window.__ktableGridEvents = events
  const log = (name: string, detail: unknown = null) => events.push({ name, detail })
  const call = async (method: string, params: unknown) => {
    const answer = JSON.parse(await engine.invoke<string>('call', method, JSON.stringify(params))) as { error?: unknown }
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
  }
  const again = (name: string, run: () => Promise<void>) => {
    run()
      .then(() => view().reopen())
      .then(() => log(`${name}:done`))
      .catch((error: unknown) => log(`${name}:failed`, String(error)))
  }
  const below = (anchor: ClientRect) => ({ x: anchor.x, y: anchor.y + anchor.height })
  return {
    onEdit: (cell, text) => log('edit', { cell, text }),
    onDelete: (selection) => log('delete', selection),
    onUndo: () => {
      log('undo')
      again('undo', () => call('undo', {}))
    },
    onRedo: () => {
      log('redo')
      again('redo', () => call('redo', {}))
    },
    onContextMenu: (event) => {
      log('contextMenu', event)
      showMenu('record', { x: event.clientX, y: event.clientY }, `${event.recordIds.length} records`)
    },
    onColumnMenu: (event) => {
      log('columnMenu', event)
      showMenu('column', below(event.anchor), `field ${event.fieldId}`)
    },
    onExpandRecord: (recordId) => log('expand', recordId),
    onAppendRecord: (event) => {
      log('appendRecord', event)
      again('appendRecord', () => call('transact', { ops: [{ op: 'records.create', tableId, at: null, records: [{}] }] }))
    },
    onAppendField: (event) => {
      log('appendField', event)
      showMenu('field', below(event.anchor), 'new field')
    },
    onToggleGroup: (event) => {
      log('toggleGroup', event)
      const collapsed = new Set(source().target.collapsed ?? [])
      if (event.collapsed) collapsed.add(event.groupId)
      else collapsed.delete(event.groupId)
      source().update({ collapsed: [...collapsed] })
      again('toggleGroup', async () => {})
    },
    onCellPart: (hit) => log('cellPart', { row: hit.row, col: hit.col, kind: hit.part?.kind, index: hit.part?.index }),
  }
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
  .then(async ({ view, engine, source }) => {
    const interaction = new GridInteraction(view, handlers(engine, source.target.tableId, () => view, () => source))
    window.__ktableGrid = { view, engine, interaction, source }
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') closeMenu()
    })
    document.addEventListener('pointerdown', (event) => {
      if (!(event.target instanceof Element && event.target.closest('[data-ktable-grid-menu]'))) closeMenu()
    })
    await view.open()
    document.body.dataset.ready = 'true'
    if (params.get('hud') === '1') hud(view)
  })
  .catch((error: unknown) => {
    document.body.dataset.error = String(error)
    console.error(error)
  })
