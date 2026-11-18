// The grid's edits against the lines of the grid plan (section 4.2), on the development page:
// node tests/perfEdit.mjs [chromium]
//
// Scenario (g): 50 cells edited one after another (a typed value, then Enter, which moves down),
// each timed from the Enter to the first frame that draws the written value (every band the
// viewport shows exact again, the write done): edit-to-redraw p95 against the line (50 ms; the
// headless tier relaxes it three times, as tests/perf.mjs does). Then a whole number column of
// the 100,000 records cleared with Delete, timed the same way and checked in the engine.
//
// The fixture is tests/perf.mjs's (bindings/wasm/examples/perf_fixture.rs, in target/perf/; it is
// written when missing). PERF_TIER=strict checks the line itself (owner's machine, PERF_HEADED=1);
// PERF_ROWS changes the record count; PERF_OUT names a JSON report.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { REPO, checkPackage, launch, openGrid, quantile, serveFile, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
const tier = process.env.PERF_TIER ?? 'ci'
/** The plan's line for one edit to its redraw (section 4.2), and the headless tier's factor (lines.json's relax). */
const EDIT_P95_MS = 50
const RELAX = JSON.parse(fs.readFileSync(path.join(REPO, 'cases', 'table.editor.grid', 'scroll-perf', 'expected', 'lines.json'), 'utf8')).relax
const k = tier === 'strict' ? 1 : RELAX
const EDITS = 50
const rows = Number(process.env.PERF_ROWS ?? 100_000)
const fixture = path.join(REPO, 'target', 'perf', `grid-${rows}.ktable`)
const views = path.join(REPO, 'target', 'perf', `grid-${rows}.json`)

checkPackage()
if (!fs.existsSync(fixture) || !fs.existsSync(views)) {
  console.log(`writing ${path.relative(REPO, fixture)}`)
  const out = execFileSync('cargo', ['run', '--release', '-q', '-p', 'kition-table-kit', '--example', 'perf_fixture', '--', fixture, String(rows)], { cwd: REPO, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] })
  fs.writeFileSync(views, out.trim().split('\n').pop())
}
const made = JSON.parse(fs.readFileSync(views, 'utf8'))

