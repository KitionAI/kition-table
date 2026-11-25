// Export in a real browser, through the development host: node tests/export.mjs [chromium|webkit]
// The engine answers the table's file (`file.export`, base64) and the host writes it as a new
// file beside the document (`host/write` mode `export`): the editor says it exported only when the
// host answers, names the file the host wrote, and shows the host's error when it refuses. The
// agent's export command answers the same bytes. Build the engine first (pnpm engine).
import fs from 'node:fs'
import { chromium, webkit } from 'playwright'
import { createServer } from 'vite'

const which = process.argv[2] ?? 'chromium'

const engineDir = new URL('../public/engine/', import.meta.url)
const sdk = await import(new URL('kition_table_kit.js', engineDir).href)
await sdk.default({ module_or_path: fs.readFileSync(new URL('kition_table_kit_bg.wasm', engineDir)) })
/** A document whose first table is "People" (Ada, Countess; Grace) and, with `second`, a table "Places" (Paris). */
function fixture(name, second) {
  const doc = sdk.TableDocument.create(name, 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const table = call('file.get').tables[0]
  const primary = (of) => of.fields.find((field) => field.id === of.primary_field_id).name
  call('table.set', { tableId: table.id, title: 'People' })
  for (const title of ['Ada, Countess', 'Grace']) call('record.add', { tableId: table.id, values: { [primary(table)]: title } })
  let places = null
  if (second) {
    places = call('table.add', { title: 'Places' })
    call('record.add', { tableId: places.id, values: { [primary(places)]: 'Paris' } })
  }
  doc.markSaved()
  const bytes = Buffer.from(doc.save()).toString('base64')
  doc.free()
  return { bytes, places: places?.id ?? null }
}
const TEAM = fixture('Team.ktable', false)
const ATLAS = fixture('Atlas.ktable', true)

const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 0, host: '127.0.0.1' }, logLevel: 'error' })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await (which === 'webkit' ? webkit : chromium).launch()
const page = await (await browser.newContext({ viewport: { width: 1440, height: 900 } })).newPage()
const errors = []
let failures = 0
page.on('pageerror', (error) => errors.push(error.message))
const step = (name, ok, detail = '') => {
  if (!ok) failures += 1
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${detail}` : ''}`)
}
const until = async (check, tries = 150) => {
  for (let i = 0; i < tries; i += 1) {
    if (await check().catch(() => false)) return true
    await page.waitForTimeout(100)
  }
  return false
}
const kit = page.frameLocator('#kit')
const names = () => page.evaluate(() => [...window.devHost.files.keys()])
// A workspace file's bytes as UTF-8 text, or null while it is not there.
const text = (name) =>
  page.evaluate((file) => {
    const found = window.devHost.files.get(file)
    return found ? new TextDecoder().decode(found.bytes) : null
  }, name)
const toasts = (level) => kit.locator(`.kui-toast[data-level=${level}]`).allTextContents().then((all) => all.join(' | '))

async function open(name, bytes) {
  await page.evaluate(
    async ({ name, bytes }) => {
      const binary = atob(bytes)
      const data = new Uint8Array(binary.length)
      for (let index = 0; index < binary.length; index += 1) data[index] = binary.charCodeAt(index)
      await window.devHost.open(name, data.buffer)
    },
    { name, bytes },
  )
  await page.waitForFunction((name) => document.getElementById('status')?.textContent?.startsWith(`${name} open in `), name, { timeout: 60000 })
  await kit.locator('.data-inline-footer').filter({ hasText: /\d+ records/ }).waitFor({ timeout: 30000 })
}

// The file the next export adds to the workspace.
async function exported(menuItem, extension) {
  const before = new Set(await names())
  await kit.getByTestId('table-export').click()
  await kit.getByTestId(menuItem).click()
  let added = null
  await until(async () => {
    added = (await names()).find((name) => !before.has(name) && name.endsWith(`.${extension}`)) ?? null
    return added != null || (await toasts('error')) !== ''
  })
  return added
}

