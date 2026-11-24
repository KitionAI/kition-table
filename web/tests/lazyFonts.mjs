// The engine's lazy fonts on the development page (grid plan D3; @kition/sdk/editor's `lazyFonts`, Kition
// platform E9 and E9a): node tests/lazyFonts.mjs [chromium|webkit]
//
// The engine gets Inter before the table opens and Droid Sans Fallback, Nanum Gothic and the emoji
// face only once its text wants them. A table in Latin only never loads them. With them held back,
// a table in Chinese, Korean and emoji shows its first content frame all the same (their runs are
// tofu in the engine then, which the page does not draw); once they come, the bands are fetched
// again and draw those runs in their faces, with no tofu left. Exit code 0 when every step passes.
import { checkPackage, launch, nodeEngine, settled, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
const LAZY = /\/fonts\/(droid-sans-fallback|nanum-gothic|noto-color-emoji)\//

checkPackage()

/** A table whose primary field holds `titles`, made with the engine's own requests; answers its bytes. */
async function fixture(titles) {
  const sdk = await nodeEngine()
  const doc = sdk.TableDocument.create('Fonts.ktable', 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('documents.get').tables[0]
  const primary = table.fields.find((field) => field.is_primary)
  const records = Array.from({ length: 40 }, (_, i) => ({ [primary.name]: titles[i % titles.length] }))
  call('transact', { ops: [{ op: 'records.create', tableId: table.id, at: null, records }] })
  doc.markSaved()
  const bytes = (doc.prepareSave(), doc.save())
  doc.free()
  return bytes
}

/**
 * The development page on `bytes`; the lazy font files are held back until `release` is called
 * when `hold` is set. Answers once the page has its axes.
 */
async function open(browser, origin, bytes, hold) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 })
  const held = []
  let released = false
  await context.route((url) => url.pathname.startsWith('/__grid/'), (route) => route.fulfill({ status: 200, body: Buffer.from(bytes), headers: { 'content-type': 'application/octet-stream' } }))
  const asked = []
  await context.route(LAZY, async (route) => {
    asked.push(new URL(route.request().url()).pathname)
    if (!hold || released) return route.continue()
    held.push(route)
  })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text())
  })
  await page.goto(`${origin}/dev/grid.html?file=/__grid/Fonts.ktable`)
  await page.waitForFunction(() => document.body.dataset.ready === 'true' || document.body.dataset.error, null, { timeout: 120_000 })
  const failed = await page.evaluate(() => document.body.dataset.error)
  if (failed) throw new Error(`the grid page did not open: ${failed}`)
  const release = async () => {
    released = true
    for (const route of held.splice(0)) await route.continue()
  }
  return { context, page, errors, asked, held, release }
}

/** What the bands the viewport shows hold: how many text runs per face, emoji and tofu boxes. */
function shown() {
  const { view } = window.__ktableGrid
  const counts = { sans: 0, fallback: 0, hangul: 0, emoji: 0, tofu: 0 }
  for (const key of view.neededKeys()) {
    for (const op of view.cache.peek(key)?.ops ?? []) {
      if (op.op === 0x05) counts[['sans', 'fallback', 'hangul', 'mono'][op.face]] += 1
      else if (op.op === 0x07) counts.emoji += 1
      else if (op.op === 0x06) counts.tofu += 1
    }
  }
  return counts
}

let failures = 0
function step(name, ok, detail = '') {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}

const { server, origin } = await startServer()
const browser = await launch(which)
try {
  {
    const { context, page, errors, asked } = await open(browser, origin, await fixture(['Quarterly planning', 'Launch review', 'Office move']), false)
    await settled(page)
    // A few more requests' worth of time, in which a wrong engine would ask.
    await page.evaluate(() => new Promise((done) => setTimeout(done, 500)))
    const pending = await page.evaluate(() => window.__ktableGrid.engine.fontsPending)
    step('a table in Latin only never loads the lazy fonts', asked.length === 0 && pending === 3, `asked ${asked.join(', ') || 'none'}, ${pending} left`)
    step('Latin only: page errors', errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
    await context.close()
  }
  {
    const titles = ['季度计划 Q3 客户反馈', '서울 지사 회의록', 'Launch day \u{1F680} \u{2705}', 'Mixed 中文 English 한국어 text']
    const { context, page, errors, asked, held, release } = await open(browser, origin, await fixture(titles), true)
    await page.waitForFunction(() => window.__ktableGridStats.firstContent !== null, null, { timeout: 30_000, polling: 50 })
    const before = await page.evaluate(() => ({ pending: window.__ktableGrid.engine.fontsPending, complete: window.__ktableGrid.view.complete() }))
    const first = await page.evaluate(shown)
    // The engine's request for the first lazy font reaches the route a moment after the band that wanted it.
    for (let waited = 0; held.length === 0 && waited < 5000; waited += 50) await new Promise((done) => setTimeout(done, 50))
    step(
      'the first content frame does not wait for the lazy fonts',
      before.pending === 3 && held.length >= 1 && first.sans > 0,
      `${held.length} held, ${before.pending} left, ${first.sans} Inter runs and ${first.tofu} tofu boxes not drawn yet`,
    )
    step('the engine asked for them once its text wanted them', asked.length >= 1 && /droid-sans-fallback/.test(asked[0]), asked[0] ?? 'none')
    await release()
    await page.waitForFunction(() => window.__ktableGrid.engine.fontsPending === 0, null, { timeout: 60_000, polling: 50 })
    await settled(page)
    const after = await page.evaluate(shown)
    step(
      'Chinese, Korean and emoji draw in their faces once the fonts are in',
      after.fallback > 0 && after.hangul > 0 && after.emoji > 0 && after.tofu === 0,
      `${after.fallback} CJK runs, ${after.hangul} Hangul runs, ${after.emoji} emoji, ${after.tofu} tofu`,
    )
    step('lazy fonts: page errors', errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
    await context.close()
  }
} catch (error) {
  failures += 1
  console.log(`FAIL lazy fonts - ${error instanceof Error ? error.stack : error}`)
} finally {
  await browser.close()
  await server.close()
}
console.log(`${failures ? 'FAIL' : 'PASS'} lazy fonts (${which})`)
process.exit(failures ? 1 : 0)
