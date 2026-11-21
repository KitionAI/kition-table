// The editor's flow in a real browser, through the development host: node tests/flow.mjs [chromium|webkit]
// Starts the Vite dev server itself, opens fixtures the built engine makes (public/engine) in
// dev.html, and prints PASS or FAIL per step. The steps port Kition's former table editor gate
// (e2e/kitable.spec.ts): its HTTP requests are now engine commands, read from the editor's engine
// worker (every `execute` it is sent, with its answer, and how many records each arrangement's
// axis shows) and from the engine's state (`ai.run`). The grid is the engine's (grid plan phase 5):
// it draws bands the engine lays out, so its text is read from the canvas's `fillText` calls.
// FLOW_ONLY=<regex> runs only the tests whose names match; FLOW_SHOTS=<folder> keeps a screenshot of each failure.
import fs from 'node:fs'
import { chromium, webkit } from 'playwright'
import { createServer } from 'vite'

const which = process.argv[2] ?? 'chromium'

// Fixtures: the gate's two-task table (Title, Score, Received At, a view "All tasks"), and the
// same table with 1,137 records.
const engineDir = new URL('../public/engine/', import.meta.url)
const sdk = await import(new URL('kition_table_kit.js', engineDir).href)
await sdk.default({ module_or_path: fs.readFileSync(new URL('kition_table_kit_bg.wasm', engineDir)) })
function fixture(name, count) {
  const doc = sdk.TableDocument.create(name, 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('documents.get').tables[0]
  call('tables.update', { tableId: table.id, title: 'Tasks' })
  call('views.update', { tableId: table.id, viewId: table.views[0].id, title: 'All tasks' })
  for (const field of table.fields.filter((field) => !field.is_primary)) call('fields.delete', { tableId: table.id, fieldId: field.id })
  call('fields.create', { tableId: table.id, title: 'Score', type: 'rating', options: { max: 5, icon: 'star' } })
  call('fields.create', { tableId: table.id, title: 'Received At', type: 'datetime' })
  call('records.create', { tableId: table.id, values: { title: 'Phase 2 wrap', score: 4, received_at: '2026-07-23T04:14:47Z' } })
  call('records.create', { tableId: table.id, values: { title: 'Phase 3 deferred', score: 0, received_at: '2026-07-24T05:15:48Z' } })
  for (let index = 2; index < count; index += 1) call('records.create', { tableId: table.id, values: { title: `Message ${String(index + 1).padStart(4, '0')}`, score: index % 6 } })
  doc.markSaved()
  const bytes = Buffer.from(doc.save()).toString('base64')
  doc.free()
  return bytes
}
const SMALL = fixture('Tasks.ktable', 2)
// Two tables and a dashboard, for the views a host opens at: { tables: [ids], dashboard: id, bytes }.
function viewsFixture() {
  const doc = sdk.TableDocument.create('Base.ktable', 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const first = call('documents.get').tables[0]
  call('tables.update', { tableId: first.id, title: 'Tasks' })
  call('records.create', { tableId: first.id, values: { title: 'Phase 2 wrap' } })
  const second = call('tables.create', { title: 'People' })
  call('dashboards.upsert', { dashboardId: 'overview', title: 'Overview', source_table_id: first.id, layout: [], widgets: [] })
  doc.markSaved()
  const bytes = Buffer.from(doc.save()).toString('base64')
  doc.free()
  return { tables: [first.id, second.id], dashboard: 'overview', bytes }
}
const VIEWS = viewsFixture()
// A table shown as a gallery, its cards' covers an SVG, a GIF (a green first frame, a magenta
// second) and a HEIC (the engine's tests' 32 x 16 picture, its left half red, its right half blue).
function coversFixture() {
  const doc = sdk.TableDocument.create('Covers.ktable', 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('documents.get').tables[0]
  call('views.update', { tableId: table.id, viewId: table.views[0].id, title: 'Covers', type: 'gallery' })
  const files = call('fields.create', { tableId: table.id, title: 'Files', type: 'attachment' })
  const name = (files.field ?? files).name
  const held = (fileName, mimeType, bytes) => call('attachments.put', { tableId: table.id, name: fileName, mimeType, data: Buffer.from(bytes).toString('base64') })
  const svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='rgb(40,170,60)'/></svg>"
  // An 8 x 4 GIF of two frames over a 4-color table; its LZW data sends a clear code (4) before
  // every pixel, so the codes stay 3 bits wide.
  const frame = (index) => {
    const codes = Array.from({ length: 32 }, () => [4, index]).flat().concat(5)
    const bytes = []
    let bits = 0
    let filled = 0
    for (const code of codes) {
      bits |= code << filled
      filled += 3
      for (; filled >= 8; filled -= 8, bits >>= 8) bytes.push(bits & 255)
    }
    if (filled) bytes.push(bits & 255)
    return [0x21, 0xf9, 4, 0, 10, 0, 0, 0, 0x2c, 0, 0, 0, 0, 8, 0, 4, 0, 0, 2, bytes.length, ...bytes, 0]
  }
  const gif = [...Buffer.from('GIF89a'), 8, 0, 4, 0, 0xf1, 0, 0, 40, 170, 60, 200, 20, 200, 0, 0, 0, 0, 0, 0, ...frame(0), ...frame(1), 0x3b]
  const heic = fs.readFileSync(new URL('../../crates/view/display/testdata/split.heic', import.meta.url))
  const covers = [held('drawn.svg', 'image/svg+xml', Buffer.from(svg)), held('moving.gif', 'image/gif', gif), held('photo.heic', 'image/heic', heic)]
  for (const [index, cover] of covers.entries()) call('records.create', { tableId: table.id, values: { title: `Cover ${index + 1}`, [name]: [cover] } })
  doc.markSaved()
  const bytes = Buffer.from(doc.save()).toString('base64')
  doc.free()
  return bytes
}
const COVERS = coversFixture()
const LARGE = fixture('Messages.ktable', 1137)

const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 0, host: '127.0.0.1' }, logLevel: 'error' })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await (which === 'webkit' ? webkit : chromium).launch()
// The date steps expect local time eight hours ahead of UTC, as the gate did.
const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, timezoneId: 'Asia/Shanghai' })
// WebKit knows no clipboard-write permission: writing needs only the key press's user gesture.
await context.grantPermissions(which === 'webkit' ? ['clipboard-read'] : ['clipboard-read', 'clipboard-write'], { origin })
// In every frame (the editor's iframe too): every `execute` the engine worker is sent, with its
// answer; and every text the canvas draws.
await context.addInitScript(() => {
  const calls = []
  window.__engineCalls = calls
  // How many records each axis the editor's arrangement answered shows (gridOpen, gridSync).
  const arranged = []
  window.__arranged = arranged
  const Base = window.Worker
  if (Base) {
    window.Worker = class extends Base {
      constructor(...args) {
        super(...args)
        const pending = new Map()
        this.__pending = pending
        this.addEventListener('message', (event) => {
          const data = event.data
          const call = data && pending.get(data.id)
          if (!call) return
          pending.delete(data.id)
          if (call.invoke) {
            // An arrangement's axis (format v2): how many records it shows is at byte 16.
            const value = data.value
            const bytes = value instanceof ArrayBuffer ? new Uint8Array(value) : ArrayBuffer.isView(value) ? new Uint8Array(value.buffer, value.byteOffset, value.byteLength) : null
            if (bytes && bytes.byteLength >= 32) arranged.push(new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(16, true))
            return
          }
          call.ok = data.ok === true
          if (call.ok) {
            try {
              const answer = JSON.parse(data.value?.result ?? 'null')
              call.result = answer?.result
              call.change = answer?.change ?? null
            } catch {
              call.result = data.value?.result
            }
          } else call.error = data.error
        })
      }
      postMessage(message, ...rest) {
        const request = message && message.request
        if (request && request.method === 'execute') {
          const call = { command: request.command, params: JSON.parse(JSON.stringify(request.params ?? {})) }
          calls.push(call)
          this.__pending.set(message.id, call)
        } else if (request && request.method === 'invoke' && (request.name === 'gridOpen' || request.name === 'gridSync')) {
          this.__pending.set(message.id, { invoke: request.name })
        }
        return super.postMessage(message, ...rest)
      }
    }
  }
  const draws = []
  window.__textDraws = draws
  const fillText = CanvasRenderingContext2D.prototype.fillText
  CanvasRenderingContext2D.prototype.fillText = function (text, x, y, maxWidth) {
    if (draws.length > 20000) draws.splice(0, 10000)
    draws.push({ text: String(text), textAlign: this.textAlign, y })
    return maxWidth === undefined ? fillText.call(this, text, x, y) : fillText.call(this, text, x, y, maxWidth)
  }
})
const page = await context.newPage()
const errors = []
let failures = 0
// The test running when a page error came, so a late report still names its step.
let running = 'opening the development host'
page.on('pageerror', (error) => errors.push(`${running}: ${error.message}`))
// Console errors (all frames) are not failures; a failed step shows the ones it saw.
const consoleErrors = []
page.on('console', (message) => {
  if (message.type() === 'error') consoleErrors.push(message.text())
})
const step = (name, ok, detail = '') => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}
const until = async (check, tries = 60) => {
  for (let i = 0; i < tries; i += 1) {
    if (await check().catch(() => false)) return true
    await page.waitForTimeout(100)
  }
  return false
}
/** Runs one test: an exception fails it and the next one still runs. */
async function test(name, body) {
  if (process.env.FLOW_ONLY && !new RegExp(process.env.FLOW_ONLY).test(name)) return
  running = name
  try {
    await body()
  } catch (error) {
    if (process.env.FLOW_SHOTS) await page.screenshot({ path: `${process.env.FLOW_SHOTS}/${name.replace(/\W+/g, '-')}.png` }).catch(() => {})
    step(name, false, error instanceof Error ? error.message.split('\n').slice(0, 3).join(' ') : String(error))
  }
}