try {
  await page.goto(`${origin}/dev.html?locale=en-US`)
  await page.waitForFunction(() => document.getElementById('status')?.textContent?.includes(' open in '), null, { timeout: 60000 })
  await open('Team.ktable', TEAM.bytes)

  step('the bar offers the table\'s export', (await kit.getByTestId('table-export').count()) === 1)

  const csv = await exported('table-export-csv', 'csv')
  const csvText = csv ? await text(csv) : null
  step('Export as CSV writes Team.csv through the host', csv === 'Team.csv' && csvText?.includes('"Ada, Countess"') && csvText.includes('Grace'), `${csv}: ${csvText} ${await toasts('error')}`)
  await until(async () => (await toasts('success')).includes('Team.csv'))
  step('says it exported, naming the file the host wrote', (await toasts('success')).includes('Team.csv'), await toasts('success'))

  const md = await exported('table-export-md', 'md')
  const mdText = md ? await text(md) : null
  step('Export as Markdown writes Team.md through the host', md === 'Team.md' && mdText?.startsWith('|') && mdText.includes('Ada, Countess'), `${md}: ${mdText}`)

  // A second CSV: the host never replaces a file, and the editor names the one it chose.
  const again = await exported('table-export-csv', 'csv')
  await until(async () => (await toasts('success')).includes('Team 2.csv'))
  step('a second export is the new file the host names', again === 'Team 2.csv' && (await toasts('success')).includes('Team 2.csv'), `${again} ${await toasts('success')}`)

  // The host refuses the write: the editor shows its error and claims nothing.
  await page.evaluate(() => {
    window.devHost.files.set = () => {
      throw new Error('the workspace disk is full')
    }
  })
  const before = (await names()).length
  const successes = (await toasts('success')).split(' | ').filter((toast) => toast.startsWith('Exported')).length
  await kit.getByTestId('table-export').click()
  await kit.getByTestId('table-export-csv').click()
  await until(async () => (await toasts('error')).includes('the workspace disk is full'))
  step('the host\'s refusal shows as the export\'s error', (await toasts('error')).includes('the workspace disk is full'), await toasts('error'))
  step('and no file is written, nor success claimed', (await names()).length === before && (await toasts('success')).split(' | ').filter((toast) => toast.startsWith('Exported')).length <= successes)
  await page.evaluate(() => {
    delete window.devHost.files.set
  })

  // The agent's export (kit.json `agent.export`): the open editor runs the engine's command, which
  // answers the file for the host to write.
  const agent = await page.evaluate(async () => (await window.devHost.connection().ai.run([{ command: 'file.export', params: { format: 'csv' } }], false)).completed[0].result)
  const agentText = agent?.data ? Buffer.from(agent.data, 'base64').toString('utf8') : ''
  step('the agent\'s export command answers the CSV bytes', agent?.extension === 'csv' && agentText === csvText, JSON.stringify(agent).slice(0, 160))
  const dirty = await page.evaluate(() => window.devHost.changes.filter((state) => state.dirty).length)
  step('exporting does not change the table', dirty === 0, `${dirty} dirty changes`)

  // A document of several tables: the file names the table shown.
  await open('Atlas.ktable', ATLAS.bytes)
  await kit.getByTestId(`table-tab-${ATLAS.places}`).click()
  await kit.locator('.data-inline-footer').filter({ hasText: /1 record/ }).waitFor({ timeout: 30000 })
  const places = await exported('table-export-csv', 'csv')
  const placesText = places ? await text(places) : null
  step('one table of several is named in the new file', places === 'Atlas - Places.csv' && placesText?.includes('Paris') && !placesText.includes('Grace'), `${places}: ${placesText}`)

  step('no page errors', errors.length === 0, errors.join(' | '))
} catch (error) {
  failures += 1
  console.log(`FAIL ${error.stack ?? error}`)
} finally {
  await browser.close()
  await server.close()
}
console.log(failures ? `${failures} failed (${which})` : `all passed (${which})`)
process.exit(failures ? 1 : 0)
