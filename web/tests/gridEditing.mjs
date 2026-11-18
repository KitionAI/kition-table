// Editing on the new grid on the development page (grid plan F5, F9-F13, F18, F19, F21; card
// tg4b), in a real browser: node tests/gridEditing.mjs [chromium|webkit]
//
// The intents of the old grid's tests in the plan's section 6, on the new grid: a typed key opens
// a text cell's editor, which is the focused textarea on the cell, with one brand border and the
// text centred (TextEditor.spec), and Enter writes one records.update and moves down (flow.mjs:290,
// case regressions/issue-grid-typing-and-drag); an input method's composition opens it and stays
// in it (platform UI-K6); a date cell opens the compact calendar and time menu, and a picked day is
// one records.update (flow.mjs:321, TextEditor.spec); a choice cell opens its list; a checkbox and a
// rating's symbol write on a click; copy puts the selection's cells on the clipboard as TSV
// (flow.mjs:270, tableClipboard.spec); a paste past the last record is one transact of cells.set
// and records.create (flow.mjs:360); Delete clears the selection in one transact; the fill handle
// fills down in one transact; a row dragged by its handle is one records.move (flow.mjs:381); a
// selected column dragged is one fields.move (flow.mjs:401, tableColumnOrdering.spec); a column's
// edge resizes it with views.patchField (flow.mjs:421); the frozen edge dragged saves the frozen
// count (flow.mjs:599, useColumnFreeze.spec); an attachment's picture is drawn from the document's
// blob and a click previews it (imageCellRenderer.spec, AttachmentCell.spec); an AI cell shows a
// regenerate button on hover and a spinner while it generates (aiActionLayer.spec).
//
// Card tg4c, the old grid's features the new one lacked: an edit that leaves the arrangement as it
// was redraws only its record's bands (gridSync, no new arrangement); a column header that is not
// selected drags its column (flow.mjs:401); the active attachment cell's add button picks files
// and adds them in one records.update, and files pasted on an attachment cell are added the same
// way (TableEditorGridView's picker, useTableClipboard's file paste, imageCellRenderer.spec); a
// picture at a web address is drawn (the old grid's ImageManager loaded such urls).
// Prints PASS or FAIL per step; exits 1 on any FAIL.
import zlib from 'node:zlib'
import { checkPackage, launch, nodeEngine, openGrid, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
checkPackage()

/** A `width` x `height` PNG of one color (RGB). */
function png(width, height, [r, g, b]) {
  const chunk = (type, data) => {
    const head = Buffer.alloc(8)
    head.writeUInt32BE(data.length, 0)
    head.write(type, 4, 'latin1')
    const crc = Buffer.alloc(4)
    crc.writeUInt32BE(zlib.crc32(Buffer.concat([Buffer.from(type, 'latin1'), data])), 0)
    return Buffer.concat([head, data, crc])
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr.set([8, 2, 0, 0, 0], 8)
  const rows = Buffer.alloc((width * 3 + 1) * height)
  for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) rows.set([r, g, b], y * (width * 3 + 1) + 1 + x * 3)
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(rows)), chunk('IEND', Buffer.alloc(0))])
}

const BLUE = [30, 60, 220]
const { server, origin } = await startServer()
/** The fixture's picture at a web address (the development server's origin; the test's page serves it). */
const WEB_PICTURE = `${origin}/__grid/blue.png`

/** 12 records over one field of each kind the editors cover; one record holds a red picture, another a blue one at a web address. */
async function fixture() {
  const sdk = await nodeEngine()
  const doc = sdk.TableDocument.create('Editing.ktable', 0, 0)
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
  const amount = field('Amount', 'number')
  const notes = field('Notes', 'long_text')
  const status = field('Status', 'single_select', { choices: ['To do', 'Doing', 'Done'] })
  const tags = field('Tags', 'multi_select', { choices: ['red', 'green', 'blue'] })
  const done = field('Done', 'checkbox')
  const score = field('Score', 'rating', { max: 5 })
  const due = field('Due', 'datetime')
  const files = field('Files', 'attachment')
  const summary = field('Summary', 'ai', { ai_config: { type: 'summarize', source_field_id: notes.id, enabled: true } })
  const picture = call('attachments.put', { tableId, name: 'red.png', mimeType: 'image/png', data: png(8, 4, [220, 20, 30]).toString('base64') })
  const records = Array.from({ length: 12 }, (_, i) => ({
    [primary.name]: `Record ${i + 1}`,
    [amount.name]: (i + 1) * 10,
    [notes.name]: `Note ${i + 1}`,
    [status.name]: 'To do',
    [tags.name]: ['red'],
    [done.name]: false,
    [score.name]: 1,
    [due.name]: '2026-07-23T04:14:47Z',
    [files.name]: i === 0 ? [picture] : i === 3 ? [{ name: 'blue.png', url: WEB_PICTURE, mime_type: 'image/png' }] : [],
    [summary.name]: `Summary ${i + 1}`,
  }))
  call('transact', { ops: [{ op: 'records.create', tableId, at: null, records }] })
  call('views.patchConfig', { tableId, viewId: grid, frozen_column_count: 1 })
  doc.markSaved()
  const bytes = doc.save()
  doc.free()
  const names = { title: primary.name, amount: amount.name, notes: notes.name, status: status.name, tags: tags.name, done: done.name, score: score.name, due: due.name, files: files.name, summary: summary.name }
  return { bytes, tableId, view: grid, names, ids: { title: primary.id, amount: amount.id, summary: summary.id } }
}

