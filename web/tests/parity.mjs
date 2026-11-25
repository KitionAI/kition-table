// Screen == picture, first layer (grid plan section 5.1): the same display list drawn by the page's
// Canvas 2D player and by the engine's rasterizer (vello_cpu) must agree, region by region:
// node tests/parity.mjs [chromium|webkit]
//
// A fixture of every cell kind (Latin, ligature candidates, CJK, Hangul, emoji, characters no face
// covers, long text in tall rows, chips, checkboxes, ratings, dates, people, attachments), grouped
// and frozen views, light and dark, a changed brand color, at device pixel ratios 1 and 2 and at two
// scroll positions. For each band the viewport shows, `gridBands` gives the list the page plays into
// an offscreen canvas, and `gridRaster` gives the engine's pixels of the same list. Pixels are
// compared by region, with masks the list itself gives (TEXT boxes, curved shapes, the engine's tiles (emoji, icons) and tofu,
// and the flat rest), at the tolerances the case table.editor.grid/parity holds (calibrated on macOS
// in spike TG-0c; cases/table.editor.grid/parity/expected/tiers.json). Text runs that cross a
// band's edge are not compared.
//
// The grid sets its text at 10 to 13 px, under Inter's optical size axis's floor (14), where the
// page's Canvas draws Inter alike whether it follows the font size (Chromium: the engine measures
// with `auto`) or keeps the default optical size (WebKit: `fixed-default`). So in the light grid at
// both ratios the page also compares a specimen at 16 and at 20 px (weights 400 and 600, Latin and a
// mixed line): `gridSpecimen` sets the lines with the grid's shaping and text ops in the page's
// engine, which measures and rasterizes with the optical sizing the page found, and answers its
// display list and raster; the page plays the list and both are compared with the same tiers.
//
// The text tiers that depend on the platform's font rasterizer (mean difference, PSNR) are
// calibrated for macOS only: elsewhere only the flat, curve, emoji and text geometry tiers run.
// Every run's widths are checked too: each TEXT run's engine width against the page's
// measureText, within 0.5 CSS px (a warning above 0.1 px). PARITY_DUMP=1 writes each band's page,
// engine and difference images to target/parity/<browser>/.
import fs from 'node:fs'
import path from 'node:path'
import { REPO, checkPackage, launch, nodeEngine, openGrid, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
const macos = process.platform === 'darwin'
const OUT = path.join(REPO, 'target', 'parity', which)

/** The tolerances: the expected result of case table.editor.grid/parity (only a maintainer changes it). */
export const TIERS = JSON.parse(fs.readFileSync(path.join(REPO, 'cases', 'table.editor.grid', 'parity', 'expected', 'tiers.json'), 'utf8'))

checkPackage()

/** The fixture, made with the engine's own requests; answers its bytes and view ids. */
async function fixture() {
  const sdk = await nodeEngine()
  const doc = sdk.TableDocument.create('Parity.ktable', 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('file.get').tables[0]
  const tableId = table.id
  const grid = table.views[0].id
  call('table.set', { tableId, title: 'Parity' })
  const primary = table.fields.find((field) => field.is_primary)
  for (const field of table.fields.filter((field) => !field.is_primary)) call('field.delete', { tableId, fieldId: field.id })
  const field = (title, type, options) => {
    const made = call('field.add', { tableId, title, type, ...(options ? { options } : {}) })
    return made.field ?? made
  }
  const notes = field('Notes', 'long_text')
  const amount = field('Amount', 'number')
  const status = field('Status', 'single_select', { choices: ['To do', 'In progress', 'Done', '待处理'] })
  const tags = field('Tags', 'multi_select', { choices: ['red', 'green', 'design', '市场', '디자인'] })
  const done = field('Done', 'checkbox')
  const score = field('Score', 'rating', { max: 5, icon: 'star' })
  const due = field('Due', 'date')
  const owner = field('Owner', 'user')
  const files = field('Files', 'attachment')
  const titles = [
    'Quarterly planning WAVE',
    'Typography -> => != 1/2 x2 office',
    '季度计划 Q3 客户反馈',
    '서울 지사 회의록',
    '東京オフィス 移転',
    'Launch day \u{1F680} \u{2705}',
    'Family \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} and \u{1F44D}\u{1F3FD}',
    'Ethiopic \u{1230}\u{120B}\u{121D} and Khmer \u{1781}\u{17D2}\u{1798}\u{17C2}\u{179A}',
    'A long title that will not fit into its column and is cut with an ellipsis at the end',
    'Mixed 中文 English 한국어 text',
  ]
  // A whole 4 x 4 PNG of one color: both sides decode it and draw it alike (a picture the engine
  // cannot decode stays a placeholder there, while a browser may still show part of it).
  const pixel = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAEElEQVR42mMIc70CRwzEcQBM0hbx9EnJcgAAAABJRU5ErkJggg==', 'base64')
  const blob = call('attachment.add', { tableId, name: 'swatch.png', mimeType: 'image/png', data: pixel.toString('base64') })
  const attachment = blob.attachment ?? blob
  const records = []
  for (let i = 0; i < 80; i += 1) {
    records.push({
      [primary.name]: `${titles[i % titles.length]}${i >= titles.length ? ` #${i + 1}` : ''}`,
      [notes.name]: i % 3 === 0 ? `Line one of the note, long enough to wrap in a tall row ${i}.\nSecond line 第二行 둘째 줄.` : `Short note ${i}`,
      [amount.name]: i % 4 === 0 ? 1234567.89 + i : i * 12.5,
      [status.name]: ['To do', 'In progress', 'Done', '待处理'][i % 4],
      [tags.name]: [['red'], ['green', 'design'], ['市场', '디자인'], []][i % 4],
      [done.name]: i % 2 === 0,
      [score.name]: i % 6,
      [due.name]: `2026-0${1 + (i % 9)}-1${i % 10}`,
      [owner.name]: [{ id: `u${i % 3}`, name: ['Ada Lovelace', '李雷', 'Grace Hopper'][i % 3] }],
      ...(i % 5 === 0 ? { [files.name]: [attachment] } : {}),
    })
  }
  call('edit.transact', { ops: [{ op: 'records.create', tableId, at: null, records }] })
  call('tableView.setConfig', { tableId, viewId: grid, frozen_column_count: 1 })
  const tall = call('tableView.add', { tableId, title: 'Tall', type: 'grid' })
  const tallId = tall.id ?? tall.view.id
  call('tableView.setConfig', { tableId, viewId: tallId, row_height: 'extra_tall' })
  const grouped = call('tableView.add', { tableId, title: 'Grouped', type: 'grid' })
  const groupedId = grouped.id ?? grouped.view.id
  call('tableView.setGroups', { tableId, viewId: groupedId, items: [{ field_id: status.id, direction: 'asc' }] })
  doc.markSaved()
  const bytes = doc.save()
  doc.free()
  return { bytes, views: { grid, tall: tallId, grouped: groupedId } }
}

/** The specimen's lines at `size` px: short runs (the width check takes runs up to 200 px). */
const specimenLines = (size) => ['Quarterly WAVE', 'office -> 1/2 fi', 'Mixed 中文 한국어'].flatMap((text) => [400, 600].map((weight) => ({ text, weight, size })))
const SPECIMEN_SIZES = [16, 20]

/**
 * Compares every band the viewport shows, or, with `specimen` (`[{text, weight, size}]`),
 * `gridSpecimen`'s list of those lines; runs in the page.
 */
async function compareBands({ dpr, dump, specimen }) {
  const { view, engine } = window.__ktableGrid
  const player = await import('/src/editor/table/canvas/player.ts')
  const list = await import('/src/editor/table/canvas/displayList.ts')
  const { Opcode } = list
  const out = []
  const widths = []
  const measure = document.createElement('canvas').getContext('2d')
  const families = ['KitionTableSans', 'KitionTableFallback', 'KitionTableHangul', 'KitionTableMono']
  // The bands the viewport shows (all in the cache once the view has settled), each rasterized by
  // gridRaster; or the specimen's list with the raster gridSpecimen made of it.
  const sources = []
  if (specimen) {
    const bytes = await engine.invoke('gridSpecimen', JSON.stringify(specimen), dpr)
    const length = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0, true)
    const band = list.decodeList(bytes.subarray(4, 4 + length))
    sources.push({ key: band.header.key, band, given: bytes.subarray(4 + Math.ceil(length / 4) * 4) })
  } else
    for (const key of view.neededKeys()) {
      const band = view.cache.peek(key)
      if (band) sources.push({ key, band, given: null })
    }
  for (const { key, band, given } of sources) {
    const h = band.header
    const W = Math.round(h.width * dpr)
    const H = Math.round(h.height * dpr)
    if (W === 0 || H === 0) continue
    const canvas = document.createElement('canvas')
    canvas.width = W
    canvas.height = H
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    player.play({ ctx, x: h.x, y: h.y, dpr }, band, view.resources, null)
    const page = ctx.getImageData(0, 0, W, H).data
    const raster = given ?? (await engine.invoke('gridRaster', h.epoch, key.layer, key.colBand, key.rowBand, dpr))
    const dv = new DataView(raster.buffer, raster.byteOffset, raster.byteLength)
    const rw = dv.getUint32(0, true)
    const rh = dv.getUint32(4, true)
    if (rw !== W || rh !== H) {
      out.push({ key, error: `raster ${rw}x${rh}, page ${W}x${H}` })
      continue
    }
    const rust = raster.subarray(8)
    // Masks: 3 emoji, icons and tofu (the engine's tiles, the same box), 1 text, 2 curves, 0 flat.
    const mask = new Uint8Array(W * H)
    const fill = (x0, y0, x1, y1, value, over) => {
      x0 = Math.max(0, Math.floor(x0))
      y0 = Math.max(0, Math.floor(y0))
      x1 = Math.min(W, Math.ceil(x1))
      y1 = Math.min(H, Math.ceil(y1))
      for (let y = y0; y < y1; y += 1)
        for (let x = x0; x < x1; x += 1) {
          const at = y * W + x
          if (mask[at] === 0 || (over && mask[at] < value && mask[at] !== 3)) mask[at] = value
        }
    }
    const dx = (v) => (v - h.x) * dpr
    const dy = (v) => (v - h.y) * dpr
    const runs = []
    const { packedColor } = await import('/src/editor/table/canvas/resources.ts')
    for (const op of band.ops) {
      if (op.op === Opcode.Emoji || op.op === Opcode.Icon) {
        // The tile's own pixels, where the page puts them (its snapped pen or box and offset).
        const tile =
          op.op === Opcode.Emoji
            ? view.resources.emoji(op.atlas, op.size, dpr)
            : view.resources.icon(op.icon, op.size, op.stroke, op.solid, packedColor(band.rgba, op.paint), dpr)
        if (!tile) continue
        const alpha = tile.image.getContext('2d').getImageData(0, 0, tile.image.width, tile.image.height).data
        const left = Math.round(dx(op.x)) + tile.left
        const top = Math.round(dy(op.y)) + tile.top
        for (let y = 0; y < tile.image.height; y += 1)
          for (let x = 0; x < tile.image.width; x += 1) {
            const px = left + x
            const py = top + y
            if (alpha[(y * tile.image.width + x) * 4 + 3] > 0 && px >= 0 && py >= 0 && px < W && py < H) mask[py * W + px] = 3
          }
      } else if (op.op === Opcode.Tofu) fill(Math.round(dx(op.x)), Math.round(dy(op.y)), Math.round(dx(op.x + op.w)), Math.round(dy(op.y + op.h)), 3, true)
    }
    for (const op of band.ops) {
      if (op.op !== Opcode.Text) continue
      const left = player.runLeft(op.align, op.x, op.width)
      const box = { x0: dx(left) - 1, y0: dy(op.y - 1.1 * op.size) - 1, x1: dx(left + op.width) + 1, y1: dy(op.y + 0.35 * op.size) + 1 }
      fill(box.x0, box.y0, box.x1, box.y1, 1, false)
      const inside = left >= h.x && left + op.width <= h.x + h.width && op.y - 1.1 * op.size >= h.y && op.y + 0.35 * op.size <= h.y + h.height
      runs.push({ op, left, box, inside })
      measure.font = `${op.weight} ${op.size}px ${families[op.face]}`
      if ('fontKerning' in measure) measure.fontKerning = 'normal'
      if (op.width <= 200) widths.push({ text: op.text, face: op.face, weight: op.weight, size: op.size, engine: op.width, page: measure.measureText(op.text).width })
    }
    for (const op of band.ops) {
      const curve = op.op === Opcode.RRect || op.op === Opcode.RRectStroke || op.op === Opcode.Image
      // Text in a chip stays text: curves take only what no text box or tile took.
      if (curve) fill(dx(op.x) - 2, dy(op.y) - 2, dx(op.x + op.w) + 2, dy(op.y + op.h) + 2, 2, false)
      else if (op.op === Opcode.Shadow) fill(dx(op.x - 3 * op.blur) - 2, dy(op.y - 3 * op.blur) - 2, dx(op.x + op.w + 3 * op.blur) + 2, dy(op.y + op.h + 3 * op.blur) + 2, 2, false)
    }
    const tiers = { flat: { n: 0, maxAbs: 0, diff0: 0, sq: 0 }, curve: { n: 0, maxAbs: 0, diff2: 0, sq: 0 }, glyphTile: { n: 0, maxAbs: 0, sq: 0 }, text: { n: 0, sq: 0 } }
    let sqAll = 0
    for (let p = 0, i = 0; p < W * H; p += 1, i += 4) {
      let m = 0
      let sq = 0
      for (let c = 0; c < 3; c += 1) {
        const d = Math.abs(page[i + c] - rust[i + c])
        if (d > m) m = d
        sq += d * d
      }
      sqAll += sq
      const kind = mask[p]
      if (kind === 0) {
        tiers.flat.n += 1
        tiers.flat.sq += sq
        if (m > tiers.flat.maxAbs) tiers.flat.maxAbs = m
        if (m > 0) tiers.flat.diff0 += 1
      } else if (kind === 2) {
        tiers.curve.n += 1
        tiers.curve.sq += sq
        if (m > tiers.curve.maxAbs) tiers.curve.maxAbs = m
        if (m > 2) tiers.curve.diff2 += 1
      } else if (kind === 3) {
        tiers.glyphTile.n += 1
        tiers.glyphTile.sq += sq
        if (m > tiers.glyphTile.maxAbs) tiers.glyphTile.maxAbs = m
      } else {
        tiers.text.n += 1
        tiers.text.sq += sq
      }
    }
    // The curved shape with the largest difference (for a failure's message).
    let worstCurve = null
    for (const op of band.ops) {
      if (![Opcode.RRect, Opcode.RRectStroke, Opcode.Image].includes(op.op)) continue
      for (let y = Math.max(0, Math.floor(dy(op.y)) - 2); y < Math.min(H, Math.ceil(dy(op.y + op.h)) + 2); y += 1)
        for (let x = Math.max(0, Math.floor(dx(op.x)) - 2); x < Math.min(W, Math.ceil(dx(op.x + op.w)) + 2); x += 1) {
          if (mask[y * W + x] !== 2) continue
          const i = (y * W + x) * 4
          const m = Math.max(Math.abs(page[i] - rust[i]), Math.abs(page[i + 1] - rust[i + 1]), Math.abs(page[i + 2] - rust[i + 2]))
          if (!worstCurve || m > worstCurve.m) worstCurve = { m, op: op.op, at: [x, y] }
        }
    }
    const psnr = (sq, n) => (sq === 0 || n === 0 ? 99 : 10 * Math.log10((255 * 255) / (sq / (n * 3))))
    const segments = []
    for (const run of runs) {
      if (!run.inside || !run.op.text.trim()) continue
      const { op, box } = run
      const fg = band.rgba.subarray(op.paint * 4, op.paint * 4 + 3)
      // The run's own advance box, without its leading and trailing spaces (a neighbour's ink may
      // reach into them).
      measure.font = `${op.weight} ${op.size}px ${families[op.face]}`
      const lead = measure.measureText(op.text.slice(0, op.text.length - op.text.trimStart().length)).width
      const body = measure.measureText(op.text.trim()).width
      const ax0 = Math.max(0, Math.round(dx(run.left + lead)))
      const ax1 = Math.min(W, Math.round(dx(run.left + lead + body)))
      const y0 = Math.max(0, Math.floor(box.y0))
      const y1 = Math.min(H, Math.ceil(box.y1))
      const corner = (y0 * W + Math.max(0, Math.floor(box.x0))) * 4
      const bg = [rust[corner], rust[corner + 1], rust[corner + 2]]
      const contrast = (Math.abs(fg[0] - bg[0]) + Math.abs(fg[1] - bg[1]) + Math.abs(fg[2] - bg[2])) / 3
      if (contrast < 24) continue
      const ink = (img) => {
        let minX = Infinity
        let maxX = -1
        let minY = Infinity
        let maxY = -1
        let mass = 0
        let moment = 0
        for (let y = y0; y < y1; y += 1)
          for (let x = ax0; x < ax1; x += 1) {
            const i = (y * W + x) * 4
            const d = (Math.abs(img[i] - bg[0]) + Math.abs(img[i + 1] - bg[1]) + Math.abs(img[i + 2] - bg[2])) / 3
            mass += d
            moment += d * x
            if (d > contrast / 2) {
              if (x < minX) minX = x
              if (x > maxX) maxX = x
              if (y < minY) minY = y
              if (y > maxY) maxY = y
            }
          }
        return { minX, maxX, minY, maxY, centroid: mass ? moment / mass : 0, any: maxX >= 0 }
      }
      const a = ink(page)
      const b = ink(rust)
      let sum = 0
      let n = 0
      for (let y = y0; y < y1; y += 1)
        for (let x = Math.max(0, Math.floor(box.x0)); x < Math.min(W, Math.ceil(box.x1)); x += 1) {
          const i = (y * W + x) * 4
          for (let c = 0; c < 3; c += 1) sum += Math.abs(page[i + c] - rust[i + c])
          n += 3
        }
      segments.push({
        text: op.text,
        font: `${op.face} ${op.weight} ${op.size}`,
        at: [Math.round(box.x0), Math.round(box.y0), key.layer, key.rowBand],
        edge: a.any && b.any ? Math.max(Math.abs(a.minX - b.minX), Math.abs(a.maxX - b.maxX), Math.abs(a.minY - b.minY), Math.abs(a.maxY - b.maxY)) : 0,
        centroid: a.any && b.any ? Math.abs(a.centroid - b.centroid) : 0,
        mean: n ? sum / n : 0,
      })
    }
    const images = {}
    if (dump) {
      // The page's pixels, the engine's, and their difference (x4, red), for looking at a failure.
      const png = (pixels) => {
        const c = document.createElement('canvas')
        c.width = W
        c.height = H
        const image = new ImageData(W, H)
        image.data.set(pixels)
        c.getContext('2d').putImageData(image, 0, 0)
        return c.toDataURL('image/png')
      }
      const diff = new Uint8ClampedArray(W * H * 4)
      for (let i = 0; i < W * H * 4; i += 4) {
        const d = Math.min(255, 4 * Math.max(Math.abs(page[i] - rust[i]), Math.abs(page[i + 1] - rust[i + 1]), Math.abs(page[i + 2] - rust[i + 2])))
        diff.set([255, 255 - d, 255 - d, 255], i)
      }
      images.page = png(page)
      images.rust = png(rust)
      images.diff = png(diff)
    }
    out.push({ key, W, H, tiers, textPsnr: psnr(tiers.text.sq, tiers.text.n), wholePsnr: psnr(sqAll, W * H), segments, images, worstCurve })
  }
  return { bands: out, widths }
}

const { bytes, views } = await fixture()
const { server, origin } = await startServer()
const browser = await launch(which)
const version = browser.version()
fs.mkdirSync(OUT, { recursive: true })
let failures = 0
let warnings = 0
const step = (name, ok, detail) => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}

