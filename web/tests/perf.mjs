// The grid's read-only scrolling against the lines of the grid plan (section 4.2), on the
// development page: node tests/perf.mjs [chromium]
//
// The fixture is 100,000 records in 50 fields (bindings/wasm/examples/perf_fixture.rs, written to
// target/perf/ when missing). Scenarios: (a) a steady 6,000 px/s scroll for 5 s, (b) a 20,000 px/s
// fling, (c) the scroll bar dragged from top to bottom in 1 s, (d) across the 50 columns, (e) (a) in
// a grouped and sorted view, (f) (a) with 88 px rows. Edits (g) belong to the interactive grid.
// The jumps (b, c) run five times each and are checked on the median of the runs.
//
// PERF_TIER=ci (default) checks every line relaxed three times, the headless tier that catches
// regressions; PERF_TIER=strict checks the lines themselves, on the owner's machine with a headed
// browser (PERF_HEADED=1). PERF_ROWS changes the record count; PERF_OUT names a JSON report.
// WebKit's numbers are measured in Safari and the desktop app, not here (Playwright's WebKit has
// no JIT on the owner's machine).
//
// On a GitHub-hosted runner (RUNNER_ENVIRONMENT=github-hosted, which GitHub sets in every job of
// its own runners; a self-hosted runner has self-hosted) the lines that depend on the runner's
// speed (frame intervals, content play, skeleton frames and skeleton runs, band generation) are
// measured and printed against their lines but do not fail the run: a shared runner's speed varies
// from run to run. The lines that do not depend on its speed (the axes' size, the bytes
// transferred, the steady scrolls' prefetch plan, page errors) still fail. PERF_SPEED_LINES=fail
// makes the speed lines fail wherever it runs, PERF_SPEED_LINES=report makes them informational
// wherever it runs. Self-hosted runners and the owner's machine check every line; the strict tier
// runs on the owner's machine before a release (docs/development.md).
//
// A steady scroll's skeleton frames come from two things: the prefetch logic (which bands the view
// plans and asks for ahead of the scroll) and how fast the worker answers. The logic is checked
// apart, and fails everywhere: every band that came into view and was not here yet had been in the
// view's prefetch plan in an earlier frame (unless the view moved farther in that frame than the
// plan reached), and at least one band came into view that had been asked for (or was here) before
// it was visible. The skeleton frames themselves are a speed line.
import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { REPO, checkPackage, launch, openGrid, quantile, serveFile, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
const tier = process.env.PERF_TIER ?? 'ci'
/** Whether the speed lines only report (see above): PERF_SPEED_LINES, else on a GitHub-hosted runner. */
const speedLines = process.env.PERF_SPEED_LINES ?? (process.env.RUNNER_ENVIRONMENT === 'github-hosted' ? 'report' : 'fail')
if (speedLines !== 'fail' && speedLines !== 'report') {
  console.log(`FAIL perf - PERF_SPEED_LINES is fail or report, not ${JSON.stringify(speedLines)}`)
  process.exit(1)
}
const hosted = speedLines === 'report'
const why = process.env.PERF_SPEED_LINES ? `PERF_SPEED_LINES=${speedLines}` : `RUNNER_ENVIRONMENT=${process.env.RUNNER_ENVIRONMENT}`
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
/** Speed lines over their line on a hosted runner (reported, not failed). */
let informational = 0
const results = {}
/** A line: `speed` when it depends on the runner's speed (informational on a GitHub-hosted runner). */
const step = (name, ok, detail, speed = false) => {
  if (!ok && speed && hosted) {
    informational += 1
    console.log(`INFO ${name} - ${detail} (over its line; a speed line, informational here: ${why})`)
    return
  }
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name} - ${detail}`)
}
if (hosted)
  console.log(
    `INFO speed lines report (${why}): frame intervals, content play, skeleton frames and runs, and band generation are reported against their lines and do not fail; axes size, transfer, the prefetch plan and page errors still fail. Self-hosted runners and the owner's machine check every line; the strict tier runs on the owner's machine before a release.`,
  )

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
      stats.bands.clear()
      stats.planned.clear()
      stats.requested.clear()
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
      const arrivals = [...stats.bands.values()].map((band) => ({ ...band }))
      const after = await bands()
      const made = after.ms.slice(Math.max(0, after.ms.length - (after.count - before)))
      const sorted = [...made].sort((x, y) => x - y)
      const worker = { count: made.length, p95: sorted.length ? sorted[Math.round((sorted.length - 1) * 0.95)] : 0 }
      return { intervals, frames, requests, arrivals, seconds, worker }
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
  // The bands that came into view after the first frame, and what the prefetch had done for them.
  const came = run.arrivals.filter((band) => !band.first)
  const missing = came.filter((band) => !band.held)
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
    prefetch: {
      came: came.length,
      ahead: came.filter((band) => band.held || band.requested).length,
      // Not here, and not in the plan of an earlier frame though the view moved less than it reached: the logic's miss.
      unplanned: missing.filter((band) => !band.planned && band.moved <= band.reach).length,
      // Not here, not planned, after a frame that moved past the plan's reach (the frame's speed, not the logic).
      outran: missing.filter((band) => !band.planned && band.moved > band.reach).length,
    },
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

/** How many times each jump (b, c) runs. */
const JUMP_RUNS = 5

/**
 * A jump (b, c) measured JUMP_RUNS times from the top of the table, settled first (where the person
 * starts it), and the median of each line's value over the runs: a jump's skeleton run is a few
 * round trips to the worker, and one slow round trip on a shared runner does not decide the line.
 */
async function jumps(name, page, script, args) {
  const runs = []
  for (let index = 1; index <= JUMP_RUNS; index += 1) {
    await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
    await settled(page)
    runs.push(summarize(`${name} (run ${index} of ${JUMP_RUNS})`, await measure(page, script, args)))
  }
  const median = (pick) => quantile(runs.map(pick), 0.5)
  const summary = { runs, longestSkeleton: median((run) => run.longestSkeleton), interval: { max: median((run) => run.interval.max) } }
  results[name] = summary
  console.log(`${name}: median of ${JUMP_RUNS} runs: longest skeleton run ${summary.longestSkeleton} frames, max interval ${summary.interval.max.toFixed(1)} ms`)
  return summary
}

/** The lines of section 4.2 for a steady scroll (a, e, f), relaxed by `k`. */
function steadyLines(name, s) {
  step(`${name} frame interval`, s.interval.p95 <= LINES.interval.p95 * k && s.interval.p99 <= LINES.interval.p99 * k && s.interval.max <= LINES.interval.max * k, `p95 ${s.interval.p95.toFixed(1)} ≤ ${at(LINES.interval.p95)}, p99 ${s.interval.p99.toFixed(1)} ≤ ${at(LINES.interval.p99)}, max ${s.interval.max.toFixed(1)} ≤ ${at(LINES.interval.max)}`, true)
  step(`${name} content play`, s.play.p95 <= LINES.playP95 * k, `p95 ${s.play.p95.toFixed(2)} ≤ ${at(LINES.playP95)} ms`, true)
  // The prefetch logic, whatever the machine's speed: every band that came into view had been planned
  // before (bar the frames that outran the plan), and the view asked for bands before they showed.
  const p = s.prefetch
  step(`${name} prefetch plan`, p.came > 0 && p.unplanned === 0, `${p.unplanned} of ${p.came} bands came into view unplanned (${p.outran} after a frame that moved past the plan's reach)`)
  step(`${name} prefetch ahead`, p.ahead > 0, `${p.ahead} of ${p.came} bands asked for or here before they came into view`)
  step(`${name} no skeleton frames`, s.skeletonFrames <= LINES.skeletonSteady * k, `${s.skeletonFrames} ≤ ${at(LINES.skeletonSteady)} skeleton frames`, true)
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
    step('band generation', bench.warm.p95 <= LINES.bandWarmP95 * k && bench.cold.p95 <= LINES.bandColdP95 * k, `warm p95 ${bench.warm.p95.toFixed(2)} ≤ ${at(LINES.bandWarmP95)}, cold p95 ${bench.cold.p95.toFixed(2)} ≤ ${at(LINES.bandColdP95)} ms`, true)

    const b = await jumps('b fling 20000 px/s', page, steady, { speed: 20000, seconds: 1.5, axis: 'y' })
    step('b skeleton runs', b.longestSkeleton <= LINES.skeletonRun * k, `median longest ${b.longestSkeleton} ≤ ${at(LINES.skeletonRun)} frames`, true)
    step('b frame interval', b.interval.max <= LINES.interval.max * k, `median max ${b.interval.max.toFixed(1)} ≤ ${at(LINES.interval.max)} ms`, true)

    const c = await jumps('c scroll bar top to bottom', page, drag, { seconds: 1 })
    step('c skeleton runs', c.longestSkeleton <= LINES.skeletonRun * k, `median longest ${c.longestSkeleton} ≤ ${at(LINES.skeletonRun)} frames`, true)
    step('c frame interval', c.interval.max <= LINES.interval.max * k, `median max ${c.interval.max.toFixed(1)} ≤ ${at(LINES.interval.max)} ms`, true)

    await page.evaluate(() => window.__ktableGrid.view.scrollTo(0, 0))
    await settled(page)
    const d = summarize('d across 50 columns', await measure(page, steady, { speed: 3000, seconds: 4, axis: 'x' }))
    step('d frame interval', d.interval.p95 <= LINES.interval.p95 * k && d.interval.max <= LINES.interval.max * k, `p95 ${d.interval.p95.toFixed(1)}, max ${d.interval.max.toFixed(1)} ms`, true)
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
// A record's history before the session on the same fixture (tests/historyCost.mjs): measured
// and printed, never failed: it shows what opening a record's history panel costs at this size.
{
  const cost = spawnSync(process.execPath, [path.join(REPO, 'web', 'tests', 'historyCost.mjs'), fixture, '3'], { encoding: 'utf8', timeout: 600_000 })
  for (const line of `${cost.stdout ?? ''}`.trim().split('\n').filter(Boolean)) console.log(`INFO record history - ${line}`)
  if (cost.status !== 0) console.log(`INFO record history - stopped (${cost.signal ?? `exit ${cost.status}`}): ${`${cost.stderr ?? ''}`.trim().split('\n').filter((line) => line.trim()).slice(-8).join(' | ')}`)
}
const report = { browser: which, version, tier, relax: k, rows, speedLines, informational, results }
if (process.env.PERF_OUT) fs.writeFileSync(process.env.PERF_OUT, JSON.stringify(report, null, 1))
const runner = hosted ? `, ${why}: ${informational} speed line${informational === 1 ? '' : 's'} over, informational` : ''
console.log(`${failures ? 'FAIL' : 'PASS'} perf (${tier} tier, ${which} ${report.version}${runner})`)
process.exit(failures ? 1 : 0)
