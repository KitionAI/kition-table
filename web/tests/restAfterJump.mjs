// A view that jumps to the end of a table rests with its exact bands: node tests/restAfterJump.mjs [chromium|webkit]
//
// The scroll bar is dragged down in slow, steady frames, more than a screen a frame, until it
// reaches the end of the table. While it jumps, the view asks for drafts where it will be; at
// the end every such draft is here, so a request in the last moving frames has nothing to fetch.
// Once the view rests it must ask for the exact bands by itself: no answer or scroll comes to
// make it draw again. Exit code 0 when every step passes.
import { checkPackage, launch, nodeEngine, openGrid, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
/** The frames of the drag (ms): slow frames, as on a busy machine. */
const BEAT_MS = 100
/** How long a resting view may take to show its exact bands (ms). */
const REST_MS = 3000

checkPackage()

/** A table of `count` records in Latin text, made with the engine's own requests; answers its bytes. */
async function fixture(count) {
  const sdk = await nodeEngine()
  const doc = sdk.TableDocument.create('Rest.ktable', 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('documents.get').tables[0]
  const primary = table.fields.find((field) => field.is_primary)
  const records = Array.from({ length: count }, (_, i) => ({ [primary.name]: `Record ${i + 1}` }))
  call('transact', { ops: [{ op: 'records.create', tableId: table.id, at: null, records }] })
  doc.markSaved()
  const bytes = doc.save()
  doc.free()
  return bytes
}

let failures = 0
function step(name, ok, detail = '') {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}

const { server, origin } = await startServer()
const browser = await launch(which)
try {
  const files = { 'Rest.ktable': await fixture(3000) }
  const { context, page, errors } = await openGrid(browser, origin, { dpr: 1, files, query: { file: '/__grid/Rest.ktable' } })
  await page.waitForFunction(() => window.__ktableGrid.view.complete(), null, { timeout: 60_000, polling: 50 })
  const drag = await page.evaluate(async (beat) => {
    const { view } = window.__ktableGrid
    const scroller = view.scroller
    const screen = view.viewport.height
    const max = scroller.scrollHeight - scroller.clientHeight
    // Even steps of a screen and a half, the last one landing on the end of the table.
    const count = 6
    const stride = 1.5 * screen
    for (let frame = count; frame >= 0; frame -= 1) {
      scroller.scrollTop = max - frame * stride
      await new Promise((done) => setTimeout(done, beat))
    }
    return { frames: count + 1, max: Math.round(max) }
  }, BEAT_MS)
  const rested = await page
    .waitForFunction(() => window.__ktableGrid.view.complete(), null, { timeout: REST_MS, polling: 50 })
    .then(() => true)
    .catch(() => false)
  const state = await page.evaluate(() => {
    const { view } = window.__ktableGrid
    return { fetching: view.fetching, y: Math.round(view.viewport.scrollY), complete: view.complete() }
  })
  step(
    'a view that jumped to the end of the table shows its exact bands once it rests',
    rested,
    `${drag.frames} frames ${BEAT_MS} ms apart to y ${drag.max}; after ${REST_MS} ms at y ${state.y}: ${state.complete ? 'complete' : 'drafts or skeleton left'}, ${state.fetching ? 'fetching' : 'not fetching'}`,
  )
  step('page errors', errors.length === 0, errors.slice(0, 3).join(' | ') || 'none')
  await context.close()
} catch (error) {
  failures += 1
  console.log(`FAIL rest after a jump - ${error instanceof Error ? error.stack : error}`)
} finally {
  await browser.close()
  await server.close()
}
console.log(`${failures ? 'FAIL' : 'PASS'} rest after a jump (${which})`)
process.exit(failures ? 1 : 0)
