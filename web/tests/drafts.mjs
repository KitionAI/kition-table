// An agent's draft and a restored version in the table editor (Kit UI protocol sections 7d and
// 7m), through the development host with `?checkpoints=1` and the kit's sample table:
// - the person retitles a record by typing in the grid; an agent's draft adds a record; the
//   draft's comparison lists the record and the document does not change; the person retitles
//   the record again meanwhile; the person accepts the draft: the grid shows the draft's record
//   and the person's title;
// - one click of the editor's Undo takes back the draft's record only (the person's title stays);
//   Redo brings it back;
// - a checkpoint recorded at a save with a label, a later retitle, the checkpoint restored
//   through the host: the record's title is the checkpoint's; one Undo brings the later title back.
// node tests/drafts.mjs [chromium|webkit]
import assert from 'node:assert/strict'
import { chromium, webkit } from 'playwright'
import { createServer } from 'vite'

const which = process.argv[2] ?? 'chromium'
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 0, host: '127.0.0.1' }, logLevel: 'error' })
await server.listen()
const browser = await (which === 'webkit' ? webkit : chromium).launch()
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
// The grid draws its text with the canvas's fillText: what the editor shows is read from it.
await page.addInitScript(() => {
  const draws = []
  window.__textDraws = draws
  const fillText = CanvasRenderingContext2D.prototype.fillText
  CanvasRenderingContext2D.prototype.fillText = function (text, x, y, maxWidth) {
    if (draws.length > 20000) draws.splice(0, 10000)
    draws.push(String(text))
    return maxWidth === undefined ? fillText.call(this, text, x, y) : fillText.call(this, text, x, y, maxWidth)
  }
})
const kit = page.frameLocator('#kit')
const frame = () => page.frames().find((candidate) => candidate !== page.mainFrame() && candidate.url().includes('index.html'))
const errors = []
page.on('pageerror', (error) => errors.push(error.message))
const until = async (check, message) => {
  for (let n = 0; n < 200; n += 1) {
    if (await check().catch(() => false)) return
    await page.waitForTimeout(100)
  }
  assert.fail(message)
}
/** Runs one engine command as the agent, on the document or (`draft`) on the agent's draft; answers its result. */
const engine = async (command, params = {}, draft = false) => {
  const answer = await page.evaluate(
    async ({ command, params, draft }) => window.devHost.connection().ai.run([{ command, params }], false, draft ? { draft: true } : {}),
    { command, params, draft },
  )
  const result = answer.completed[0].result
  return (typeof result === 'string' ? JSON.parse(result) : result).result
}
const PATH = 'Sample.ktable'
let table
let title
/** The titles of the sample table's records, in order, in the document or (`draft`) in the agent's draft. */
const titles = async (draft = false) => (await engine('record.list', { tableId: table.id }, draft)).items.map((record) => record.values[title])
/** How many texts the grid drew so far. */
const drawn = () => frame().evaluate(() => window.__textDraws.length)
/** Whether the grid drew `text` since the count `from`. */
const drewSince = (from, text) => frame().evaluate(({ from, text }) => window.__textDraws.slice(from).includes(text), { from, text })
const stage = () => kit.locator('.data-inline-grid-canvas [data-ktable-grid-stage="true"]')
const cellEditor = () => kit.locator('.data-inline-grid-canvas textarea[data-ktable-grid-input]')
/** The person types `text` over the first record's title, which reads `from`, in the grid. */
const retitle = async (from, text) => {
  const mark = await drawn()
  await stage().click({ position: { x: 145, y: 48 } })
  await stage().dblclick({ position: { x: 145, y: 48 } })
  await until(() => cellEditor().evaluate((element) => element.style.pointerEvents === 'auto'), 'the cell editor shows')
  await until(async () => (await cellEditor().inputValue()) === from, `the cell editor holds ${from}`)
  await cellEditor().fill(text)
  await page.keyboard.press('Enter')
  await until(async () => (await titles())[0] === text, `the first record is titled ${text}`)
  await until(() => drewSince(mark, text), `the grid shows ${text}`)
}
/** Clicks the editor's Undo or Redo (`table-undo`, `table-redo`) as a person does. */
const press = async (name) => {
  await until(() => kit.getByTestId(`table-${name}`).isEnabled(), `${name} is enabled`)
  await kit.getByTestId(`table-${name}`).click()
}
let failures = 0