const kit = page.frameLocator('#kit')
const frame = () => page.frames().find((candidate) => candidate !== page.mainFrame() && candidate.url().includes('index.html'))
const editor = () => kit.getByTestId('ktable-editor')
const stage = () => kit.locator('.data-inline-grid-canvas [data-ktable-grid-stage="true"]')
/** The grid's text editor: the focused textarea on the active cell, shown while a cell is edited. */
const cellEditor = () => kit.locator('.data-inline-grid-canvas textarea[data-ktable-grid-input]')
/** Waits for the cell editor to show (the textarea is on the grid, invisible, until a cell is edited). */
const editorShown = () => until(() => cellEditor().evaluate((element) => element.style.pointerEvents === 'auto'))
/**
 * Waits for the cell editor to hold `text`, the cell's value, which it reads from the engine after
 * it shows: a person sees the value before typing over it (what is typed before it arrives is
 * replaced by it, and the cell keeps its value).
 */
const editorHolds = (text) => until(async () => (await cellEditor().inputValue()) === text)
/** The engine commands the editor's worker was sent since `from` (a count from `mark()`), with their answers. */
const mark = async () => frame().evaluate(() => window.__engineCalls.length)
const sent = async (from, command) => (await frame().evaluate((from) => window.__engineCalls.slice(from), from)).filter((call) => call.ok !== undefined && (!command || call.command === command))
const writes = async (from) => (await sent(from)).filter((call) => /^table\.(transact|undo|redo|records\.(create|update|delete|move|bulk\w*)|cells\.\w+|fields\.\w+|views\.(update|patchField|create)|tables\.\w+)$/.test(call.command) && !/linkCandidates|listFields/.test(call.command))
/** Runs engine commands from the host (the agent's `ai/run`), as `origin` when given. */
const run = (steps) => page.evaluate(async (steps) => window.devHost.connection().ai.run(steps, false), steps)
const engine = async (method, params = {}) => {
  const answer = await run([{ command: `table.${method}`, params }])
  const result = answer.completed[0].result
  return (typeof result === 'string' ? JSON.parse(result) : result).result
}
const tables = async () => (await engine('documents.get')).tables
const records = async (tableId) => (await engine('records.list', { tableId })).items
const drew = (predicate) => frame().evaluate((source) => window.__textDraws.some(new Function('draw', `return (${source})(draw)`)), predicate.toString())
const modifier = () => frame().evaluate(() => (/mac/i.test(navigator.userAgent) && !/iphone|ipad|ipod/i.test(navigator.userAgent) ? 'Meta' : 'Control'))