let failures = 0
const step = (name, ok, detail = '') => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}

const made = await fixture()
const browser = await launch(which)
const VIEWPORT = { width: 1400, height: 700 }
const mod = 'ControlOrMeta'
const COLS = ['title', 'amount', 'notes', 'status', 'tags', 'done', 'score', 'due', 'files', 'summary']
const col = (name) => COLS.indexOf(name)
/** Two CJK characters, as an input method commits them. */
const COMPOSED = String.fromCodePoint(0x4f60, 0x597d)

async function test(name, body) {
  try {
    await body()
  } catch (error) {
    step(name, false, error instanceof Error ? error.message.split('\n').slice(0, 3).join(' ') : String(error))
  }
}

async function idle(page) {
  await page.waitForFunction(() => window.__ktableGrid.editing.busy === 0 && !window.__ktableGrid.view.fetching, null, { timeout: 30_000, polling: 50 })
  await settled(page)
  await page.waitForFunction(() => !window.__ktableGrid.view.fetching, null, { timeout: 30_000, polling: 50 })
}

/** Scrolls cell (`row`, `col`) into view and answers its client rectangle and center. */
const cell = (page, row, col) =>
  page.evaluate(
    async ([row, col]) => {
      const { interaction, view } = window.__ktableGrid
      interaction.scrollToCell({ row, col })
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))
      const area = interaction.cellScreenArea({ row, col })
      const box = view.scroller.getBoundingClientRect()
      return { x: box.left + area.x + area.width / 2, y: box.top + area.y + area.height / 2, left: box.left + area.x, top: box.top + area.y, width: area.width, height: area.height }
    },
    [row, col],
  )

/** The client center of a cell's engine hit part (`kind`, `index`), in its body or frozen band. */
const cellPart = (page, kind, row, col, index = 0) =>
  page.evaluate(
    ([kind, row, col, index]) => {
      const { view } = window.__ktableGrid
      const axes = view.currentAxes
      const column = axes.columns[col]
      const key = column.frozen
        ? { layer: 1, colBand: 0, rowBand: Math.floor(row / axes.bandRows) }
        : { layer: 0, colBand: Math.floor((column.x - axes.bodyLeft) / axes.bandWidth), rowBand: Math.floor(row / axes.bandRows) }
      const hit = view.cache.peek(key)?.hits.find((hit) => hit.kind === kind && hit.row === row && hit.col === col && hit.index === index)
      if (!hit) return null
      const vp = view.viewport
      const box = view.scroller.getBoundingClientRect()
      return { x: box.left + hit.x + hit.w / 2 - (column.frozen ? 0 : vp.scrollX), y: box.top + hit.y + hit.h / 2 - vp.scrollY, w: hit.w, h: hit.h }
    },
    [kind, row, col, index],
  )

const writes = (page) => page.evaluate(() => window.__ktableGridWrites.length)
const writesFrom = (page, from) => page.evaluate((from) => window.__ktableGridWrites.slice(from), from)
async function waitWrites(page, from, count = 1) {
  await page.waitForFunction(([from, count]) => window.__ktableGridWrites.length >= from + count, [from, count], { timeout: 15_000 })
  await idle(page)
  return writesFrom(page, from)
}
/** The records of the view in display order (id and values), read from the engine. */
const shown = (page) =>
  page.evaluate(async () => {
    const { interaction, data } = window.__ktableGrid
    const rows = interaction.rows
    const ids = []
    for (let row = 0; row < rows.count; row += 1) if (rows.recordId(row) != null) ids.push(rows.recordId(row))
    return data.records(ids)
  })
const valueAt = async (page, row, name) => (await shown(page))[row]?.values[name] ?? null
const active = (page) => page.evaluate(() => window.__ktableGrid.interaction.active)
const contentPixel = (page, x, y) =>
  page.evaluate(
    ([x, y]) => {
      const { view } = window.__ktableGrid
      const box = view.scroller.getBoundingClientRect()
      const dpr = view.viewport.dpr
      return [...view.content.getContext('2d').getImageData(Math.floor((x - box.left) * dpr), Math.floor((y - box.top) * dpr), 1, 1).data]
    },
    [x, y],
  )
