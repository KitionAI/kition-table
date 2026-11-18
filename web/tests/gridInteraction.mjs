// The new grid's interaction on the development page (grid plan F7, F8, F14-F17, F22, F23; card
// tg4a), in a real browser: node tests/gridInteraction.mjs [chromium|webkit]
//
// The same intents as the old grid's tests in the plan's section 6, on the new grid's selectors
// (`data-ktable-grid-*`): a drag that selects cells redraws the overlay only when the cell under
// the pointer changes and never the content (case regressions/issue-grid-typing-and-drag);
// Cmd/Ctrl+Z runs one engine undo and Cmd/Ctrl+Shift+Z one redo (flow.mjs); a right click on a
// cell opens the record menu (Kition's e2e/table-kit.spec.ts, at 145,48); Escape closes a column
// menu (flow.mjs); the row head controls are centred and a checked one is drawn in the brand in
// both themes (rowControl, baseRenderer, layoutRenderer specs); a group collapses and expands
// (gridCollapsedGroupStore's purpose). Prints PASS or FAIL per step; exits 1 on any FAIL.
import { checkPackage, launch, nodeEngine, openGrid, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
checkPackage()

/** 150 records in five fields; a plain view (one frozen column) and one grouped by status. */
async function fixture() {
  const sdk = await nodeEngine()
  const doc = sdk.TableDocument.create('Interaction.ktable', 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('documents.get').tables[0]
  const tableId = table.id
  const grid = table.views[0].id
  const primary = table.fields.find((field) => field.is_primary)
  for (const field of table.fields.filter((field) => !field.is_primary)) call('fields.delete', { tableId, fieldId: field.id })
  const field = (title, type, options) => {
    const made = call('fields.create', { tableId, title, type, ...(options ? { options } : {}) })
    return made.field ?? made
  }
  const notes = field('Notes', 'text')
  const amount = field('Amount', 'number')
  const status = field('Status', 'single_select', { choices: ['To do', 'Done'] })
  const done = field('Done', 'checkbox')
  const records = Array.from({ length: 150 }, (_, i) => ({
    [primary.name]: `Record ${i + 1}`,
    [notes.name]: `Note ${i + 1}`,
    [amount.name]: i * 10,
    [status.name]: i < 4 ? 'Done' : 'To do',
    [done.name]: i % 2 === 0,
  }))
  call('transact', { ops: [{ op: 'records.create', tableId, at: null, records }] })
  call('views.patchConfig', { tableId, viewId: grid, frozen_column_count: 1 })
  const grouped = call('views.create', { tableId, title: 'Grouped', type: 'grid' })
  const groupedId = grouped.id ?? grouped.view.id
  call('views.putGroups', { tableId, viewId: groupedId, items: [{ field_id: status.id, direction: 'asc' }] })
  doc.markSaved()
  const bytes = doc.save()
  doc.free()
  return { bytes, tableId, primary: primary.name, views: { grid, grouped: groupedId } }
}

let failures = 0
const step = (name, ok, detail = '') => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}

const made = await fixture()
const { server, origin } = await startServer()
const browser = await launch(which)
const VIEWPORT = { width: 1000, height: 600 }

// ---- page helpers (run in the page) ----------------------------------------------------------

/** Waits for the grid to be complete and the band requests to stop, then two frames. */
async function idle(page) {
  await settled(page)
  await page.waitForFunction(() => !window.__ktableGrid.view.fetching, null, { timeout: 30_000, polling: 50 })
  await page.waitForTimeout(150)
  await page.waitForFunction(() => !window.__ktableGrid.view.fetching, null, { timeout: 30_000, polling: 50 })
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
}

/** The client center of a cell. */
const cellCenter = (page, row, col) =>
  page.evaluate(
    ([row, col]) => {
      const area = window.__ktableGrid.interaction.cellScreenArea({ row, col })
      const box = window.__ktableGrid.view.scroller.getBoundingClientRect()
      return area ? { x: box.left + area.x + area.width / 2, y: box.top + area.y + area.height / 2, area } : null
    },
    [row, col],
  )

/** The client center of an engine hit record: `kind` in the band `layer` (row head 4, header 2, corner 3) of `row` / `col`. */
const partCenter = (page, layer, kind, row, col = -1) =>
  page.evaluate(
    ([layer, kind, row, col]) => {
      const { view } = window.__ktableGrid
      const axes = view.currentAxes
      const vp = view.viewport
      const rowBand = row < 0 ? 0 : Math.floor(row / axes.bandRows)
      const colBand = layer === 2 ? Math.max(0, Math.floor((axes.columns[col].x - axes.bodyLeft) / axes.bandWidth)) : 0
      const list = view.cache.peek({ layer, colBand, rowBand })
      const hit = list?.hits.find((hit) => hit.kind === kind && hit.row === row && hit.col === col)
      if (!hit) return null
      const box = view.scroller.getBoundingClientRect()
      const cx = hit.x + hit.w / 2 - (layer === 2 ? vp.scrollX : 0)
      const cy = hit.y + hit.h / 2 - (layer === 4 ? vp.scrollY : 0)
      return { x: box.left + cx, y: box.top + cy }
    },
    [layer, kind, row, col],
  )

/** The overlay canvas's RGBA at client point (`x`, `y`). */
const overlayPixel = (page, x, y) =>
  page.evaluate(
    ([x, y]) => {
      const { view } = window.__ktableGrid
      const box = view.scroller.getBoundingClientRect()
      const dpr = view.viewport.dpr
      const ctx = view.overlay.getContext('2d')
      return [...ctx.getImageData(Math.floor((x - box.left) * dpr), Math.floor((y - box.top) * dpr), 1, 1).data]
    },
    [x, y],
  )

const state = (page) =>
  page.evaluate(() => {
    const { view, interaction } = window.__ktableGrid
    return {
      selection: interaction.selection,
      active: interaction.active,
      overlayDraws: view.overlayDraws,
      frames: window.__ktableGridStats.frames.length,
      scrollY: view.viewport.scrollY,
      rows: view.currentAxes.rows,
      events: window.__ktableGridEvents.map((event) => event.name),
    }
  })

const events = (page, name) => page.evaluate((name) => window.__ktableGridEvents.filter((event) => event.name === name).map((event) => event.detail), name)
/** Waits for `name:done`; throws with the failure when the page logged `name:failed`. */
async function waitEvent(page, name) {
  const base = name.replace(/:done$/, '')
  await page.waitForFunction((base) => window.__ktableGridEvents.some((event) => event.name === `${base}:done` || event.name === `${base}:failed`), base, { timeout: 30_000 })
  const failed = await events(page, `${base}:failed`)
  if (failed.length) throw new Error(`${base} failed: ${failed.join(' | ')}`)
}
const near = (a, b, tolerance = 10) => a.length >= 3 && b.every((value, i) => Math.abs(a[i] - value) <= tolerance)
const hex = (color) => [1, 3, 5].map((at) => Number.parseInt(color.slice(at, at + 2), 16))
const mod = 'ControlOrMeta'

/** Runs one test; an exception fails it and the next one still runs. */
async function test(name, body) {
  try {
    await body()
  } catch (error) {
    step(name, false, error instanceof Error ? error.message.split('\n').slice(0, 3).join(' ') : String(error))
  }
}

async function open(query = {}) {
  const opened = await openGrid(browser, origin, { dpr: 2, viewport: VIEWPORT, files: { 'Interaction.ktable': made.bytes }, query: { file: '/__grid/Interaction.ktable', view: made.views.grid, ...query } })
  await idle(opened.page)
  return opened
}

try {
  // ---- the plain view ------------------------------------------------------------------------
  const { context, page, errors } = await open()

  await test('marks the stage, the content and the overlay', async () => {
    const marks = await page.evaluate(() => ['stage', 'content', 'overlay'].map((name) => document.querySelectorAll(`[data-ktable-grid-${name}="true"]`).length))
    step('marks the stage, the content and the overlay', marks.join() === '1,1,1', marks.join())
  })

  await test('a click makes a cell active and outlines it in the brand', async () => {
    const at = await cellCenter(page, 1, 1)
    await page.mouse.click(at.x, at.y)
    const s = await state(page)
    const style = await page.evaluate(() => window.__ktableGrid.interaction.style)
    const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
    const edge = await overlayPixel(page, box.left + at.area.x + 0.5, at.y)
    step('a click makes a cell active and outlines it in the brand', JSON.stringify(s.active) === '{"row":1,"col":1}' && near(edge, [...hex(style.border), 255]), JSON.stringify({ active: s.active, edge }))
  })

  await test('a drag selects a range, redrawing the overlay only when the cell changes and never the content', async () => {
    await idle(page)
    const from = await cellCenter(page, 2, 1)
    const to = await cellCenter(page, 4, 2)
    await page.mouse.move(from.x, from.y)
    await page.mouse.down()
    const start = await state(page)
    for (const dx of [2, 5, 9, 14]) await page.mouse.move(from.x + dx, from.y + 1)
    const within = await state(page)
    await page.mouse.move(to.x, to.y, { steps: 1 })
    const moved = await state(page)
    await page.mouse.up()
    const range = moved.selection
    const ok =
      within.overlayDraws === start.overlayDraws &&
      moved.overlayDraws === start.overlayDraws + 1 &&
      moved.frames === start.frames &&
      range.kind === 'cells' &&
      JSON.stringify([range.anchor, range.focus]) === '[{"row":2,"col":1},{"row":4,"col":2}]'
    step('a drag selects a range, redrawing the overlay only when the cell changes and never the content', ok, JSON.stringify({ start: [start.overlayDraws, start.frames], within: [within.overlayDraws, within.frames], moved: [moved.overlayDraws, moved.frames], range }))
  })

  await test('Shift+click extends from the active cell', async () => {
    const a = await cellCenter(page, 1, 0)
    const b = await cellCenter(page, 3, 3)
    await page.mouse.click(a.x, a.y)
    await page.keyboard.down('Shift')
    await page.mouse.click(b.x, b.y)
    await page.keyboard.up('Shift')
    const { selection, active } = await state(page)
    step('Shift+click extends from the active cell', JSON.stringify([selection.anchor, selection.focus, active]) === '[{"row":1,"col":0},{"row":3,"col":3},{"row":1,"col":0}]', JSON.stringify(selection))
  })

  await test('arrows, Shift+arrows, Cmd/Ctrl+arrows and Tab move the active cell', async () => {
    const a = await cellCenter(page, 0, 0)
    await page.mouse.click(a.x, a.y)
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('ArrowDown')
    const moved = (await state(page)).active
    await page.keyboard.press('Shift+ArrowDown')
    const extended = (await state(page)).selection
    await page.keyboard.press('Tab')
    const tabbed = (await state(page)).active
    await page.keyboard.press('Shift+Tab')
    const back = (await state(page)).active
    const ok =
      JSON.stringify(moved) === '{"row":1,"col":1}' &&
      JSON.stringify([extended.anchor, extended.focus]) === '[{"row":1,"col":1},{"row":2,"col":1}]' &&
      JSON.stringify(tabbed) === '{"row":1,"col":2}' &&
      JSON.stringify(back) === '{"row":1,"col":1}'
    step('arrows, Shift+arrows and Tab move the active cell', ok, JSON.stringify({ moved, extended, tabbed, back }))
    await page.keyboard.press(`${mod}+ArrowDown`)
    await idle(page)
    const s = await state(page)
    const shown = await page.evaluate(() => window.__ktableGrid.interaction.activeScreenArea())
    const vp = await page.evaluate(() => window.__ktableGrid.view.viewport)
    step('Cmd/Ctrl+Down jumps to the last record and scrolls it into view', s.active.row === 149 && s.scrollY > 0 && shown && shown.y + shown.height <= vp.height + 0.5, JSON.stringify({ active: s.active, scrollY: s.scrollY, shown }))
    await page.keyboard.press(`${mod}+ArrowUp`)
    await idle(page)
    const top = await state(page)
    step('Cmd/Ctrl+Up comes back to the first record and the top', top.active.row === 0 && top.scrollY === 0, JSON.stringify({ active: top.active, scrollY: top.scrollY }))
  })

  await test('Enter, F2 and a typed key ask to edit the active cell', async () => {
    const a = await cellCenter(page, 2, 1)
    await page.mouse.click(a.x, a.y)
    await page.keyboard.press('Enter')
    await page.keyboard.press('F2')
    await page.keyboard.press('k')
    const edits = (await events(page, 'edit')).slice(-3)
    step('Enter, F2 and a typed key ask to edit the active cell', JSON.stringify(edits) === JSON.stringify([{ cell: { row: 2, col: 1 }, text: null }, { cell: { row: 2, col: 1 }, text: null }, { cell: { row: 2, col: 1 }, text: 'k' }]), JSON.stringify(edits))
  })

  await test('Escape collapses a range to the active cell; Cmd/Ctrl+A selects every cell', async () => {
    await page.keyboard.press('Shift+ArrowRight')
    await page.keyboard.press('Escape')
    const collapsed = (await state(page)).selection
    await page.keyboard.press(`${mod}+a`)
    const all = (await state(page)).selection
    step(
      'Escape collapses a range to the active cell; Cmd/Ctrl+A selects every cell',
      JSON.stringify([collapsed.anchor, collapsed.focus]) === '[{"row":2,"col":1},{"row":2,"col":1}]' && JSON.stringify([all.anchor, all.focus]) === '[{"row":0,"col":0},{"row":149,"col":4}]',
      JSON.stringify({ collapsed, all }),
    )
  })

  await test('Cmd/Ctrl+Z runs one engine undo, Cmd/Ctrl+Shift+Z one redo', async () => {
    const title = () =>
      page.evaluate(
        async ({ tableId, primary }) => {
          const { engine, interaction } = window.__ktableGrid
          const id = interaction.rows.recordId(0)
          const answer = JSON.parse(await engine.invoke('call', 'records.list', JSON.stringify({ tableId })))
          const records = Array.isArray(answer.result) ? answer.result : (answer.result.records ?? answer.result.items)
          return records.find((record) => record.id === id).values[primary]
        },
        { tableId: made.tableId, primary: made.primary },
      )
    await page.evaluate(
      async ({ tableId, primary }) => {
        const { engine, interaction } = window.__ktableGrid
        const id = interaction.rows.recordId(0)
        await engine.invoke('call', 'records.update', JSON.stringify({ tableId, recordId: id, values: { [primary]: 'Renamed' } }))
      },
      { tableId: made.tableId, primary: made.primary },
    )
    const a = await cellCenter(page, 0, 0)
    await page.mouse.click(a.x, a.y)
    await page.keyboard.press(`${mod}+z`)
    await waitEvent(page, 'undo:done')
    const undone = await title()
    await page.keyboard.press(`${mod}+Shift+z`)
    await waitEvent(page, 'redo:done')
    const redone = await title()
    const names = (await state(page)).events
    const once = names.filter((name) => name === 'undo').length === 1 && names.filter((name) => name === 'redo').length === 1
    step('Cmd/Ctrl+Z runs one engine undo, Cmd/Ctrl+Shift+Z one redo', once && undone === 'Record 1' && redone === 'Renamed', JSON.stringify({ undone, redone, names }))
  })

  await test('hovering a row tints it and shows its controls, redrawing only when the row changes', async () => {
    await idle(page)
    await page.keyboard.press('Escape')
    const a = await cellCenter(page, 5, 2)
    const other = await cellCenter(page, 7, 2)
    await page.mouse.move(a.x, a.y)
    const before = await state(page)
    await page.mouse.move(a.x + 20, a.y + 3)
    const within = await state(page)
    const tint = await overlayPixel(page, a.x + 40, a.y - 10)
    const untinted = await overlayPixel(page, other.x + 40, other.y - 10)
    // The controls cover the row number: the row head of the hovered row is drawn over, another row's is not.
    const slot = await partCenter(page, 4, 0, 5)
    const otherSlot = await partCenter(page, 4, 0, 7)
    const covered = await overlayPixel(page, slot.x - 10, slot.y - 10)
    const bare = await overlayPixel(page, otherSlot.x - 10, otherSlot.y - 10)
    step(
      'hovering a row tints it and shows its controls, redrawing only when the row changes',
      within.overlayDraws === before.overlayDraws && tint[3] > 0 && untinted[3] === 0 && covered[3] === 255 && bare[3] === 0,
      JSON.stringify({ draws: [before.overlayDraws, within.overlayDraws], tint, untinted, covered, bare }),
    )
  })

  await test('row checkboxes check rows, Shift checks a run, the header box all; checked boxes are in the brand', async () => {
    const style = await page.evaluate(() => window.__ktableGrid.interaction.style)
    const box = await partCenter(page, 4, 0, 1)
    await page.mouse.click(box.x, box.y)
    const one = (await state(page)).selection
    await page.mouse.move(box.x + 300, box.y)
    const fill = await overlayPixel(page, box.x - 6.5, box.y - 5)
    const run = await partCenter(page, 4, 0, 4)
    await page.keyboard.down('Shift')
    await page.mouse.click(run.x, run.y)
    await page.keyboard.up('Shift')
    const four = (await state(page)).selection
    const corner = await partCenter(page, 3, 0, -1, -1)
    await page.mouse.click(corner.x, corner.y)
    const all = (await state(page)).selection
    const cornerFill = await overlayPixel(page, corner.x - 6.5, corner.y - 5)
    await page.mouse.click(corner.x, corner.y)
    const none = (await state(page)).selection
    step(
      'row checkboxes check rows, Shift checks a run, the header box all; checked boxes are in the brand',
      JSON.stringify(one.ranges) === '[[1,1]]' &&
        JSON.stringify(four.ranges) === '[[1,4]]' &&
        JSON.stringify(all.ranges) === '[[0,149]]' &&
        none.kind === 'none' &&
        near(fill, [...hex(style.check), 255]) &&
        near(cornerFill, [...hex(style.check), 255]),
      JSON.stringify({ one, four, all: all.ranges, none: none.kind, fill, cornerFill, check: style.check }),
    )
  })

  await test('the expand control opens the record', async () => {
    const expand = await partCenter(page, 4, 5, 3)
    await page.mouse.move(expand.x, expand.y)
    await page.mouse.click(expand.x, expand.y)
    const id = await page.evaluate(() => window.__ktableGrid.interaction.rows.recordId(3))
    const opened = await events(page, 'expand')
    step('the expand control opens the record', opened.at(-1) === id, JSON.stringify({ opened, id }))
  })

  await test('a right click on a cell opens the record menu for its record', async () => {
    await page.mouse.click(145, 48, { button: 'right' })
    const menu = (await events(page, 'contextMenu')).at(-1)
    const id = await page.evaluate(() => window.__ktableGrid.interaction.rows.recordId(0))
    const shown = await page.locator('[data-ktable-grid-menu="record"]').count()
    await page.keyboard.press('Escape')
    const closed = await page.locator('[data-ktable-grid-menu]').count()
    step('a right click on a cell opens the record menu for its record', JSON.stringify(menu?.recordIds) === JSON.stringify([id]) && JSON.stringify(menu?.cell) === '{"row":0,"col":0}' && shown === 1 && closed === 0, JSON.stringify({ menu, shown, closed }))
  })

  await test('a column caret opens the column menu with its field; Escape closes it', async () => {
    const caret = await partCenter(page, 2, 8, -1, 2)
    await page.mouse.click(caret.x, caret.y)
    const menu = (await events(page, 'columnMenu')).at(-1)
    const field = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns[2].fieldId)
    const shown = await page.locator('[data-ktable-grid-menu="column"]').count()
    await page.keyboard.press('Escape')
    const closed = await page.locator('[data-ktable-grid-menu]').count()
    const selection = (await state(page)).selection
    step('a column caret opens the column menu with its field; Escape closes it', menu?.fieldId === field && menu?.col === 2 && shown === 1 && closed === 0 && selection.kind !== 'columns', JSON.stringify({ menu, field, shown, closed }))
  })

  await test('a press on a column header selects it; Shift extends; the hovered header is tinted', async () => {
    const one = await page.evaluate(() => {
      const { view } = window.__ktableGrid
      const column = view.currentAxes.columns[1]
      const box = view.scroller.getBoundingClientRect()
      return { x: box.left + column.x + 40, y: box.top + 16 }
    })
    await page.mouse.click(one.x, one.y)
    const single = (await state(page)).selection
    await page.keyboard.down('Shift')
    await page.mouse.click(one.x + 440, one.y)
    await page.keyboard.up('Shift')
    const range = (await state(page)).selection
    // Column 4 is neither selected nor hovered until the pointer is over its header.
    const plain = await overlayPixel(page, one.x + 650, one.y + 6)
    await page.mouse.move(one.x + 640, one.y)
    const tint = await overlayPixel(page, one.x + 650, one.y + 6)
    await page.keyboard.press('Escape')
    step(
      'a press on a column header selects it; Shift extends; the hovered header is tinted',
      single.kind === 'columns' && single.focus === 1 && range.anchor === 1 && range.focus === 3 && plain[3] === 0 && tint[3] > 0,
      JSON.stringify({ single, range, plain, tint }),
    )
  })

  await test('the add-field column asks for a field', async () => {
    await page.evaluate(() => window.__ktableGrid.view.scrollTo(100_000, 0))
    await idle(page)
    const at = await page.evaluate(() => {
      const { view } = window.__ktableGrid
      const axes = view.currentAxes
      const add = axes.columns[axes.columns.length - 1]
      const box = view.scroller.getBoundingClientRect()
      return { x: box.left + add.x + add.width / 2 - view.viewport.scrollX, y: box.top + 16, visible: add.x - view.viewport.scrollX < view.viewport.width }
    })
    if (at.visible) await page.mouse.click(at.x, at.y)
    const asked = await events(page, 'appendField')
    await page.keyboard.press('Escape')
    step('the add-field column asks for a field', at.visible && asked.length === 1 && asked[0].anchor.width === 100, JSON.stringify({ at, asked }))
  })

  await test('the add row adds a record', async () => {
    const before = (await state(page)).rows
    await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 1_000_000))
    await idle(page)
    const at = await page.evaluate(() => {
      const { view } = window.__ktableGrid
      const axes = view.currentAxes
      const row = axes.rows - 1
      const box = view.scroller.getBoundingClientRect()
      return { x: box.left + 300, y: box.top + axes.rowTop(row) + axes.rowHeight(row) / 2 - view.viewport.scrollY }
    })
    const last = await page.evaluate(() => window.__ktableGrid.interaction.rows.recordId(149))
    await page.mouse.click(at.x, at.y)
    await waitEvent(page, 'appendRecord:done')
    const after = (await state(page)).rows
    const asked = await events(page, 'appendRecord')
    step('the add row adds a record', after === before + 1 && asked.length === 1 && asked[0].anchorRecordId === last, JSON.stringify({ before, after, asked, last }))
  })

  await test('a drag past the bottom edge scrolls and keeps selecting', async () => {
    await page.keyboard.press(`${mod}+ArrowUp`)
    await idle(page)
    const from = await cellCenter(page, 1, 1)
    const vp = await page.evaluate(() => window.__ktableGrid.view.viewport)
    await page.mouse.move(from.x, from.y)
    await page.mouse.down()
    await page.mouse.move(from.x, vp.height - 4, { steps: 4 })
    await page.waitForTimeout(900)
    await page.mouse.move(from.x + 1, vp.height - 4)
    const s = await state(page)
    await page.mouse.up()
    const lastShown = await page.evaluate(() => {
      const { view } = window.__ktableGrid
      return view.currentAxes.rowAt(view.viewport.height - 1)
    })
    step('a drag past the bottom edge scrolls and keeps selecting', s.scrollY > 100 && s.selection.kind === 'cells' && s.selection.focus.row > 14 && s.selection.anchor.row === 1, JSON.stringify({ scrollY: s.scrollY, selection: s.selection, lastShown }))
    await page.keyboard.press(`${mod}+ArrowUp`)
  })

  step('no page errors (plain view)', errors.length === 0, errors.slice(0, 3).join(' | '))
  await context.close()

  // ---- the grouped view ----------------------------------------------------------------------
  {
    const { context, page, errors } = await open({ view: made.views.grouped })
    await test('arrows step over group headers and add rows', async () => {
      // The first group's last record, the add row after it, the next group's header.
      const { first, lastOfGroup, kinds } = await page.evaluate(() => {
        const rows = window.__ktableGrid.interaction.rows
        let append = 0
        while (rows.kind(append) !== 2) append += 1
        return { first: rows.firstRecord, lastOfGroup: append - 1, kinds: [rows.kind(append - 1), rows.kind(append), rows.kind(append + 1), rows.kind(append + 2)] }
      })
      await page.evaluate((row) => window.__ktableGrid.interaction.scrollToCell({ row: row + 3, col: 1 }), lastOfGroup)
      await idle(page)
      const a = await cellCenter(page, lastOfGroup, 1)
      await page.mouse.click(a.x, a.y)
      await page.keyboard.press('ArrowDown')
      const down = (await state(page)).active
      await page.keyboard.press('ArrowUp')
      const up = (await state(page)).active
      step('arrows step over group headers and add rows', first === 1 && kinds.join() === '0,2,1,0' && down.row === lastOfGroup + 3 && up.row === lastOfGroup, JSON.stringify({ kinds, lastOfGroup, down, up }))
    })

    await test('a group toggle collapses and expands its group', async () => {
      await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
      await idle(page)
      const before = (await state(page)).rows
      const toggle = await partCenter(page, 4, 7, 0)
      // The first group's records and its add row go.
      const count = await page.evaluate(() => {
        const rows = window.__ktableGrid.interaction.rows
        let append = 0
        while (rows.kind(append) !== 2) append += 1
        return append
      })
      await page.mouse.click(toggle.x, toggle.y)
      await page.waitForFunction(() => window.__ktableGridEvents.filter((event) => event.name === 'toggleGroup:done').length === 1)
      const collapsed = (await state(page)).rows
      await idle(page)
      const again = await partCenter(page, 4, 7, 0)
      await page.mouse.click(again.x, again.y)
      await page.waitForFunction(() => window.__ktableGridEvents.filter((event) => event.name === 'toggleGroup:done').length === 2)
      const expanded = (await state(page)).rows
      const asked = await events(page, 'toggleGroup')
      step(
        'a group toggle collapses and expands its group',
        collapsed === before - count && expanded === before && asked[0].collapsed === true && asked[1].collapsed === false,
        JSON.stringify({ before, collapsed, expanded, count, asked }),
      )
    })

    await test('the grouped row head has no drag handle', async () => {
      await idle(page)
      const parts = await page.evaluate(() => {
        const { view } = window.__ktableGrid
        const list = view.cache.peek({ layer: 4, colBand: 0, rowBand: 0 })
        return list.hits.filter((hit) => hit.row === 1).map((hit) => [hit.kind, hit.x, hit.w])
      })
      step('the grouped row head has no drag handle', JSON.stringify(parts) === '[[0,0,24],[5,24,24]]', JSON.stringify(parts))
    })
    step('no page errors (grouped view)', errors.length === 0, errors.slice(0, 3).join(' | '))
    await context.close()
  }

  // ---- dark ----------------------------------------------------------------------------------
  {
    const { context, page, errors } = await open({ theme: 'dark' })
    await test('a checked box is drawn in the brand in the dark theme too', async () => {
      const style = await page.evaluate(() => window.__ktableGrid.interaction.style)
      const box = await partCenter(page, 4, 0, 2)
      await page.mouse.click(box.x, box.y)
      await page.mouse.move(box.x + 300, box.y + 200)
      const fill = await overlayPixel(page, box.x - 6.5, box.y - 5)
      step('a checked box is drawn in the brand in the dark theme too', style.check === '#3fd1bf' && near(fill, [...hex(style.check), 255]), JSON.stringify({ fill, check: style.check }))
    })
    step('no page errors (dark)', errors.length === 0, errors.slice(0, 3).join(' | '))
    await context.close()
  }
} finally {
  await browser.close()
  await server.close()
}

console.log(failures ? `${failures} FAILED (${which})` : `ALL PASS (${which})`)
process.exit(failures ? 1 : 0)
