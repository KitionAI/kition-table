// What the grid's browser tests share (tests/perf.mjs, tests/parity.mjs, tests/widths.mjs): the
// Vite dev server, a browser at a device pixel ratio, the development page `dev/grid.html` on a
// table file, and checks that the package holds what the page draws with (the built engine with
// the grid's extras, and the locked fonts). Files the tests make are served under /__grid/.
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium, webkit } from 'playwright'
import { createServer } from 'vite'
import { LOCK, PACKAGE_FONTS, verify } from '../scripts/fonts.mjs'

const web = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
export const REPO = path.dirname(web)
export const ENGINE = path.join(web, 'public', 'engine')

/** Fails unless public/engine is built with the grid's extras and public/fonts holds the locked faces. */
export function checkPackage() {
  const glue = path.join(ENGINE, 'kition_table_kit.js')
  if (!fs.existsSync(glue)) throw new Error('public/engine is not built (pnpm engine)')
  if (!fs.readFileSync(glue, 'utf8').includes('gridBands')) throw new Error('public/engine has no gridBands: rebuild it (pnpm engine)')
  const problems = verify()
  if (problems.length) throw new Error(`public/fonts does not match fonts.lock.json (node scripts/fonts.mjs): ${problems.join('; ')}`)
}

/**
 * The built engine in Node, as tests/flow.mjs loads it, with the package's faces registered (as
 * the page's worker registers them): for fixtures made with the engine's own requests.
 */
export async function nodeEngine() {
  const sdk = await import(new URL('kition_table_kit.js', `file://${ENGINE}/`).href)
  await sdk.default({ module_or_path: fs.readFileSync(path.join(ENGINE, 'kition_table_kit_bg.wasm')) })
  for (const file of Object.keys(LOCK.files).filter((name) => name.endsWith('.ttf'))) sdk.addFont(fs.readFileSync(path.join(PACKAGE_FONTS, file)))
  return sdk
}

/**
 * Serves one large file over loopback (a browser route cannot carry hundreds of megabytes);
 * answers its URL and a close function.
 */
export async function serveFile(file) {
  const server = http.createServer((request, response) => {
    response.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': fs.statSync(file).size, 'access-control-allow-origin': '*' })
    fs.createReadStream(file).pipe(response)
  })
  await new Promise((done) => server.listen(0, '127.0.0.1', done))
  return { url: `http://127.0.0.1:${server.address().port}/${path.basename(file)}`, close: () => new Promise((done) => server.close(done)) }
}

/** Starts the Vite dev server. */
export async function startServer() {
  const server = await createServer({ configFile: path.join(web, 'vite.config.ts'), server: { port: 0, host: '127.0.0.1' }, logLevel: 'error' })
  await server.listen()
  return { server, origin: `http://127.0.0.1:${server.httpServer.address().port}` }
}

export async function launch(which, headless = true) {
  return (which === 'webkit' ? webkit : chromium).launch({ headless })
}

/**
 * A page of `browser` at `dpr` and `viewport` showing `dev/grid.html` with `query`; `files` are
 * served under /__grid/<name> (bytes or a path). Answers once the page has its axes.
 */
export async function openGrid(browser, origin, { dpr = 2, viewport = { width: 1440, height: 900 }, files = {}, query = {} }) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: dpr })
  await context.route('**/__grid/**', async (route) => {
    const name = decodeURIComponent(new URL(route.request().url()).pathname.replace(/^\/__grid\//, ''))
    const file = files[name]
    if (!file) return route.fulfill({ status: 404, body: 'missing' })
    await route.fulfill({ status: 200, body: typeof file === 'string' ? fs.readFileSync(file) : Buffer.from(file), headers: { 'content-type': 'application/octet-stream' } })
  })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text())
  })
  const search = new URLSearchParams(query).toString()
  await page.goto(`${origin}/dev/grid.html${search ? `?${search}` : ''}`)
  await page.waitForFunction(() => document.body.dataset.ready === 'true' || document.body.dataset.error, null, { timeout: 120_000 })
  const failed = await page.evaluate(() => document.body.dataset.error)
  if (failed) throw new Error(`the grid page did not open: ${failed}`)
  return { context, page, errors }
}

/** Waits until every band the viewport shows is drawn from its list (no skeleton). */
export async function settled(page, timeout = 60_000) {
  await page.waitForFunction(() => window.__ktableGrid.view.complete(), null, { timeout, polling: 50 })
  // One more frame, so the canvas holds the complete picture.
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
}

export function quantile(values, q) {
  if (!values.length) return 0
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.round((sorted.length - 1) * q))]
}