/** How many overlay pixels in the `size` CSS px square around (`x`, `y`) are the brand, nearly opaque. */
const brandPixels = (page, x, y, size, rgb) =>
  page.evaluate(
    ([x, y, size, rgb]) => {
      const { view } = window.__ktableGrid
      const box = view.scroller.getBoundingClientRect()
      const dpr = view.viewport.dpr
      const side = Math.round(size * dpr)
      const data = view.overlay.getContext('2d').getImageData(Math.round((x - box.left - size / 2) * dpr), Math.round((y - box.top - size / 2) * dpr), side, side).data
      let count = 0
      for (let i = 0; i < data.length; i += 4) if (data[i + 3] > 150 && Math.abs(data[i] - rgb[0]) < 40 && Math.abs(data[i + 1] - rgb[1]) < 40 && Math.abs(data[i + 2] - rgb[2]) < 40) count += 1
      return count
    },
    [x, y, size, rgb],
  )
const near = (a, b, tolerance = 12) => b.every((value, i) => Math.abs(a[i] - value) <= tolerance)

async function open() {
  const opened = await openGrid(browser, origin, { dpr: 2, viewport: VIEWPORT, files: { 'Editing.ktable': made.bytes, 'blue.png': png(8, 8, BLUE) }, query: { file: '/__grid/Editing.ktable', view: made.view } })
  await idle(opened.page)
  return opened
}

