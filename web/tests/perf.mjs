// The grid's read-only scrolling against the lines of the grid plan (section 4.2), on the
// development page: node tests/perf.mjs [chromium]
//
// The fixture is 100,000 records in 50 fields (bindings/wasm/examples/perf_fixture.rs, written to
// target/perf/ when missing). Scenarios: (a) a steady 6,000 px/s scroll for 5 s, (b) a 20,000 px/s
// fling, (c) the scroll bar dragged from top to bottom in 1 s, (d) across the 50 columns, (e) (a) in
// a grouped and sorted view, (f) (a) with 88 px rows. Edits (g) belong to the interactive grid.
//
// PERF_TIER=ci (default) checks every line relaxed three times, the headless tier that catches
// regressions; PERF_TIER=strict checks the lines themselves, on the owner's machine with a headed
// browser (PERF_HEADED=1). PERF_ROWS changes the record count; PERF_OUT names a JSON report.
// WebKit's numbers are measured in Safari and the desktop app, not here (Playwright's WebKit has
// no JIT on the owner's machine).
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { REPO, checkPackage, launch, openGrid, quantile, serveFile, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
const tier = process.env.PERF_TIER ?? 'ci'
// The lines: the expected result of case table.editor.grid/scroll-perf (only a maintainer changes it).
const LINES = JSON.parse(fs.readFileSync(path.join(REPO, 'cases', 'table.editor.grid', 'scroll-perf', 'expected', 'lines.json'), 'utf8'))
const k = tier === 'strict' ? 1 : LINES.relax
/** A line times the tier's factor, as printed. */
const at = (line) => +(line * k).toFixed(1)
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
const version = browser.version()
let failures = 0
const results = {}
const step = (name, ok, detail) => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name} - ${detail}`)
}

/** Opens the fixture at `view`; answers the page, how long the first content took and the axes' size. */
async function open(view) {
  const { context, page, errors } = await openGrid(browser, origin, { dpr: 2, query: { file: served.url, view: String(view) } })
  await settled(page, 120_000)
  return { context, page, errors }
}

/**
 * Drives the scroller with `script` (run in the page, given the scroller and a done callback),
 * and answers what happened meanwhile: rAF intervals, content frames, band requests.
 */
async function measure(page, script, args) {
  return page.evaluate(
    async ({ source, args }) => {
      const { view, engine } = window.__ktableGrid
      const stats = window.__ktableGridStats
      // The worker's band times (gridStats' bands): the ones made from here on.
      const bands = async () => JSON.parse(await engine.invoke('gridStats')).bands
      const before = (await bands()).count
      stats.frames.length = 0
      stats.requests.length = 0
      const intervals = []
      let last = 0
      let running = true
      const tick = (at) => {
        if (last) intervals.push(at - last)
        last = at
        if (running) requestAnimationFrame(tick)
      }
      requestAnimationFrame(tick)
      const started = performance.now()
      await new Function('scroller', 'args', `return (${source})(scroller, args)`)(view.scroller, args)
      const seconds = (performance.now() - started) / 1000
      running = false
      const frames = stats.frames.map((frame) => ({ ...frame }))
      const requests = stats.requests.map((request) => ({ ...request }))
      const after = await bands()
      const made = after.ms.slice(Math.max(0, after.ms.length - (after.count - before)))
      const sorted = [...made].sort((x, y) => x - y)
      const worker = { count: made.length, p95: sorted.length ? sorted[Math.round((sorted.length - 1) * 0.95)] : 0 }
      return { intervals, frames, requests, seconds, worker }
    },
    { source: script.toString(), args },
  )
}

/** A steady scroll at `speed` px/s for `seconds` along `axis`, started from the current position. */
const steady = (scroller, { speed, seconds, axis }) =>
  new Promise((done) => {
    const key = axis === 'x' ? 'scrollLeft' : 'scrollTop'
    const from = scroller[key]
    const max = axis === 'x' ? scroller.scrollWidth - scroller.clientWidth : scroller.scrollHeight - scroller.clientHeight
    let start = 0
    const frame = (at) => {
      if (!start) start = at
      const t = (at - start) / 1000
      scroller[key] = Math.min(max, from + speed * t)
      if (t < seconds && scroller[key] < max) requestAnimationFrame(frame)
      else done()
    }
    requestAnimationFrame(frame)
  })

/** The scroll bar dragged from the top to the bottom in `seconds`. */
const drag = (scroller, { seconds }) =>
  new Promise((done) => {
    scroller.scrollTop = 0
    const max = scroller.scrollHeight - scroller.clientHeight
    let start = 0
    const frame = (at) => {
      if (!start) start = at
      const t = Math.min(1, (at - start) / 1000 / seconds)
      scroller.scrollTop = max * t
      if (t < 1) requestAnimationFrame(frame)
      else setTimeout(done, 300)
    }
    requestAnimationFrame(frame)
  })

/** The longest run of consecutive skeleton frames. */
function longestSkeleton(frames) {
  let run = 0
  let longest = 0
  for (const frame of frames) {
    run = frame.skeleton ? run + 1 : 0
    longest = Math.max(longest, run)
  }
  return longest
}

function summarize(name, run) {
  const gaps = run.intervals
  const plays = run.frames.map((frame) => frame.playMs)
  const bytes = run.requests.reduce((sum, request) => sum + request.bytes, 0)
  const summary = {
    frames: gaps.length,
    interval: { p50: quantile(gaps, 0.5), p95: quantile(gaps, 0.95), p99: quantile(gaps, 0.99), max: Math.max(0, ...gaps) },
    play: { p50: quantile(plays, 0.5), p95: quantile(plays, 0.95), max: Math.max(0, ...plays) },
    skeletonFrames: run.frames.filter((frame) => frame.skeleton).length,
    // Frames that drew a draft band (no skeleton): content in the skeleton lines (the owner, 2026-11-18), counted apart.
    draftFrames: run.frames.filter((frame) => frame.draft && !frame.skeleton).length,
    longestSkeleton: longestSkeleton(run.frames),
    frameBytesMax: Math.max(0, ...run.frames.map((frame) => frame.bytes)),
    bytesPerSecond: bytes / Math.max(0.001, run.seconds),
    requests: run.requests.length,
    roundTrip: { p50: quantile(run.requests.map((request) => request.ms), 0.5), p95: quantile(run.requests.map((request) => request.ms), 0.95) },
    worker: run.worker,
  }
  results[name] = summary
  const f = (v) => v.toFixed(1)
  console.log(
    `${name}: interval p50 ${f(summary.interval.p50)} p95 ${f(summary.interval.p95)} p99 ${f(summary.interval.p99)} max ${f(summary.interval.max)} ms; play p95 ${f(summary.play.p95)} ms; ` +
      `skeleton ${summary.skeletonFrames}/${run.frames.length} (longest ${summary.longestSkeleton}), draft ${summary.draftFrames}; frame bytes max ${summary.frameBytesMax}; ${f(summary.bytesPerSecond / 1024)} KB/s; ` +
      `${summary.requests} requests, rtt p95 ${f(summary.roundTrip.p95)} ms; band ${f(run.worker.p95)} ms p95 (${run.worker.count} made)`,
  )
  return summary
}

/** The lines of section 4.2 for a steady scroll (a, e, f), relaxed by `k`. */
function steadyLines(name, s) {
  step(`${name} frame interval`, s.interval.p95 <= LINES.interval.p95 * k && s.interval.p99 <= LINES.interval.p99 * k && s.interval.max <= LINES.interval.max * k, `p95 ${s.interval.p95.toFixed(1)} ≤ ${at(LINES.interval.p95)}, p99 ${s.interval.p99.toFixed(1)} ≤ ${at(LINES.interval.p99)}, max ${s.interval.max.toFixed(1)} ≤ ${at(LINES.interval.max)}`)
  step(`${name} content play`, s.play.p95 <= LINES.playP95 * k, `p95 ${s.play.p95.toFixed(2)} ≤ ${at(LINES.playP95)} ms`)
  step(`${name} no skeleton frames`, s.skeletonFrames <= LINES.skeletonSteady * k, `${s.skeletonFrames} ≤ ${at(LINES.skeletonSteady)} skeleton frames`)
  step(`${name} transfer`, s.frameBytesMax <= LINES.frameBytes * k && s.bytesPerSecond <= LINES.bytesPerSecond * k, `frame max ${(s.frameBytesMax / 1024).toFixed(0)} ≤ ${(LINES.frameBytes * k) / 1024} KB, ${(s.bytesPerSecond / 1024).toFixed(0)} ≤ ${(LINES.bytesPerSecond * k) / 1024} KB/s`)
}

try {
  // (a), (b), (c), (d) and the open, in the first view.
  {
    const { context, page, errors } = await open(made.views.all)
    // Opening is not a scrolling line: it is measured and shown, from navigation to the first
    // frame with every visible band (the file's transfer and the engine's open included).
    const first = await page.evaluate(() => window.__ktableGridStats.firstContent)
    results.open = { firstContentMs: first }
    console.log(`INFO open to first content - ${Math.round(first)} ms (line ${LINES.openMs} ms on the owner's machine)`)
    const axisBytes = JSON.parse(await page.evaluate(() => window.__ktableGrid.engine.invoke('gridStats'))).axisBytes ?? 0
    step('axes size', axisBytes > 0 && axisBytes <= LINES.axisBytes * k, `${(axisBytes / 1024).toFixed(0)} ≤ ${(LINES.axisBytes * k) / 1024} KB`)

    const a = summarize('a steady 6000 px/s', await measure(page, steady, { speed: 6000, seconds: 5, axis: 'y' }))
    steadyLines('a', a)

    // The worker's band times: bands never made before (cold: their records and much of their
    // text are new), then the same bands again (warm).
    await settled(page)
    const bench = await page.evaluate(async () => {
      const { view, engine } = window.__ktableGrid
      // The view's own prefetch done, so the worker makes only these bands.
      for (let quiet = 0; quiet < 10; ) {
        await new Promise((done) => setTimeout(done, 30))
        quiet = view.fetching ? 0 : quiet + 1
      }
      const axes = view.currentAxes
      const keys = []
      for (let i = 0; i < 24; i += 1) keys.push([0, 0, Math.floor(axes.rowBands / 4) + i * Math.floor(axes.rowBands / 40)])
      const p95 = (values) => [...values].sort((x, y) => x - y)[Math.round((values.length - 1) * 0.95)]
      const run = async () => {
        const before = JSON.parse(await engine.invoke('gridStats')).bands.count
        // Session 0 is not the page's (its own is random): the page's resources are left as they are.
        for (const key of keys) await engine.invoke('gridBands', axes.epoch, new Uint32Array(key), 2, 0, false)
        const after = JSON.parse(await engine.invoke('gridStats')).bands
        const made = after.ms.slice(after.ms.length - (after.count - before))
        return { count: made.length, p95: p95(made) }
      }
      const cold = await run()
      const warm = await run()
      return { cold, warm }
    })
    results.bands = bench
    step('band generation', bench.warm.p95 <= LINES.bandWarmP95 * k && bench.cold.p95 <= LINES.bandColdP95 * k, `warm p95 ${bench.warm.p95.toFixed(2)} ≤ ${at(LINES.bandWarmP95)}, cold p95 ${bench.cold.p95.toFixed(2)} ≤ ${at(LINES.bandColdP95)} ms`)

    await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
    await settled(page)
    const b = summarize('b fling 20000 px/s', await measure(page, steady, { speed: 20000, seconds: 1.5, axis: 'y' }))
    step('b skeleton runs', b.longestSkeleton <= LINES.skeletonRun * k, `longest ${b.longestSkeleton} ≤ ${at(LINES.skeletonRun)} frames`)
    step('b frame interval', b.interval.max <= LINES.interval.max * k, `max ${b.interval.max.toFixed(1)} ≤ ${at(LINES.interval.max)} ms`)

    const c = summarize('c scroll bar top to bottom', await measure(page, drag, { seconds: 1 }))
    step('c skeleton runs', c.longestSkeleton <= LINES.skeletonRun * k, `longest ${c.longestSkeleton} ≤ ${at(LINES.skeletonRun)} frames`)
    step('c frame interval', c.interval.max <= LINES.interval.max * k, `max ${c.interval.max.toFixed(1)} ≤ ${at(LINES.interval.max)} ms`)

    await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
    await settled(page)
    const d = summarize('d across 50 columns', await measure(page, steady, { speed: 3000, seconds: 4, axis: 'x' }))
    step('d frame interval', d.interval.p95 <= LINES.interval.p95 * k && d.interval.max <= LINES.interval.max * k, `p95 ${d.interval.p95.toFixed(1)}, max ${d.interval.max.toFixed(1)} ms`)
    step('page errors', errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
    await context.close()
  }
  for (const [name, view] of [
    ['e grouped and sorted', made.views.grouped],
    ['f 88 px rows', made.views.tall],
  ]) {
    const { context, page, errors } = await open(view)
    const s = summarize(name, await measure(page, steady, { speed: 6000, seconds: 5, axis: 'y' }))
    steadyLines(name[0], s)
    step(`${name[0]} page errors`, errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
    await context.close()
  }
} catch (error) {
  failures += 1
  console.log(`FAIL perf - ${error instanceof Error ? error.stack : error}`)
} finally {
  await browser.close()
  await server.close()
  await served.close()
}
const report = { browser: which, version, tier, relax: k, rows, results }
if (process.env.PERF_OUT) fs.writeFileSync(process.env.PERF_OUT, JSON.stringify(report, null, 1))
console.log(`${failures ? 'FAIL' : 'PASS'} perf (${tier} tier, ${which} ${report.version})`)
process.exit(failures ? 1 : 0)