/** `specimen`: the run also compares the specimen at 16 and 20 px. */
const RUNS = [
  { name: 'grid light', view: views.grid, dpr: 1, specimen: true },
  { name: 'grid light', view: views.grid, dpr: 2, specimen: true },
  { name: 'grid light scrolled', view: views.grid, dpr: 2, scroll: [900, 700] },
  { name: 'grid dark', view: views.grid, dpr: 1, theme: 'dark' },
  { name: 'grid dark', view: views.grid, dpr: 2, theme: 'dark' },
  { name: 'grid brand', view: views.grid, dpr: 2, brand: '#c2410c' },
  { name: 'tall rows', view: views.tall, dpr: 2 },
  { name: 'tall rows', view: views.tall, dpr: 1 },
  { name: 'grouped', view: views.grouped, dpr: 2 },
  { name: 'grouped', view: views.grouped, dpr: 1, scroll: [0, 900] },
]

try {
  for (const run of RUNS) {
    const label = `${run.name} @${run.dpr}`
    const query = { file: '/__grid/parity.ktable', view: String(run.view), ...(run.theme ? { theme: run.theme } : {}), ...(run.brand ? { brand: run.brand } : {}) }
    const { context, page, errors } = await openGrid(browser, origin, { dpr: run.dpr, viewport: { width: 1280, height: 800 }, files: { 'parity.ktable': bytes }, query })
    await page.evaluate(() => window.__ktableGrid.view.allFonts())
    if (run === RUNS[0]) {
      const sizing = await page.evaluate(() => window.__ktableGrid.view.textSizing)
      console.log(`INFO optical sizing - the page's Canvas: ${sizing?.sizing} (probe ratio ${sizing?.ratio?.toFixed(4)}${sizing?.measured ? '' : ', not measured'}); the engine lays out and rasterizes with the same`)
    }
    if (run.scroll) await page.evaluate(([x, y]) => window.__ktableGrid.view.scrollTo(x, y), run.scroll)
    await settled(page)
    // The pictures the bands show, decoded by the page (the engine's raster decodes its own).
    await page
      .waitForFunction(
        () => {
          const { view } = window.__ktableGrid
          return view.neededKeys().every((key) => (view.cache.peek(key)?.ops ?? []).every((op) => op.op !== 0x09 || view.resources.image(op.key)))
        },
        null,
        { timeout: 10_000, polling: 50 },
      )
      .catch(() => console.log('INFO pictures not all decoded on the page after 10 s'))
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
    const result = await page.evaluate(compareBands, { dpr: run.dpr, dump: Boolean(process.env.PARITY_DUMP) })
    await page.screenshot({ path: path.join(OUT, `${label.replace(/\W+/g, '-')}.png`) })
    check(label, run, result)
    for (const size of run.specimen ? SPECIMEN_SIZES : []) {
      const lines = specimenLines(size)
      const compared = await page.evaluate(compareBands, { dpr: run.dpr, dump: Boolean(process.env.PARITY_DUMP), specimen: lines })
      check(`${label} text ${size} px`, run, compared)
      const widths = compared.widths.map((width) => `${width.weight} "${width.text}" ${Math.abs(width.page - width.engine).toFixed(3)}`)
      console.log(`INFO ${label} text ${size} px - ${lines.length} lines; |engine - measureText| per run: ${widths.join(', ')}`)
    }
    step(`${label}: page errors`, errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
    await context.close()
  }
} catch (error) {
  failures += 1
  console.log(`FAIL parity - ${error instanceof Error ? error.stack : error}`)
} finally {
  await browser.close()
  await server.close()
}
console.log(`${failures ? 'FAIL' : 'PASS'} parity (${which} ${version}${macos ? '' : ', geometry tiers only'}${warnings ? `, ${warnings} width warnings` : ''})`)
process.exit(failures ? 1 : 0)

/** The tiers of one comparison (the viewport's bands, or a specimen), as steps under `label`. */
function check(label, run, result) {
  for (const band of result.bands) {
    for (const [kind, url] of Object.entries(band.images ?? {})) {
      const name = `${label}-${band.key.layer}-${band.key.colBand}-${band.key.rowBand}-${kind}.png`.replace(/[^\w.-]+/g, '-')
      fs.writeFileSync(path.join(OUT, name), Buffer.from(url.split(',')[1], 'base64'))
    }
    delete band.images
  }
  const bands = result.bands
  step(`${label}: bands compared`, bands.length > 0 && bands.every((band) => !band.error), bands.filter((band) => band.error).map((band) => band.error).join('; ') || `${bands.length} bands`)
  const ok = bands.filter((band) => !band.error)
  const worst = (pick) => Math.max(0, ...ok.map(pick))
  const least = (pick) => Math.min(99, ...ok.map(pick))
  const flatShare = worst((band) => band.tiers.flat.diff0 / Math.max(1, band.tiers.flat.n))
  step(`${label}: flat`, worst((band) => band.tiers.flat.maxAbs) <= TIERS.flat.maxAbs && flatShare <= TIERS.flat.diffShare, `max_abs ${worst((band) => band.tiers.flat.maxAbs)}, differing ${(flatShare * 100).toFixed(3)}%`)
  const curveShare = worst((band) => band.tiers.curve.diff2 / Math.max(1, band.tiers.curve.n))
  const curveMax = worst((band) => band.tiers.curve.maxAbs)
  const curveAt = ok.find((band) => band.tiers.curve.maxAbs === curveMax)?.worstCurve
  step(`${label}: curves`, curveMax <= TIERS.curve.maxAbs && curveShare <= TIERS.curve.diff2Share, `max_abs ${curveMax}${curveMax > TIERS.curve.maxAbs && curveAt ? ` (op ${curveAt.op} at ${curveAt.at})` : ''}, > 2 ${(curveShare * 100).toFixed(2)}%`)
  step(`${label}: icon and emoji tiles, tofu`, worst((band) => band.tiers.glyphTile.maxAbs) <= TIERS.glyphTile.maxAbs, `max_abs ${worst((band) => band.tiers.glyphTile.maxAbs)} over ${ok.reduce((sum, band) => sum + band.tiers.glyphTile.n, 0)} px`)
  const segments = ok.flatMap((band) => band.segments)
  const edge = Math.max(0, ...segments.map((segment) => segment.edge))
  const centroid = Math.max(0, ...segments.map((segment) => segment.centroid))
  const widestEdge = segments.find((segment) => segment.edge === edge)
  step(`${label}: text ink box`, edge <= TIERS.inkEdge[run.dpr], `${edge} ≤ ${TIERS.inkEdge[run.dpr]} device px over ${segments.length} runs${edge > TIERS.inkEdge[run.dpr] ? ` ("${widestEdge.text}" at ${widestEdge.at})` : ''}`)
  step(`${label}: text centroid`, centroid <= TIERS.centroid[run.dpr], `${centroid.toFixed(2)} ≤ ${TIERS.centroid[run.dpr]} device px`)
  if (process.env.PARITY_DUMP) {
    const share = (name) => ok.reduce((sum, band) => sum + band.tiers[name].sq, 0)
    const all = ['flat', 'curve', 'glyphTile', 'text'].map((name) => [name, share(name)])
    const total = all.reduce((sum, [, sq]) => sum + sq, 0) || 1
    console.log(`  squared error by region: ${all.map(([name, sq]) => `${name} ${((100 * sq) / total).toFixed(1)}%`).join(', ')}`)
    const meanest = [...segments].sort((a, b) => b.mean - a.mean).slice(0, 6)
    console.log(`  largest text differences: ${meanest.map((segment) => `"${segment.text}" (${segment.font}) ${segment.mean.toFixed(1)} at ${segment.at}`).join('; ')}`)
  }
  if (macos) {
    const mean = Math.max(0, ...segments.map((segment) => segment.mean))
    step(`${label}: text mean difference`, mean <= TIERS.segmentMean, `${mean.toFixed(1)} ≤ ${TIERS.segmentMean}`)
    step(`${label}: text PSNR`, least((band) => band.textPsnr) >= TIERS.textPsnr, `${least((band) => band.textPsnr).toFixed(1)} ≥ ${TIERS.textPsnr} dB`)
    // Light text on dark is drawn heavier by macOS than by the engine: the dark theme has its own line.
    const whole = (run.theme === 'dark' ? TIERS.wholePsnrDark : TIERS.wholePsnr)[run.dpr]
    step(`${label}: whole PSNR`, least((band) => band.wholePsnr) >= whole, `${least((band) => band.wholePsnr).toFixed(1)} ≥ ${whole} dB`)
  }
  const widths = result.widths.map((width) => ({ ...width, diff: Math.abs(width.page - width.engine) }))
  const widest = widths.reduce((worst, width) => (width.diff > (worst?.diff ?? -1) ? width : worst), null)
  step(`${label}: text widths`, widths.length > 0 && widths.every((width) => width.diff <= TIERS.width), widest ? `max ${widest.diff.toFixed(3)} px ("${widest.text}" ${widest.weight} ${widest.size}px) over ${widths.length} runs` : 'no runs')
  const loose = widths.filter((width) => width.diff > TIERS.widthWarn)
  if (loose.length) {
    warnings += 1
    console.log(`WARN ${label}: ${loose.length} runs differ by more than ${TIERS.widthWarn} px`)
  }
}