/** Opens `bytes` (base64) in the development host, at `view` when given, and waits for the table editor and its records. */
async function open(name, bytes, view) {
  await page.evaluate(async ({ name, bytes, view }) => {
    const binary = atob(bytes)
    const data = new Uint8Array(binary.length)
    for (let index = 0; index < binary.length; index += 1) data[index] = binary.charCodeAt(index)
    await window.devHost.open(name, data.buffer, view ? { view } : {})
  }, { name, bytes, view })
  await page.waitForFunction((name) => document.getElementById('status')?.textContent?.startsWith(`${name} open in `), name, { timeout: 60000 })
  await editor().waitFor({ timeout: 30000 })
  await kit.locator('.data-inline-footer').filter({ hasText: /\d+ records/ }).waitFor({ timeout: 30000 })
  // The grid draws its first frame.
  await until(() => drew((draw) => draw.text === 'Phase 2 wrap'))
}

try {
  await page.goto(`${origin}/dev.html?locale=en-US`)
  await page.waitForFunction(() => document.getElementById('status')?.textContent?.includes(' open in '), null, { timeout: 60000 })
  // The sample's grid draws its first frame, which waits for Inter, before the first test opens
  // another file, as a person sees the page before choosing one: opening a file loads the editor's
  // frame again, and WebKit reports the fetch of Inter it stops then as a page error ("cannot load
  // .../fonts/inter/Inter-Variable.ttf due to access control checks").
  await until(() => drew((draw) => draw.text === 'Write the brief'), 600)

  // 1. Opens the file and mounts the editor's shell.
  await test('opens the table and mounts the editor', async () => {
    await open('Tasks.ktable', SMALL)
    const counts = await Promise.all([
      kit.locator('.data-inline-title-block').count(),
      kit.locator('.data-inline-topbar .data-inline-icon-button').count(),
      kit.getByRole('button', { name: 'Grid view' }).count(),
      kit.getByRole('button', { name: 'All tasks' }).count(),
      kit.locator('.data-inline-view-tabs').count(),
    ])
    step('opens the table and mounts the editor', counts.join() === '0,0,1,0,1', counts.join())
  })

  // 3. Every grid toolbar control has its marker.
  await test('marks every grid toolbar control', async () => {
    const markers = ['filter', 'sort', 'group', 'row-height', 'freeze', 'search']
    const visible = await Promise.all(markers.map((marker) => kit.getByTestId(`ktable-toolbar-${marker}`).isVisible()))
    step('marks every grid toolbar control', visible.every(Boolean), markers.filter((_, index) => !visible[index]).join())
  })

  // 4. Filter opens its menu and closes on a second click.
  await test('Filter opens and closes on a second click', async () => {
    const before = await kit.getByTestId('ktable-toolbar-filter-menu').count()
    await kit.getByTestId('ktable-toolbar-filter').click()
    const opened = await until(() => kit.getByTestId('ktable-toolbar-filter-menu').isVisible())
    await kit.getByTestId('ktable-toolbar-filter').click()
    const closed = await until(async () => (await kit.getByTestId('ktable-toolbar-filter-menu').count()) === 0)
    step('Filter opens and closes on a second click', before === 0 && opened && closed)
  })

  // 5. Sort and Group open their menus; Escape closes.
  await test('Sort and Group open their menus', async () => {
    await kit.getByTestId('ktable-toolbar-sort').click()
    const sort = await until(() => kit.getByTestId('ktable-toolbar-sort-menu').isVisible())
    await page.keyboard.press('Escape')
    const closed = await until(async () => (await kit.getByTestId('ktable-toolbar-sort-menu').count()) === 0)
    await kit.getByTestId('ktable-toolbar-group').click()
    const group = await until(() => kit.getByTestId('ktable-toolbar-group-menu').isVisible())
    await page.keyboard.press('Escape')
    await until(async () => (await kit.getByTestId('ktable-toolbar-group-menu').count()) === 0)
    step('Sort and Group open their menus, Escape closes', sort && closed && group)
  })

  // 6. A sort rule's controls stay inside the popover.
  await test('keeps the sort rule inside the popover', async () => {
    await kit.getByTestId('ktable-toolbar-sort').click()
    const menu = kit.getByTestId('ktable-toolbar-sort-menu')
    await menu.getByRole('button', { name: 'Add another sort' }).click()
    const row = menu.locator('.data-inline-sort-row').first()
    const field = row.locator('.data-inline-sort-field')
    const direction = row.locator('.data-inline-sort-direction')
    const remove = row.getByRole('button', { name: 'Remove sort' })
    await field.locator('select[aria-label="Sort field"]').selectOption('received_at')
    await direction.locator('select[aria-label="Sort direction"]').selectOption('desc')
    const label = await direction.locator('select[aria-label="Sort direction"]').evaluate((select) => select.selectedOptions[0]?.text ?? '')
    const [menuBox, rowBox, fieldBox, directionBox, removeBox] = await Promise.all([menu, row, field, direction, remove].map((part) => part.boundingBox()))
    const inside = rowBox.x >= menuBox.x && removeBox.x + removeBox.width <= menuBox.x + menuBox.width && fieldBox.width > 80 && directionBox.width > 120
    step('keeps the sort rule inside the popover', inside && label === 'Latest → Earliest', JSON.stringify({ label, menuBox, rowBox, removeBox, field: fieldBox.width, direction: directionBox.width }))
    await remove.click()
    await page.keyboard.press('Escape')
  })

  // 7. Search takes typing.
  await test('search takes typing', async () => {
    const search = kit.getByTestId('ktable-toolbar-search')
    const empty = (await search.inputValue()) === ''
    await search.fill('phase 2')
    step('search takes typing', empty && (await search.inputValue()) === 'phase 2')
    await search.fill('')
  })

  // 7b. The engine arranges the view (kition-table-arrange): the search keeps the records that hold
  // it, any field, case ignored, and clearing it shows them all again.
  await test('the search keeps the records that hold it', async () => {
    const search = kit.getByTestId('ktable-toolbar-search')
    // The footer's first part is the count; the edit hint follows it with no space between in textContent.
    const footer = kit.locator('.data-inline-footer > span').first()
    // "1 record", "2 records" (the footer's plural, regressions/issue-footer-count-plural).
    const shows = (count) => until(async () => new RegExp(`(^|\\D)${count} ${count === 1 ? 'record\\b' : 'records'}`).test(await footer.textContent()))
    await search.fill('PHASE')
    const both = await shows(2)
    await search.fill('deferred')
    const one = await shows(1)
    await search.fill('nothing like it')
    const none = await shows(0)
    await search.fill('')
    const all = await shows(2)
    step('the search keeps the records that hold it', both && one && none && all, JSON.stringify({ both, one, none, all }))
  })

  // 11. Editing a text cell shows one brand-purple border.
  await test('a text cell editor has one brand border', async () => {
    await stage().click({ position: { x: 145, y: 48 } })
    await stage().dblclick({ position: { x: 145, y: 48 } })
    const input = cellEditor()
    await editorShown()
    const focused = await input.evaluate((element) => element === document.activeElement)
    const style = await input.evaluate((element) => {
      const computed = getComputedStyle(element)
      return { borderColor: computed.borderColor, boxShadow: computed.boxShadow }
    })
    step('a text cell editor has one brand border', focused && style.borderColor === 'rgb(12, 131, 117)' && style.boxShadow === 'none', JSON.stringify(style))
    await page.keyboard.press('Escape')
  })

  // 12. Content is centred, dates are formatted, and Copy goes to the system clipboard.
  await test('copies the selected cell to the system clipboard', async () => {
    // The first row's middle is at 52 (a 32 px header, 40 px rows); the engine puts the text's
    // alphabetic baseline below it by half of Inter's ascent less its descent (the face the grid
    // draws with), as a browser centres a line box.
    const below = await frame().evaluate(() => {
      const metrics = Object.assign(document.createElement('canvas').getContext('2d'), { font: '13px KitionTableSans' }).measureText('x')
      return (metrics.fontBoundingBoxAscent - metrics.fontBoundingBoxDescent) / 2
    })
    const centred = await until(() => frame().evaluate((baseline) => window.__textDraws.some((draw) => draw.text === 'Phase 2 wrap' && draw.textAlign === 'center' && Math.abs(draw.y - baseline) < 0.75), 52 + below))
    step('draws grid content centred', centred, JSON.stringify({ baseline: 52 + below, drawn: await frame().evaluate(() => window.__textDraws.filter((draw) => draw.text === 'Phase 2 wrap').slice(-2)) }))
    const dates = await frame().evaluate(() => {
      const raw = '2026-07-23T04:14:47Z'
      const date = new Date(raw)
      const pad = (value) => String(value).padStart(2, '0')
      const expected = `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
      return { formatted: window.__textDraws.some((draw) => draw.text === expected), raw: window.__textDraws.some((draw) => draw.text === raw), expected }
    })
    step('draws dates formatted, not raw', dates.formatted && !dates.raw && dates.expected === '2026/07/23 12:14', JSON.stringify(dates))
    await stage().hover({ position: { x: 145, y: 48 } })
    await stage().click({ position: { x: 145, y: 48 } })
    const seen = consoleErrors.length
    await page.keyboard.press(`${await modifier()}+c`)
    let copied = ''
    if (which === 'webkit') {
      // Playwright's WebKit on Linux answers navigator.clipboard.readText() with nothing whatever
      // the clipboard holds: the browser's own paste shows what it holds, as a person pastes it
      // somewhere else (a text box of the host page, outside the editor).
      const key = await modifier()
      await until(async () => {
        await page.evaluate(() => {
          const probe = document.createElement('textarea')
          probe.id = 'clipboard-probe'
          document.body.append(probe)
          probe.focus()
        })
        await page.keyboard.press(`${key}+v`)
        copied = await page.evaluate(() => {
          const probe = document.getElementById('clipboard-probe')
          probe.remove()
          return probe.value
        })
        return copied === 'Phase 2 wrap'
      })
    } else {
      await until(async () => (copied = await page.evaluate(() => navigator.clipboard.readText())) === 'Phase 2 wrap')
    }
    step('copies the selected cell to the system clipboard', copied === 'Phase 2 wrap', JSON.stringify({ copied, consoleErrors: consoleErrors.slice(seen, seen + 3) }))
  })

  // 14. Undo and Redo keys run the engine's undo and redo.
  await test('Undo and Redo keys run the engine', async () => {
    const disabled = (await kit.getByTestId('table-undo').isDisabled()) && (await kit.getByTestId('table-redo').isDisabled())
    step('Undo and Redo start disabled', disabled)
    const [table] = await tables()
    const first = (await records(table.id))[0]
    // An edit to undo: type over the first title.
    await stage().click({ position: { x: 145, y: 48 } })
    await stage().dblclick({ position: { x: 145, y: 48 } })
    const input = cellEditor()
    const shown = (await editorShown()) && (await editorHolds(first.values.title))
    await input.fill('Phase 2 renamed')
    const typed = await input.inputValue()
    await page.keyboard.press('Enter')
    await until(async () => (await records(table.id))[0].values.title === 'Phase 2 renamed')
    const titles = (await records(table.id)).map((record) => record.values.title)
    step('types into a cell', titles[0] === 'Phase 2 renamed', JSON.stringify({ shown, typed, titles }))
    await until(() => kit.getByTestId('table-undo').isEnabled())
    await stage().click({ position: { x: 145, y: 88 } })
    const from = await mark()
    const key = await modifier()
    await page.keyboard.press(`${key}+z`)
    await until(async () => (await sent(from, 'table.undo')).length === 1)
    await until(async () => (await records(table.id))[0].values.title === first.values.title)
    const undone = (await records(table.id))[0].values.title
    step('Cmd/Ctrl+Z runs one engine undo', (await sent(from, 'table.undo')).length === 1 && undone === 'Phase 2 wrap', undone)
    await page.keyboard.press(`${key}+Shift+z`)
    await until(async () => (await sent(from, 'table.redo')).length === 1)
    await until(async () => (await records(table.id))[0].values.title === 'Phase 2 renamed')
    const redone = (await records(table.id))[0].values.title
    step('Cmd/Ctrl+Shift+Z runs one engine redo', (await sent(from, 'table.redo')).length === 1 && redone === 'Phase 2 renamed', redone)
  })

  // 13. Date cells open a compact calendar and time menu.
  await test('opens a date cell in a compact calendar', async () => {
    await open('Tasks.ktable', SMALL)
    const [table] = await tables()
    const first = (await records(table.id))[0]
    await stage().hover({ position: { x: 545, y: 48 } })
    await stage().click({ position: { x: 545, y: 48 } })
    const dateEditor = kit.getByTestId('table-date-cell-editor')
    const calendar = kit.getByTestId('table-date-calendar')
    await calendar.waitFor({ timeout: 5000 })
    const text = await dateEditor.textContent()
    const box = await calendar.boundingBox()
    const native = await kit.locator('input[type="datetime-local"]').count()
    step('opens a date cell in a compact calendar', text.includes('2026-07-23') && text.includes('12:14') && native === 0 && box.width <= 290 && box.height <= 390, JSON.stringify({ text, box }))
    const next = await frame().evaluate(() => {
      const date = new Date('2026-07-23T04:14:47Z')
      date.setDate(date.getDate() + 1)
      const pad = (value) => String(value).padStart(2, '0')
      return { key: `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`, stored: date.toISOString() }
    })
    const from = await mark()
    await calendar.locator(`[data-date="${next.key}"]`).click()
    await until(async () => (await calendar.count()) === 0 || !(await calendar.isVisible()))
    await until(async () => (await writes(from)).length > 0)
    const made = await writes(from)
    const stored = (await records(table.id)).find((record) => record.id === first.id).values.received_at
    const one = made.length === 1 && made[0].command === 'table.records.update' && made[0].params.values?.received_at === next.stored && made[0].ok
    step('picking a day writes one records.update', one && stored === next.stored, JSON.stringify({ made: made.map((call) => [call.command, call.params.values]), stored }))
    await stage().click({ position: { x: 545, y: 48 } })
    await calendar.waitFor({ timeout: 5000 })
    await dateEditor.getByTestId('table-date-time-trigger').click()
    const menu = kit.getByTestId('table-date-time-menu')
    await menu.waitFor({ timeout: 5000 })
    const menuBox = await menu.boundingBox()
    step('opens a compact time menu', (await menu.textContent()).includes('12:14') && menuBox.width <= 100 && menuBox.height <= 250, JSON.stringify(menuBox))
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
  })

  // 15. A 3x3 paste over two rows and past the end is one engine transact.
  await test('pastes a 3x3 block as one transact', async () => {
    await open('Tasks.ktable', SMALL)
    const [table] = await tables()
    await stage().click({ position: { x: 145, y: 48 } })
    const from = await mark()
    const tsv = ['a1\tb1\t2026-01-01', 'a2\tb2\t2026-01-02', 'a3\tb3\t2026-01-03'].join('\n')
    await frame().evaluate((text) => {
      const clipboardData = new DataTransfer()
      clipboardData.setData('text/plain', text)
      window.dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }))
    }, tsv)
    await until(async () => (await writes(from)).length > 0)
    await page.waitForTimeout(300)
    const made = await writes(from)
    const ops = made[0]?.params.ops?.map((op) => op.op) ?? []
    step('pastes a 3x3 block as one engine transact', made.length === 1 && made[0].command === 'table.transact' && made[0].ok && ops.join() === 'cells.set,records.create', JSON.stringify(made.map((call) => [call.command, call.params.ops?.map((op) => op.op), call.error])))
    const titles = (await records(table.id)).map((record) => record.values.title)
    step('the pasted rows are in the table', titles.join() === 'a1,a2,a3', titles.join())
  })

  // 16. Dragging a row onto the first is one records.move.
  await test('drags a row as one records.move', async () => {
    await open('Tasks.ktable', SMALL)
    const [table] = await tables()
    const [one, two] = await records(table.id)
    const box = await stage().boundingBox()
    const from = await mark()
    await page.mouse.move(box.x + 12, box.y + 92)
    await page.mouse.down()
    await page.mouse.move(box.x + 12, box.y + 52, { steps: 8 })
    await page.mouse.up()
    await until(async () => (await writes(from)).length > 0)
    await page.waitForTimeout(300)
    const made = await writes(from)
    const expected = JSON.stringify([{ op: 'records.move', tableId: table.id, rowKeys: [two.row_key], before: one.row_key }])
    step('drags a row as one engine transact with records.move', made.length === 1 && made[0].command === 'table.transact' && JSON.stringify(made[0].params.ops) === expected && made[0].ok, JSON.stringify(made.map((call) => [call.command, call.params])))
    const order = (await records(table.id)).map((record) => record.values.title)
    step('the row moved', order.join() === 'Phase 3 deferred,Phase 2 wrap', order.join())
  })

  // 10. Dragging a column header is one fields.move.
  await test('drags a column as one fields.move', async () => {
    await open('Tasks.ktable', SMALL)
    const [table] = await tables()
    const score = table.fields.find((field) => field.name === 'score')
    const box = await stage().boundingBox()
    const from = await mark()
    await page.mouse.move(box.x + 345, box.y + 16)
    await page.mouse.down()
    await page.mouse.move(box.x + 85, box.y + 16, { steps: 8 })
    await page.mouse.up()
    await until(async () => (await writes(from)).length > 0)
    await page.waitForTimeout(300)
    const made = await writes(from)
    const expected = JSON.stringify([{ op: 'fields.move', tableId: table.id, fieldIds: [score.id], toIndex: 0 }])
    step('drags a column as one engine transact with fields.move', made.length === 1 && made[0].command === 'table.transact' && JSON.stringify(made[0].params.ops) === expected && made[0].ok, JSON.stringify(made.map((call) => [call.command, call.params])))
    const order = (await tables())[0].fields.map((field) => field.name)
    step('the column moved', order[0] === 'score', order.join())
  })

  // 9. Dragging a header boundary resizes the column and saves its width.
  await test('resizes a column and saves the width', async () => {
    await open('Tasks.ktable', SMALL)
    const [table] = await tables()
    const title = table.fields.find((field) => field.is_primary)
    const box = await stage().boundingBox()
    const cursor = () => stage().evaluate(async (node) => {
      await new Promise((resolve) => requestAnimationFrame(() => resolve()))
      return node.style.cursor
    })
    // Three 24px row controls, and every field starts 200px wide.
    const expected = box.x + 72 + 200
    const y = box.y + 16
    let boundary = null
    for (let delta = -12; delta <= 12 && boundary == null; delta += 1) {
      await page.mouse.move(expected + delta, y)
      if ((await cursor()) === 'ew-resize') boundary = expected + delta
    }
    step('finds the column boundary', boundary != null)
    const from = await mark()
    await page.mouse.down()
    await page.mouse.move(boundary + 80, y, { steps: 8 })
    await page.mouse.up()
    await until(async () => (await writes(from)).length > 0)
    await page.waitForTimeout(300)
    const made = await writes(from)
    // The width is saved once, on release.
    const saved = made.length >= 1 && made.every((call) => call.command === 'table.views.patchField' && call.params.fieldId === title.id && call.ok) && made.at(-1).params.width === 280
    step('saves the width with views.patchField', saved, JSON.stringify(made.map((call) => [call.command, call.params])))
    await page.mouse.move(boundary + 80, y)
    step('the new boundary resizes', await until(async () => (await cursor()) === 'ew-resize'))
  })

  // 8. The horizontal scrollbar lines up with the grid.
  await test('aligns the horizontal scrollbar with the grid', async () => {
    await page.setViewportSize({ width: 720, height: 720 })
    await open('Tasks.ktable', SMALL)
    const canvas = kit.locator('.data-inline-grid-canvas')
    // The grid's scroll container is the stage: its scroll bars are the grid's own.
    const scroller = canvas.locator('[data-ktable-grid-stage="true"]')
    await scroller.waitFor({ timeout: 5000 })
    const [canvasBox, scrollerBox] = await Promise.all([canvas.boundingBox(), scroller.boundingBox()])
    const range = await scroller.evaluate((element) => ({ maximum: element.scrollWidth - element.clientWidth, scrollLeft: element.scrollLeft }))
    const aligned = Math.abs(scrollerBox.x - canvasBox.x) <= 1 && Math.abs(scrollerBox.width - canvasBox.width) <= 1
    step('aligns the horizontal scrollbar with the grid', aligned && range.maximum > 0 && range.scrollLeft === 0, JSON.stringify({ canvasBox, scrollerBox, range }))
    await page.setViewportSize({ width: 1440, height: 900 })
  })

  // 8b. At 768 px (the kit's width in a 1024 px Kition window, beside its sidebar) the property
  // bar keeps "Search records" in view, and Undo and Redo are the editor frame's alone (the bar
  // had a second pair beside them).
  await test('keeps search in view at 768 and shows one Undo and Redo', async () => {
    await page.setViewportSize({ width: 768, height: 768 })
    await open('Tasks.ktable', SMALL)
    const search = kit.getByTestId('ktable-toolbar-search')
    await search.waitFor({ timeout: 5000 })
    const box = await search.boundingBox()
    const inView = box !== null && box.x >= 0 && box.x + box.width <= 768 && box.width >= 60
    step('search records stays in view at 768 px', inView, JSON.stringify(box))
    const undo = { frame: await kit.getByTestId('table-undo').count(), bar: await kit.getByTestId('ktable-toolbar-undo').count() }
    const redo = { frame: await kit.getByTestId('table-redo').count(), bar: await kit.getByTestId('ktable-toolbar-redo').count() }
    step('one Undo and one Redo, the frame\'s', undo.frame === 1 && undo.bar === 0 && redo.frame === 1 && redo.bar === 0, JSON.stringify({ undo, redo }))
    await page.setViewportSize({ width: 1440, height: 900 })
  })

  // 2. The grid reads no records: the engine arranges every record of the table (the axis holds
  // all 1,137) and the grid draws them from its bands; the footer counts them, and a sort keeps
  // every one.
  await test('arranges the whole table without reading it', async () => {
    await open('Messages.ktable', LARGE)
    const reads = (await sent(0, 'table.records.list')).filter((call) => call.ok)
    const footer = kit.locator('.data-inline-footer')
    const shown = async () => (await frame().evaluate(() => window.__arranged)).at(-1)
    step('arranges the whole table without reading it', reads.length === 0 && (await shown()) === 1137 && (await footer.textContent()).includes('1137 records'), JSON.stringify({ reads: reads.map((call) => call.params), axis: await shown() }))
    await kit.getByTestId('ktable-toolbar-sort').click()
    await kit.getByTestId('ktable-toolbar-sort-menu').getByRole('button', { name: 'Add another sort' }).click()
    await until(async () => (await kit.getByTestId('ktable-toolbar-sort').textContent()).includes('Sort (1)'))
    const label = await kit.getByTestId('ktable-toolbar-sort').textContent()
    await page.keyboard.press('Escape')
    await page.waitForTimeout(300)
    const sorted = await shown()
    step('a sort keeps every record', label.includes('Sort (1)') && sorted === 1137 && (await footer.textContent()).includes('1137 records') && (await sent(0, 'table.records.list')).length === 0, `${label} / ${await footer.textContent()} / ${sorted}`)
  })

  // Changes made through the engine from outside the editor (the agent, a host feature) show live.
  await test('shows changes made outside the editor', async () => {
    await open('Tasks.ktable', SMALL)
    const [table] = await tables()
    let reloaded = false
    const editorFrame = frame()
    page.on('framenavigated', (navigated) => {
      if (navigated === editorFrame) reloaded = true
    })
    await engine('records.update', { tableId: table.id, recordId: (await records(table.id))[0].id, values: { title: 'Changed remotely' } })
    const agent = await until(async () => (await drew((draw) => draw.text === 'Changed remotely')) && (await kit.locator('.data-inline-footer').textContent()).includes('2 records'))
    step('an agent edit appears in the open grid', agent && !reloaded)
    // The person's own edit, then a workflow's record (a system origin).
    await stage().click({ position: { x: 145, y: 88 } })
    await stage().dblclick({ position: { x: 145, y: 88 } })
    const input = cellEditor()
    const second = (await records(table.id))[1].values.title
    await editorShown()
    await editorHolds(second)
    await input.fill('Mine')
    await page.keyboard.press('Enter')
    await until(async () => (await records(table.id)).some((record) => record.values.title === 'Mine'))
    const answer = await engine('records.create', { tableId: table.id, values: { title: 'From a workflow' }, asOrigin: 'system:workflow' })
    const workflow = await until(async () => (await drew((draw) => draw.text === 'From a workflow')) && (await kit.locator('.data-inline-footer').textContent()).includes('3 records'))
    step('a workflow-created record appears live in the open table', workflow && !reloaded && answer?.id != null)
    const since = await engine('changes.since', { version: '' })
    step('the engine reports the workflow write as system:workflow', since.created.some((change) => change.recordId === answer.id && change.origin === 'system:workflow'), JSON.stringify(since.created))
    // The person's Undo reverts their own edit, never the workflow's record.
    await stage().click({ position: { x: 145, y: 48 } })
    const from = await mark()
    await page.keyboard.press(`${await modifier()}+z`)
    await until(async () => (await sent(from, 'table.undo')).length === 1)
    await until(async () => !(await records(table.id)).some((record) => record.values.title === 'Mine'))
    const titles = (await records(table.id)).map((record) => record.values.title)
    step("the person's Undo does not undo the workflow's write", titles.includes('From a workflow') && !titles.includes('Mine') && titles.includes('Phase 3 deferred'), titles.join())
  })

  // Adding a table from a template in the bar's tabs.
  await test('adds a table from a template', async () => {
    const before = await tables()
    await kit.getByTestId('table-add').click()
    await kit.getByTestId('table-template-library-dialog').waitFor({ timeout: 10000 })
    // A card shows its Preview and Use buttons on hover.
    const card = kit.getByTestId('table-template-card-simple-client-crm')
    await card.hover()
    await card.getByRole('button', { name: 'Use', exact: true }).click()
    await until(async () => (await tables()).length > before.length, 100)
    const after = await tables()
    const added = after.find((table) => !before.some((old) => old.id === table.id))
    await until(async () => (await kit.getByTestId('table-template-library-dialog').count()) === 0)
    const tab = added ? kit.getByTestId(`table-tab-${added.id}`) : null
    const current = tab ? await tab.getAttribute('aria-current') : null
    step('adds a table from a template and shows it', Boolean(added) && current === 'page', JSON.stringify(after.map((table) => table.title)))
  })

  // Importing a CSV file as a new table through the import dialog.
  await test('imports a CSV file as a new table', async () => {
    const before = await tables()
    const from = await mark()
    await kit.locator('[data-testid="table-tabs"] input[type="file"]').setInputFiles({ name: 'People.csv', mimeType: 'text/csv', buffer: Buffer.from('Name,Points\nAda,10\nGrace,12\n') })
    const title = kit.getByTestId('table-import-new-title')
    await title.waitFor({ timeout: 10000 })
    await title.fill('People')
    await kit.getByRole('button', { name: 'Import', exact: true }).click()
    await until(async () => (await tables()).length > before.length, 100)
    const added = (await tables()).find((table) => !before.some((old) => old.id === table.id))
    const rows = added ? (await records(added.id)).map((record) => Object.values(record.values).join(':')) : []
    const commands = (await sent(from)).map((call) => call.command).filter((command) => command.startsWith('table.import.'))
    step('imports a CSV file as a new table', added?.title === 'People' && rows.length === 2 && rows.join().includes('Ada') && commands.join() === 'table.import.preview,table.import.run', JSON.stringify({ title: added?.title, rows, commands }))
    const current = added ? await kit.getByTestId(`table-tab-${added.id}`).getAttribute('aria-current') : null
    step('shows the imported table', current === 'page')
  })

  // 17. Active table controls stay readable in dark mode.
  // The host opens the file at one of its tables (`document.view`), then reveals its dashboard
  // and another table (`document/reveal`): the tree's table and dashboard children.
  await test('opens at the view the host names and reveals others', async () => {
    const [tasks, people] = VIEWS.tables
    const current = (testId) => kit.getByTestId(testId).getAttribute('aria-current')
    await page.evaluate(async ({ bytes, view }) => {
      const binary = atob(bytes)
      const data = new Uint8Array(binary.length)
      for (let index = 0; index < binary.length; index += 1) data[index] = binary.charCodeAt(index)
      await window.devHost.open('Base.ktable', data.buffer, { view })
    }, { bytes: VIEWS.bytes, view: { path: `tables/${people}` } })
    await page.waitForFunction(() => document.getElementById('status')?.textContent?.startsWith('Base.ktable open in '), null, { timeout: 60000 })
    const opened = await until(async () => (await current(`table-tab-${people}`)) === 'page')
    const dashboardTab = await kit.getByTestId(`dashboard-tab-${VIEWS.dashboard}`).isVisible()
    await page.evaluate((view) => window.devHost.connection().reveal(view), { path: `dashboards/${VIEWS.dashboard}` })
    const dashboard = await until(async () => (await current(`dashboard-tab-${VIEWS.dashboard}`)) === 'page' && (await kit.getByTestId('dashboard-editor-pane').isVisible()))
    await page.evaluate((view) => window.devHost.connection().reveal(view), { path: `tables/${tasks}` })
    const table = await until(async () => (await current(`table-tab-${tasks}`)) === 'page' && (await editor().isVisible()))
    // A view the kit does not know changes nothing.
    await page.evaluate(() => window.devHost.connection().reveal({ path: 'records/1' }))
    await page.waitForTimeout(200)
    const kept = (await current(`table-tab-${tasks}`)) === 'page'
    step('opens at the view the host names and reveals others', opened && dashboardTab && dashboard && table && kept, JSON.stringify({ opened, dashboardTab, dashboard, table, kept }))
  })

  await test('keeps active controls readable in dark mode', async () => {
    await open('Tasks.ktable', SMALL)
    await page.evaluate(() => window.devHost.setScheme('dark'))
    const dark = await until(() => frame().evaluate(() => document.documentElement.classList.contains('dark')))
    step('follows the host into dark mode', dark)
    const freeze = kit.getByTestId('ktable-toolbar-freeze')
    await freeze.click()
    await kit.getByTestId('ktable-toolbar-freeze-menu').getByRole('radio').nth(1).check()
    await until(async () => /is-active/.test(await freeze.getAttribute('class')))
    const read = () => frame().evaluate(() => {
      const read = (selector) => {
        const style = getComputedStyle(document.querySelector(selector))
        return { background: style.backgroundColor, foreground: style.color }
      }
      const root = getComputedStyle(document.documentElement)
      return {
        primary: root.getPropertyValue('--primary').trim(),
        primaryForeground: root.getPropertyValue('--primary-foreground').trim(),
        brand: root.getPropertyValue('--brand').trim(),
        brandForeground: root.getPropertyValue('--brand-foreground').trim(),
        activeView: read('.data-inline-view-tab.is-active'),
        freeze: read('[data-testid="ktable-toolbar-freeze"]'),
        addRecord: read('.data-inline-add-record-button'),
      }
    })
    // Add record is a Tessera primary button: the Table kit's brand (kit.json ui.brand) for the scheme.
    const expected = {
      primary: '172.603 61.345% 53.333%',
      primaryForeground: '0 0% 0%',
      brand: '172.603 61.345% 53.333%',
      brandForeground: '0 0% 0%',
      activeView: { background: 'rgba(63, 209, 191, 0.2)', foreground: 'rgb(63, 209, 191)' },
      freeze: { background: 'rgba(63, 209, 191, 0.2)', foreground: 'rgb(63, 209, 191)' },
      addRecord: { background: 'rgb(63, 209, 191)', foreground: 'rgb(0, 0, 0)' },
    }
    // The controls' colors ease in (transition-colors): read them once they settle.
    let colors = null
    await until(async () => JSON.stringify((colors = await read())) === JSON.stringify(expected), 30)
    step('keeps active controls readable in dark mode', /is-active/.test(await freeze.getAttribute('class')) && JSON.stringify(colors) === JSON.stringify(expected), JSON.stringify(colors))
    await page.evaluate(() => window.devHost.setScheme('light'))
  })

  // Gallery covers are decoded as the grid's thumbnails are: a GIF shows its first frame, an SVG
  // (which no browser's createImageBitmap reads) is drawn by the engine, and a HEIC shows in every
  // browser (WebKit decodes it, the engine where the browser cannot).
  await test("a gallery card's cover is decoded as the grid's thumbnails", async () => {
    await page.evaluate(async (bytes) => {
      const binary = atob(bytes)
      const data = new Uint8Array(binary.length)
      for (let index = 0; index < binary.length; index += 1) data[index] = binary.charCodeAt(index)
      await window.devHost.open('Covers.ktable', data.buffer, {})
    }, COVERS)
    await page.waitForFunction(() => document.getElementById('status')?.textContent?.startsWith('Covers.ktable open in '), null, { timeout: 60000 })
    const covers = kit.locator('.data-inline-gallery canvas[data-ktable-gallery-cover]')
    const drawn = await until(async () => (await kit.locator('.data-inline-gallery canvas[data-ktable-gallery-cover="drawn"]').count()) === 3, 100)
    /** The cover's pixel at (`fx`, `fy`) of its picture. */
    const pixel = (index, fx, fy) => covers.nth(index).evaluate((canvas, [fx, fy]) => [...canvas.getContext('2d').getImageData(Math.floor(canvas.width * fx), Math.floor(canvas.height * fy), 1, 1).data], [fx, fy])
    const near = (a, b) => a.length === 4 && b.every((value, i) => Math.abs(a[i] - value) <= 24)
    const svg = await pixel(0, 0.5, 0.5)
    const gif = await pixel(1, 0.5, 0.5)
    await page.waitForTimeout(300)
    const gifLater = await pixel(1, 0.5, 0.5)
    const heic = [await pixel(2, 0.25, 0.5), await pixel(2, 0.75, 0.5)]
    const sizes = await covers.evaluateAll((canvases) => canvases.map((canvas) => [canvas.width, canvas.height]))
    step(
      "a gallery card's cover is decoded as the grid's thumbnails",
      drawn && near(svg, [40, 170, 60, 255]) && near(gif, [40, 170, 60, 255]) && near(gifLater, [40, 170, 60, 255]) && near(heic[0], [220, 30, 30, 255]) && near(heic[1], [30, 30, 220, 255]),
      JSON.stringify({ drawn, svg, gif, gifLater, heic, sizes }),
    )
  })

  // Display modes: the editor draws its own footer, so the frame draws no status bar under it, and
  // the footer says the document is unsaved; Focus shows the records alone (no tables bar, views
  // bar, toolbar, or footer), and Escape leaves it.
  await test('Focus shows the records alone', async () => {
    await open('Tasks.ktable', SMALL)
    const app = kit.locator('.kui-app')
    step('draws no frame status bar under the footer', (await kit.locator('.kui-status-bar').count()) === 0)
    await engine('records.create', { tableId: (await tables())[0].id, values: { title: 'Unsaved row' } })
    const shown = await until(async () => (await kit.locator('.data-inline-footer [data-testid="table-unsaved"], .data-inline-footer [data-testid="table-agent-edited"]').count()) > 0)
    step('the footer shows the document state the status bar showed', shown)
    await app.focus()
    await page.keyboard.press('ControlOrMeta+Backslash')
    await until(async () => (await app.getAttribute('data-mode')) === 'focus')
    const bars = {
      tables: await kit.getByTestId('table-tabs').count(),
      views: await kit.locator('.data-inline-view-tabs').count(),
      toolbar: await kit.getByTestId('ktable-toolbar-search').count(),
      footer: await kit.locator('.data-inline-footer').count(),
      grid: await stage().count(),
    }
    step('Focus shows the records alone', (await app.getAttribute('data-mode')) === 'focus' && JSON.stringify(bars) === JSON.stringify({ tables: 0, views: 0, toolbar: 0, footer: 0, grid: 1 }), JSON.stringify(bars))
    // Escape that closes a menu of the records (the Score column's) is the menu's: Focus stays.
    const box = await stage().boundingBox()
    await page.mouse.click(box.x + 458, box.y + 16)
    const menu = kit.getByTestId('ktable-column-header-menu')
    const opened = await until(async () => (await menu.count()) === 1)
    await page.keyboard.press('Escape')
    const closed = await until(async () => (await menu.count()) === 0)
    step('Escape closes a column menu in Focus and stays in Focus', opened && closed && (await app.getAttribute('data-mode')) === 'focus', JSON.stringify({ opened, closed, mode: await app.getAttribute('data-mode') }))
    await app.focus()
    await page.keyboard.press('Escape')
    await until(async () => (await app.getAttribute('data-mode')) !== 'focus')
    step('Escape leaves Focus and the bars come back', (await kit.getByTestId('table-tabs').count()) === 1 && (await kit.locator('.data-inline-footer').count()) === 1)
  })
} catch (error) {
  failures += 1
  console.log(`FAIL ${error instanceof Error ? error.message.split('\n')[0] : String(error)}`)
} finally {
  if (errors.length) {
    failures += 1
    console.log(`FAIL page errors:\n${errors.slice(0, 8).join('\n')}`)
  }
  await browser.close()
  await server.close()
}
process.exit(failures ? 1 : 0)