const { server, origin } = await startServer()
const served = await serveFile(fixture)
const browser = await launch(which, process.env.PERF_HEADED !== '1')
let failures = 0
const results = {}
const step = (name, ok, detail) => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name} - ${detail}`)
}
const f = (value) => value.toFixed(1)

/**
 * Times every Enter or Delete from its key to the first content frame after which the write is
 * done and every band the viewport shows is exact: `window.__ktableEditTimes`.
 */
function installProbe() {
  const { view, editing } = window.__ktableGrid
  const times = []
  window.__ktableEditTimes = times
  let started = 0
  window.addEventListener(
    'keydown',
    (event) => {
      if ((event.key === 'Enter' && editing.editing) || event.key === 'Delete') started = performance.now()
    },
    true,
  )
  view.onFrame(() => {
    if (started && editing.busy === 0 && view.complete()) {
      times.push(performance.now() - started)
      started = 0
    }
  })
}

try {
  const { context, page, errors } = await openGrid(browser, origin, { dpr: 2, query: { file: served.url, view: String(made.views.all) } })
  await settled(page, 120_000)
  await page.evaluate(installProbe)
  const columns = await page.evaluate(() => {
    const { view, data } = window.__ktableGrid
    const types = view.currentAxes.columns.map((column) => (column.fieldId == null ? null : data.field(column.fieldId)?.type ?? null))
    return { text: types.findIndex((type, index) => index > 0 && type === 'text'), number: types.indexOf('number') }
  })
  const box = await page.evaluate(() => window.__ktableGrid.view.scroller.getBoundingClientRect().toJSON())
  const centre = (row, col) =>
    page.evaluate(
      ([row, col]) => {
        const { interaction } = window.__ktableGrid
        interaction.scrollToCell({ row, col })
        const area = interaction.cellScreenArea({ row, col })
        return { x: area.x + area.width / 2, y: area.y + area.height / 2 }
      },
      [row, col],
    )

  // (g) 50 edits in a row.
  const first = await centre(0, columns.text)
  await page.mouse.click(box.left + first.x, box.top + first.y)
  await settled(page)
  const before = await page.evaluate(async () => JSON.parse(await window.__ktableGrid.engine.invoke('gridStats')))
  const started = Date.now()
  for (let edit = 0; edit < EDITS; edit += 1) {
    await page.keyboard.type(`Edit ${edit}`)
    await page.keyboard.press('Enter')
    await page.waitForFunction((count) => window.__ktableEditTimes.length >= count, edit + 1, { timeout: 60_000, polling: 10 })
  }
  const seconds = (Date.now() - started) / 1000
  const after = await page.evaluate(async () => JSON.parse(await window.__ktableGrid.engine.invoke('gridStats')))
  const times = await page.evaluate(() => window.__ktableEditTimes.slice())
  const written = await page.evaluate(
    async ([count, col]) => {
      const { interaction, data, view } = window.__ktableGrid
      const field = data.field(view.currentAxes.columns[col].fieldId)
      const ids = Array.from({ length: count }, (_, row) => interaction.rows.recordId(row))
      return (await data.records(ids, [field.name])).map((record) => record.values[field.name])
    },
    [EDITS, columns.text],
  )
  const g = {
    edits: times.length,
    p50: quantile(times, 0.5),
    p95: quantile(times, 0.95),
    max: Math.max(0, ...times),
    seconds,
    arranged: (after.arrange?.count ?? 0) - (before.arrange?.count ?? 0),
    relocated: (after.relocate?.count ?? 0) - (before.relocate?.count ?? 0),
  }
  results.g = g
  console.log(`g 50 edits: edit to redraw p50 ${f(g.p50)} p95 ${f(g.p95)} max ${f(g.max)} ms; ${f(g.seconds)} s in all; ${g.arranged} whole arrangements, ${g.relocated} relocations`)
  step('g every edit is written', written.every((value, index) => value === `Edit ${index}`), JSON.stringify(written.slice(0, 3)))
  step('g edit to redraw', g.p95 <= EDIT_P95_MS * k, `p95 ${f(g.p95)} ≤ ${f(EDIT_P95_MS * k)} ms`)

  // A whole number column cleared.
  await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
  await settled(page)
  const header = await centre(0, columns.number)
  await page.mouse.click(box.left + header.x, box.top + 16)
  await settled(page)
  const cleared = times.length
  await page.keyboard.press('Delete')
  await page.waitForFunction((count) => window.__ktableEditTimes.length > count, cleared, { timeout: 600_000, polling: 50 })
  const clearMs = (await page.evaluate(() => window.__ktableEditTimes.slice()))[cleared]
  const left = await page.evaluate(async (col) => {
    const { interaction, data, view } = window.__ktableGrid
    const field = data.field(view.currentAxes.columns[col].fieldId)
    const rows = interaction.rows
    const ids = [0, Math.floor(rows.count / 2), rows.count - 2].map((row) => rows.recordId(row)).filter((id) => id != null)
    return (await data.records(ids, [field.name])).map((record) => record.values[field.name] ?? null)
  }, columns.number)
  results.clear = { ms: clearMs, records: rows }
  console.log(`clear a column of ${rows} records: ${f(clearMs)} ms from Delete to the redraw`)
  step('a whole column is cleared', left.every((value) => value == null), JSON.stringify(left))
  step('page errors', errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
  await context.close()
} catch (error) {
  failures += 1
  console.log(`FAIL perf edits - ${error instanceof Error ? error.stack : error}`)
} finally {
  await browser.close()
  await server.close()
  await served.close()
}
const report = { browser: which, tier, relax: k, rows, results }
if (process.env.PERF_OUT) fs.writeFileSync(process.env.PERF_OUT, JSON.stringify(report, null, 1))
console.log(`${failures ? 'FAIL' : 'PASS'} perf edits (${tier} tier, ${which})`)
process.exit(failures ? 1 : 0)