try {
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/dev.html?locale=en-US&checkpoints=1`)
  await page.waitForFunction((path) => document.getElementById('status')?.textContent?.includes(`${path} open in `), PATH, { timeout: 60000 })
  await kit.getByTestId('ktable-editor').waitFor({ timeout: 30000 })
  await until(async () => (await frame()?.evaluate(() => window.__textDraws.includes('Write the brief'))) === true, 'the grid draws the sample')
  ;[table] = (await engine('file.get')).tables
  title = table.fields.find((field) => field.is_primary).name
  const sample = await titles()
  assert.deepEqual(sample, ['Write the brief', 'Collect feedback', 'Plan the launch'])

  // a. The person retitles a record; the agent's draft adds one; the comparison lists it and the
  // document stays as it is; the person retitles again meanwhile; the person accepts the draft.
  await retitle('Write the brief', 'Brief by hand')
  await engine('record.add', { tableId: table.id, values: { [title]: 'From the draft' } }, true)
  assert.ok((await titles(true)).includes('From the draft'), "the draft holds the agent's record")
  const compared = await page.evaluate(() => window.devHost.connection().draft.compare())
  assert.ok(compared.summary.created.some((object) => object.type === 'Record'), `the comparison lists the draft's record: ${JSON.stringify(compared.summary)}`)
  assert.deepEqual(await titles(), ['Brief by hand', ...sample.slice(1)], 'the comparison changes nothing in the document')
  await retitle('Brief by hand', 'Brief edited twice')
  const mark = await drawn()
  const merged = await page.evaluate(() => window.devHost.connection().draft.merge({ kind: 'agent', principal: 'agent:test' }))
  assert.equal(merged.merged, true, 'the draft merged')
  await until(async () => (await titles()).includes('From the draft'), "the draft's record is in the document")
  await until(() => drewSince(mark, 'From the draft'), "the grid shows the draft's record")
  assert.equal((await titles())[0], 'Brief edited twice', "the person's edit made meanwhile stays")
  console.log(`PASS ${which}: a draft merged with the person's edit made meanwhile`)

  // b. One Undo takes back the draft's record only; Redo brings it back.
  await press('undo')
  await until(async () => !(await titles()).includes('From the draft'), "Undo takes the draft's record back")
  assert.deepEqual(await titles(), ['Brief edited twice', ...sample.slice(1)], "Undo leaves the person's title")
  await press('redo')
  await until(async () => (await titles()).includes('From the draft'), "Redo brings the draft's record back")
  assert.equal((await titles())[0], 'Brief edited twice')
  const merging = await titles()
  console.log(`PASS ${which}: one Undo takes back the draft only, Redo brings it back`)

  // d. A labelled checkpoint at a save; a later retitle; the checkpoint restored; one Undo.
  const saved = await page.evaluate(() => window.devHost.connection().save())
  assert.equal(saved.saved, true, 'saved')
  await until(() => page.evaluate((path) => window.devHost.history.replica(path) !== null, PATH), 'the replica is stored with the save')
  const checkpoint = await page.evaluate(() => window.devHost.recordCheckpoint({ origin: 'save', label: 'Before the review' }))
  assert.ok(checkpoint?.id, 'a checkpoint is recorded')
  assert.ok((await page.evaluate((path) => window.devHost.history.checkpoints(path), PATH)).some((listed) => listed.id === checkpoint.id && listed.label === 'Before the review'), 'the labelled checkpoint is listed')
  await retitle('Brief edited twice', 'After the checkpoint')
  const restoring = await drawn()
  await page.evaluate((id) => window.devHost.restoreCheckpoint(id), checkpoint.id)
  await until(async () => (await titles())[0] === 'Brief edited twice', "the restore gives back the checkpoint's title")
  await until(() => drewSince(restoring, 'Brief edited twice'), "the grid shows the checkpoint's title")
  assert.deepEqual(await titles(), merging, "the document is the checkpoint's")
  await press('undo')
  await until(async () => (await titles())[0] === 'After the checkpoint', 'one Undo takes the restore back')
  console.log(`PASS ${which}: a restored checkpoint is one step of the person's Undo`)
  assert.deepEqual(errors, [], 'no page errors')
} catch (error) {
  failures += 1
  console.log(`FAIL ${which}: ${error instanceof Error ? error.message.split('\n')[0] : String(error)}`)
} finally {
  if (errors.length) console.log(`page errors:\n${errors.slice(0, 8).join('\n')}`)
  await browser.close()
  await server.close()
}
process.exit(failures ? 1 : 0)