try {
  const { context, page, errors } = await open()
  if (which === 'chromium') await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin })
  const N = made.names

  await test('a typed key opens the text editor on the cell; Enter writes one records.update and moves down', async () => {
    const at = await cell(page, 0, col('title'))
    await page.mouse.click(at.x, at.y)
    const from = await writes(page)
    await page.keyboard.type('Hello')
    const editor = await page.evaluate(() => {
      const input = document.querySelector('[data-ktable-grid-input]')
      const box = input.getBoundingClientRect()
      const style = getComputedStyle(input)
      return { focused: document.activeElement === input, value: input.value, opacity: style.opacity, border: style.borderTopWidth, color: style.borderTopColor, align: style.textAlign, shadow: style.boxShadow, box: { x: box.x, y: box.y, w: box.width, h: box.height }, padTop: parseFloat(style.paddingTop) }
    })
    const brand = await page.evaluate(() => window.__ktableGrid.interaction.style.border)
    const rgb = [1, 3, 5].map((i) => Number.parseInt(brand.slice(i, i + 2), 16))
    const onCell = Math.abs(editor.box.x - at.left) < 1 && Math.abs(editor.box.y - at.top) < 1 && Math.abs(editor.box.w - at.width) < 1 && Math.abs(editor.box.h - at.height) < 1
    // The line box (20 px) sits in the middle of the cell, as the cell's text does.
    const lineMiddle = editor.box.y + 2 + editor.padTop + 10
    step(
      'the text editor is the focused textarea on the cell, one brand border, text centred',
      editor.focused && editor.value === 'Hello' && editor.opacity === '1' && editor.border === '2px' && editor.color === `rgb(${rgb.join(', ')})` && editor.align === 'center' && editor.shadow === 'none' && onCell && Math.abs(lineMiddle - at.y) <= 1.5,
      JSON.stringify({ editor, at, lineMiddle }),
    )
    await page.keyboard.press('Enter')
    const made = await waitWrites(page, from)
    const one = made.length === 1 && made[0].command === 'records.update' && made[0].params.values[N.title] === 'Hello'
    step('Enter writes one records.update and moves down', one && (await valueAt(page, 0, N.title)) === 'Hello' && JSON.stringify(await active(page)) === '{"row":1,"col":0}', JSON.stringify(made))
  })

  await test('an edit that keeps the arrangement redraws only its record\'s bands', async () => {
    const at = await cell(page, 1, col('notes'))
    await page.mouse.click(at.x, at.y)
    await idle(page)
    const before = await page.evaluate(() => {
      const { view } = window.__ktableGrid
      const axes = view.currentAxes
      const header = view.neededKeys().find((key) => key.layer === 2 || key.layer === 3)
      const body = view.neededKeys().find((key) => key.layer === 0 && key.rowBand === 0)
      window.__syncProbe = { header: view.cache.peek(header), body: view.cache.peek(body), headerKey: header, bodyKey: body }
      return { epoch: axes.epoch }
    })
    const from = await writes(page)
    await page.keyboard.type('Synced')
    await page.keyboard.press('Enter')
    const made = await waitWrites(page, from)
    const after = await page.evaluate(() => {
      const { view } = window.__ktableGrid
      const probe = window.__syncProbe
      const done = window.__ktableGridEvents.filter((event) => event.name === 'write:done').at(-1)?.detail
      // The header strip's list is the one it had; the record's band is a new one.
      return { epoch: view.currentAxes.epoch, done, header: view.cache.peek(probe.headerKey) === probe.header, body: view.cache.peek(probe.bodyKey) !== probe.body && view.cache.has(probe.bodyKey) }
    })
    step(
      'an edit that keeps the arrangement redraws only its record\'s bands',
      made.length === 1 && after.epoch === before.epoch && after.done?.synced === 'bands' && after.header && after.body && (await valueAt(page, 1, N.notes)) === 'Synced',
      JSON.stringify({ before, after, made }),
    )
    await page.keyboard.press('Escape')
    const back = await cell(page, 1, col('title'))
    await page.mouse.click(back.x, back.y)
  })

  await test('Enter opens the editor with the value; Escape closes it without writing; Tab commits and moves right', async () => {
    const from = await writes(page)
    await page.keyboard.press('Enter')
    await page.waitForFunction(() => document.querySelector('[data-ktable-grid-input]').value === 'Record 2', null, { timeout: 5000 })
    await page.keyboard.press('Escape')
    await page.waitForTimeout(150)
    const none = (await writes(page)) === from
    const hidden = await page.evaluate(() => getComputedStyle(document.querySelector('[data-ktable-grid-input]')).opacity === '0')
    step('Enter opens the editor with the value; Escape closes it without writing', none && hidden)
    await page.keyboard.press('Enter')
    await page.waitForFunction(() => document.querySelector('[data-ktable-grid-input]').value === 'Record 2', null, { timeout: 5000 })
    await page.keyboard.type(' x')
    await page.keyboard.press('Tab')
    const made = await waitWrites(page, from)
    step('Tab commits and moves right', made.length === 1 && made[0].params.values[N.title] === 'Record 2 x' && JSON.stringify(await active(page)) === '{"row":1,"col":1}', JSON.stringify(made))
  })

  await test('a number cell writes a number', async () => {
    const from = await writes(page)
    await page.keyboard.type('42')
    await page.keyboard.press('Enter')
    const made = await waitWrites(page, from)
    step('a number cell writes a number', made.length === 1 && made[0].params.values[N.amount] === 42 && (await valueAt(page, 1, N.amount)) === 42, JSON.stringify(made))
  })

  await test('an input method composes in the editor it opens', async () => {
    const at = await cell(page, 3, col('title'))
    await page.mouse.click(at.x, at.y)
    const from = await writes(page)
    if (which === 'chromium') {
      const cdp = await context.newCDPSession(page)
      await cdp.send('Input.imeSetComposition', { text: 'ni', selectionStart: 2, selectionEnd: 2 })
      const during = await page.evaluate(() => ({ opacity: getComputedStyle(document.querySelector('[data-ktable-grid-input]')).opacity, editing: window.__ktableGrid.editing.editing }))
      await cdp.send('Input.insertText', { text: COMPOSED })
      const after = await page.evaluate(() => document.querySelector('[data-ktable-grid-input]').value)
      await page.keyboard.press('Enter')
      const made = await waitWrites(page, from)
      step('an input method composes in the editor it opens', during.opacity === '1' && during.editing && after === COMPOSED && made.length === 1 && made[0].params.values[N.title] === COMPOSED, JSON.stringify({ during, after, made }))
    } else {
      // WebKit has no scripted input method: the composition's events, as the textarea gets them.
      const during = await page.evaluate((text) => {
        const input = document.activeElement
        input.dispatchEvent(new CompositionEvent('compositionstart', { data: '' }))
        const state = { opacity: getComputedStyle(input).opacity, editing: window.__ktableGrid.editing.editing, same: input === document.querySelector('[data-ktable-grid-input]') }
        input.value = text
        input.dispatchEvent(new CompositionEvent('compositionend', { data: text }))
        return state
      }, COMPOSED)
      await page.keyboard.press('Enter')
      const made = await waitWrites(page, from)
      step('an input method composes in the editor it opens', during.same && during.opacity === '1' && during.editing && made.length === 1 && made[0].params.values[N.title] === COMPOSED, JSON.stringify({ during, made }))
    }
  })

  await test('a date cell opens the compact calendar; a picked day is one records.update', async () => {
    const at = await cell(page, 0, col('due'))
    await page.mouse.click(at.x, at.y)
    await page.mouse.dblclick(at.x, at.y)
    const calendar = page.getByTestId('table-date-calendar')
    await calendar.waitFor({ timeout: 5000 })
    const text = await page.getByTestId('table-date-cell-editor').textContent()
    const box = await calendar.boundingBox()
    const native = await page.locator('input[type="datetime-local"]').count()
    const expected = await page.evaluate(() => {
      const date = new Date('2026-07-23T04:14:47Z')
      const pad = (value) => String(value).padStart(2, '0')
      const day = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
      const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`
      date.setDate(date.getDate() + 1)
      return { day, time, next: `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`, stored: date.toISOString() }
    })
    step('a date cell opens the compact calendar', text.includes(expected.day) && text.includes(expected.time) && native === 0 && box.width <= 290 && box.height <= 390, JSON.stringify({ text, box }))
    const from = await writes(page)
    await calendar.locator(`[data-date="${expected.next}"]`).click()
    const made = await waitWrites(page, from)
    step('a picked day is one records.update', made.length === 1 && made[0].command === 'records.update' && made[0].params.values[N.due] === expected.stored && (await valueAt(page, 0, N.due)) === expected.stored, JSON.stringify(made))
    await page.keyboard.press('Enter')
    await calendar.waitFor({ timeout: 5000 })
    await page.getByTestId('table-date-time-trigger').click()
    const menu = page.getByTestId('table-date-time-menu')
    await menu.waitFor({ timeout: 5000 })
    const menuBox = await menu.boundingBox()
    step('the time menu is compact', (await menu.textContent()).includes(expected.time) && menuBox.width <= 100 && menuBox.height <= 250, JSON.stringify(menuBox))
    await page.keyboard.press('Escape')
    await page.waitForFunction(() => !window.__ktableGrid.editing.editing, null, { timeout: 5000 })
  })

  await test('a single choice cell opens its list; a pick writes it', async () => {
    const at = await cell(page, 0, col('status'))
    await page.mouse.click(at.x, at.y)
    await page.keyboard.press('Enter')
    const list = page.getByTestId('table-select-cell-editor')
    await list.waitFor({ timeout: 5000 })
    const from = await writes(page)
    await list.locator('[data-choice="Done"]').click()
    const made = await waitWrites(page, from)
    step('a single choice cell opens its list; a pick writes it', made.length === 1 && made[0].params.values[N.status] === 'Done' && (await valueAt(page, 0, N.status)) === 'Done', JSON.stringify(made))
  })

  await test('a multiple choice cell toggles choices and writes them when it closes', async () => {
    const at = await cell(page, 0, col('tags'))
    await page.mouse.click(at.x, at.y)
    await page.keyboard.press('Enter')
    const list = page.getByTestId('table-select-cell-editor')
    await list.waitFor({ timeout: 5000 })
    const from = await writes(page)
    await list.locator('[data-choice="blue"]').click()
    await page.mouse.click(VIEWPORT.width - 40, VIEWPORT.height - 40)
    const made = await waitWrites(page, from)
    step('a multiple choice cell toggles choices and writes them when it closes', made.length === 1 && JSON.stringify(made[0].params.values[N.tags]) === '["red","blue"]', JSON.stringify(made))
  })

  await test('a click on a checkbox and on a rating symbol writes them', async () => {
    await cell(page, 2, col('done'))
    await idle(page)
    const box = await cellPart(page, 0, 2, col('done'))
    let from = await writes(page)
    await page.mouse.click(box.x, box.y)
    let made = await waitWrites(page, from)
    step('a click on a checkbox toggles it', made.length === 1 && made[0].params.values[N.done] === true && (await valueAt(page, 2, N.done)) === true, JSON.stringify(made))
    await cell(page, 2, col('score'))
    await idle(page)
    const star = await cellPart(page, 1, 2, col('score'), 3)
    from = await writes(page)
    await page.mouse.click(star.x, star.y)
    made = await waitWrites(page, from)
    step('a click on the fourth star rates 4', made.length === 1 && made[0].params.values[N.score] === 4 && (await valueAt(page, 2, N.score)) === 4, JSON.stringify(made))
  })

  await test('copy puts the selected cells on the clipboard as tab-separated text', async () => {
    const a = await cell(page, 4, col('title'))
    await page.mouse.click(a.x, a.y)
    const b = await cell(page, 5, col('amount'))
    await page.keyboard.down('Shift')
    await page.mouse.click(b.x, b.y)
    await page.keyboard.up('Shift')
    await page.waitForFunction(() => window.__ktableGrid.editing.copyReady, null, { timeout: 5000 })
    const copied = await page.evaluate(() => {
      const data = new DataTransfer()
      document.querySelector('[data-ktable-grid-input]').dispatchEvent(new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }))
      return data.getData('text/plain')
    })
    step('copy puts the selected cells on the clipboard as tab-separated text', copied === 'Record 5\t50\nRecord 6\t60', JSON.stringify(copied))
    if (which === 'chromium') {
      await page.keyboard.press(`${mod}+c`)
      let system = ''
      for (let i = 0; i < 20 && system !== copied; i += 1) {
        system = await page.evaluate(() => navigator.clipboard.readText())
        if (system !== copied) await page.waitForTimeout(100)
      }
      step('Cmd/Ctrl+C copies to the system clipboard', system === copied, JSON.stringify(system))
    }
  })

  await test('a paste past the last record is one transact of cells.set and records.create', async () => {
    const before = (await shown(page)).length
    const at = await cell(page, before - 2, col('title'))
    await page.mouse.click(at.x, at.y)
    const from = await writes(page)
    const tsv = ['a1\t11\tn1', 'a2\t12\tn2', 'a3\t13\tn3'].join('\n')
    await page.evaluate((text) => {
      const data = new DataTransfer()
      data.setData('text/plain', text)
      document.querySelector('[data-ktable-grid-input]').dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }))
    }, tsv)
    const made = await waitWrites(page, from)
    await page.waitForTimeout(200)
    const ops = made[0]?.params.ops?.map((op) => op.op) ?? []
    const after = await shown(page)
    const tail = after.slice(-3).map((record) => [record.values[N.title], record.values[N.amount], record.values[N.notes]])
    step(
      'a paste past the last record is one transact of cells.set and records.create',
      made.length === 1 && made[0].command === 'transact' && ops.join() === 'cells.set,records.create' && after.length === before + 1 && JSON.stringify(tail) === JSON.stringify([['a1', 11, 'n1'], ['a2', 12, 'n2'], ['a3', 13, 'n3']]),
      JSON.stringify({ ops, tail, before, after: after.length }),
    )
  })

  await test('Delete clears the selected cells in one transact', async () => {
    const a = await cell(page, 6, col('amount'))
    await page.mouse.click(a.x, a.y)
    const b = await cell(page, 7, col('notes'))
    await page.keyboard.down('Shift')
    await page.mouse.click(b.x, b.y)
    await page.keyboard.up('Shift')
    const from = await writes(page)
    await page.keyboard.press('Delete')
    const made = await waitWrites(page, from)
    const records = await shown(page)
    const cleared = [6, 7].every((row) => records[row].values[N.amount] == null && !records[row].values[N.notes]) && records[6].values[N.title] === 'Record 7'
    step('Delete clears the selected cells in one transact', made.length === 1 && made[0].params.ops.length === 1 && made[0].params.ops[0].op === 'cells.set' && made[0].params.ops[0].records.length === 2 && cleared, JSON.stringify(made))
  })

  await test('the fill handle fills down in one transact', async () => {
    const a = await cell(page, 8, col('amount'))
    await page.mouse.click(a.x, a.y)
    await idle(page)
    const handle = { x: a.left + a.width - 1, y: a.top + a.height - 1 }
    const target = await cell(page, 10, col('amount'))
    const from = await writes(page)
    await page.mouse.move(handle.x, handle.y)
    await page.mouse.down()
    await page.mouse.move(target.x, target.y, { steps: 6 })
    await page.mouse.up()
    const made = await waitWrites(page, from)
    const records = await shown(page)
    const filled = [9, 10].every((row) => records[row].values[N.amount] === 90)
    step('the fill handle fills down in one transact', made.length === 1 && made[0].params.ops[0].op === 'cells.set' && made[0].params.ops[0].records.length === 2 && filled, JSON.stringify({ made, values: records.map((record) => record.values[N.amount]) }))
    const selection = await page.evaluate(() => window.__ktableGrid.interaction.selection)
    step('the range grows over what was filled', JSON.stringify([selection.anchor, selection.focus]) === '[{"row":8,"col":1},{"row":10,"col":1}]', JSON.stringify(selection))
  })

  await test('a row dragged by its handle is one records.move', async () => {
    const before = await shown(page)
    const first = await cell(page, 0, col('title'))
    const third = await cell(page, 2, col('title'))
    const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
    const from = await writes(page)
    await page.mouse.move(box.left + 12, third.y)
    await page.mouse.down()
    await page.mouse.move(box.left + 12, first.y, { steps: 8 })
    await page.mouse.up()
    const made = await waitWrites(page, from)
    const after = await shown(page)
    const ops = made[0]?.params.ops ?? []
    step(
      'a row dragged by its handle is one records.move',
      made.length === 1 && ops.length === 1 && ops[0].op === 'records.move' && ops[0].rowKeys[0] === before[2].row_key && ops[0].before === before[0].row_key && after[0].id === before[2].id && after[1].id === before[0].id,
      JSON.stringify({ ops, order: after.slice(0, 3).map((record) => record.id) }),
    )
  })

  await test('a selected column dragged is one fields.move', async () => {
    const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
    await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
    await idle(page)
    const amount = await cell(page, 0, col('amount'))
    const title = await cell(page, 0, col('title'))
    await page.mouse.click(amount.x, box.top + 16)
    const from = await writes(page)
    await page.mouse.move(amount.x, box.top + 16)
    await page.mouse.down()
    await page.mouse.move(title.left + 10, box.top + 16, { steps: 8 })
    await page.mouse.up()
    const made = await waitWrites(page, from)
    const ops = made[0]?.params.ops ?? []
    const columns = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns.map((column) => column.fieldId))
    step('a selected column dragged is one fields.move', made.length === 1 && JSON.stringify(ops) === JSON.stringify([{ op: 'fields.move', tableId: made[0].params.ops[0].tableId, fieldIds: [made.length && ops[0].fieldIds[0]], toIndex: 0 }]) && columns[0] === made[0].params.ops[0].fieldIds[0], JSON.stringify({ ops, columns: columns.slice(0, 3) }))
  })

  await test('a column header that is not selected drags its column', async () => {
    const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
    const a = await cell(page, 0, 0)
    await page.mouse.click(a.x, a.y)
    await idle(page)
    const columns = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns.map((column) => ({ x: column.x, width: column.width, fieldId: column.fieldId })))
    const from = await writes(page)
    // The third column's header, pressed and dragged at once onto the first column's left half.
    await page.mouse.move(box.left + columns[2].x + columns[2].width / 2, box.top + 16)
    await page.mouse.down()
    await page.mouse.move(box.left + columns[0].x + 10, box.top + 16, { steps: 8 })
    await page.mouse.up()
    const made = await waitWrites(page, from)
    const ops = made[0]?.params.ops ?? []
    const order = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns.map((column) => column.fieldId))
    step(
      'a column header that is not selected drags its column (one fields.move)',
      made.length === 1 && ops.length === 1 && ops[0].op === 'fields.move' && ops[0].fieldIds[0] === columns[2].fieldId && ops[0].toIndex === 0 && order[0] === columns[2].fieldId,
      JSON.stringify({ ops, order: order.slice(0, 3), before: columns.slice(0, 3).map((column) => column.fieldId) }),
    )
  })

  await test("a column's edge resizes it with views.patchField", async () => {
    const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
    const column = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns[1])
    const edge = box.left + column.x + column.width - 1
    await page.mouse.move(edge, box.top + 16)
    const cursor = await page.evaluate(() => window.__ktableGrid.view.scroller.style.cursor)
    const from = await writes(page)
    await page.mouse.down()
    await page.mouse.move(edge + 80, box.top + 16, { steps: 8 })
    await page.mouse.up()
    const made = await waitWrites(page, from)
    const width = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns[1].width)
    step("a column's edge resizes it with views.patchField", cursor === 'ew-resize' && made.length === 1 && made[0].command === 'views.patchField' && made[0].params.fieldId === column.fieldId && made[0].params.width === column.width + 80 && width === column.width + 80, JSON.stringify({ cursor, made, width }))
  })

  await test('the frozen edge dragged saves the frozen columns', async () => {
    const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
    const axes = await page.evaluate(() => ({ bodyLeft: window.__ktableGrid.view.currentAxes.bodyLeft, second: window.__ktableGrid.view.currentAxes.columns[1] }))
    const y = box.top + 120
    await page.mouse.move(box.left + axes.bodyLeft, y)
    const cursor = await page.evaluate(() => window.__ktableGrid.view.scroller.style.cursor)
    const from = await writes(page)
    await page.mouse.down()
    await page.mouse.move(box.left + axes.second.x + axes.second.width * 0.75, y, { steps: 8 })
    await page.mouse.up()
    const made = await waitWrites(page, from)
    const frozen = await page.evaluate(() => window.__ktableGrid.view.currentAxes.columns.filter((column) => column.frozen).length)
    step('the frozen edge dragged saves the frozen columns', cursor === 'col-resize' && made.length === 1 && made[0].command === 'views.patchConfig' && made[0].params.frozen_column_count === 2 && frozen === 2, JSON.stringify({ cursor, made, frozen }))
  })

  await test("an attachment's picture is drawn from the document; a click previews it", async () => {
    const order = await shown(page)
    const row = order.findIndex((record) => (record.values[N.files] ?? []).length > 0)
    const files = await page.evaluate((id) => window.__ktableGrid.view.currentAxes.columns.findIndex((column) => column.fieldId === id), (await page.evaluate((name) => window.__ktableGrid.data.fields().find((field) => field.name === name).id, N.files)))
    await cell(page, row, files)
    await idle(page)
    const thumb = await cellPart(page, 2, row, files, 0)
    await page.waitForFunction(() => window.__ktableGrid.view.resources.image(0) != null, null, { timeout: 10_000 })
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
    const pixel = await contentPixel(page, thumb.x, thumb.y)
    step("an attachment's picture is drawn from the document's blob", near(pixel, [220, 20, 30, 255]), JSON.stringify({ pixel, thumb }))
    await page.mouse.click(thumb.x, thumb.y)
    await page.locator('[data-ktable-grid-preview] img').waitFor({ timeout: 5000 })
    const previewed = await page.evaluate(() => window.__ktableGridEvents.filter((event) => event.name === 'preview').at(-1)?.detail)
    step('a click on a picture previews the cell\'s attachments', previewed?.index === 0 && previewed?.count === 1, JSON.stringify(previewed))
    await page.keyboard.press('Escape')
  })

  const filesCol = () => page.evaluate((name) => {
    const { view, data } = window.__ktableGrid
    const id = data.fields().find((field) => field.name === name).id
    return view.currentAxes.columns.findIndex((column) => column.fieldId === id)
  }, N.files)
  const rowOf = async (title) => (await shown(page)).findIndex((record) => record.values[N.title] === title)

  await test('a picture at a web address is drawn', async () => {
    const files = await filesCol()
    // Record 4 (its title is the input method's by now).
    const row = (await shown(page)).findIndex((record) => (record.values[N.files] ?? []).some((item) => item.url === WEB_PICTURE))
    await cell(page, row, files)
    await idle(page)
    const thumb = await cellPart(page, 2, row, files, 0)
    let pixel = []
    for (let i = 0; i < 50 && !near(pixel, [...BLUE, 255]); i += 1) {
      await page.waitForTimeout(100)
      pixel = await contentPixel(page, thumb.x, thumb.y)
    }
    step('a picture at a web address is drawn', near(pixel, [...BLUE, 255]), JSON.stringify({ pixel, thumb }))
  })

  await test("the active attachment cell's add button picks files and adds them in one records.update", async () => {
    const files = await filesCol()
    const row = await rowOf('Record 5')
    const at = await cell(page, row, files)
    // Not active yet: no button, a press only selects the cell.
    const chooserBefore = page.waitForEvent('filechooser', { timeout: 1000 }).then(() => true, () => false)
    await page.mouse.click(at.left + 16, at.y)
    const early = await chooserBefore
    await idle(page)
    const from = await writes(page)
    const chooser = page.waitForEvent('filechooser', { timeout: 5000 })
    await page.mouse.click(at.left + 16, at.y)
    await (await chooser).setFiles({ name: 'green.png', mimeType: 'image/png', buffer: png(4, 4, [20, 200, 40]) })
    const made = await waitWrites(page, from, 2)
    const value = (await valueAt(page, row, N.files)) ?? []
    step(
      "the active attachment cell's add button picks files and adds them in one records.update",
      !early && made.length === 2 && made[0].command === 'attachments.put' && made[1].command === 'records.update' && value.length === 1 && value[0].name === 'green.png' && String(value[0].url).startsWith('kition-blob:'),
      JSON.stringify({ early, made: made.map((write) => write.command), value }),
    )
  })

  await test('files pasted on an attachment cell are added to it', async () => {
    const files = await filesCol()
    const row = await rowOf('Record 5')
    const at = await cell(page, row, files)
    // Its right end: neither the add button nor a picture.
    await page.mouse.click(at.left + at.width - 6, at.y)
    await idle(page)
    const from = await writes(page)
    await page.evaluate((bytes) => {
      const data = new DataTransfer()
      data.items.add(new File([new Uint8Array(bytes)], 'pasted.png', { type: 'image/png' }))
      document.querySelector('[data-ktable-grid-input]').dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }))
    }, [...png(4, 4, [200, 200, 20])])
    const made = await waitWrites(page, from, 2)
    const value = (await valueAt(page, row, N.files)) ?? []
    step(
      'files pasted on an attachment cell are added to it',
      made.length === 2 && made[1].command === 'records.update' && value.map((item) => item.name).join() === 'green.png,pasted.png',
      JSON.stringify({ made: made.map((write) => write.command), value: value.map((item) => item.name) }),
    )
  })

  await test('an AI cell shows a regenerate button on hover and a spinner while it generates', async () => {
    const summary = await page.evaluate((id) => window.__ktableGrid.view.currentAxes.columns.findIndex((column) => column.fieldId === id), made.ids.summary)
    const at = await cell(page, 1, summary)
    await idle(page)
    const button = { x: at.left + at.width - 4 - 9, y: at.top + 4 + 9 }
    await page.mouse.move(at.x, at.y)
    await page.mouse.move(button.x, button.y)
    const brand = await page.evaluate(() => window.__ktableGrid.interaction.style.border)
    const rgb = [1, 3, 5].map((i) => Number.parseInt(brand.slice(i, i + 2), 16))
    const brandIn = (x, y, size) => brandPixels(page, x, y, size, rgb)
    const onButton = await brandIn(button.x, button.y, 16)
    const offButton = await brandIn(at.left + 30, at.y, 16)
    step('a hovered AI cell shows its regenerate button', onButton > 0 && offButton === 0, JSON.stringify({ onButton, offButton }))
    await page.mouse.click(button.x, button.y)
    const asked = await page.evaluate(() => window.__ktableGridEvents.filter((event) => event.name === 'regenerate').at(-1)?.detail)
    const recordId = await page.evaluate(() => window.__ktableGrid.interaction.rows.recordId(1))
    await page.mouse.move(at.left - 300, at.y)
    const draws = await page.evaluate(() => window.__ktableGrid.view.overlayDraws)
    await page.waitForTimeout(300)
    const later = await page.evaluate(() => window.__ktableGrid.view.overlayDraws)
    const spinner = await brandIn(at.x, at.y, 24)
    step('the regenerate button asks for its cell; a spinner turns over it', asked?.recordId === recordId && asked?.fieldId === made.ids.summary && later - draws >= 5 && spinner > 0, JSON.stringify({ asked, draws: later - draws, spinner }))
    await page.evaluate(() => window.__ktableGrid.editing.setGenerating([]))
  })

  await test('undo takes back one action', async () => {
    const before = await shown(page)
    const at = await cell(page, 0, col('title'))
    await page.mouse.click(at.x, at.y)
    const from = await writes(page)
    await page.keyboard.type('Undo me')
    await page.keyboard.press('Enter')
    await waitWrites(page, from)
    await page.mouse.click(at.x, at.y)
    await page.keyboard.press(`${mod}+z`)
    await page.waitForFunction(() => window.__ktableGridEvents.some((event) => event.name === 'undo:done'), null, { timeout: 10_000 })
    await idle(page)
    step('undo takes back one action', (await valueAt(page, 0, N.title)) === before[0].values[N.title], JSON.stringify(await valueAt(page, 0, N.title)))
  })

  step('no page errors', errors.length === 0, errors.slice(0, 3).join(' | '))
  await context.close()
} finally {
  await browser.close()
  await server.close()
}

console.log(failures ? `${failures} FAILED (${which})` : `ALL PASS (${which})`)
process.exit(failures ? 1 : 0)
