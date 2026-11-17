// Takes the README's screenshots of the Table editor (assets/readme/*.png at the repository root):
// the editor in the development host, on a sample launch tracker the built engine makes, at a fixed
// viewport. Not a test, and not part of any suite; re-run it by hand when the editor changes:
//
//   cd web && pnpm install && pnpm run engine      # the engine in public/
//   node scripts/sample.mjs                        # the file dev.html opens first
//   node scripts/readme-shots.mjs [name ...]       # every shot, or only the named ones
//
// Run it with no HTTP proxy set (the browser reaches a local server). Images are written as PNG at
// device scale 2; the README's were then shrunk with `pngquant --quality=85-98 --strip`.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { chromium } from 'playwright'
import { createServer } from 'vite'

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const out = path.resolve(web, '..', 'assets', 'readme')
fs.mkdirSync(out, { recursive: true })
const only = new Set(process.argv.slice(2))
const wanted = (name) => only.size === 0 || only.has(name)

const engineDir = path.join(web, 'public', 'engine')
const sdk = await import(pathToFileURL(path.join(engineDir, 'kition_table_kit.js')).href)
await sdk.default({ module_or_path: fs.readFileSync(path.join(engineDir, 'kition_table_kit_bg.wasm')) })

const NAME = 'Launch tracker.ktable'

/**
 * The sample every shot opens, made by the engine: a launch tracker with Tasks, Team and
 * Milestones, linked to each other, a board of the tasks by status, and a dashboard. Invented
 * people and work. Answers the file's bytes and the ids the shots open.
 */
function sample() {
  const doc = sdk.TableDocument.create(NAME, 0, 0)
  const call = (method, params = {}) => {
    const answer = JSON.parse(doc.call(method, JSON.stringify(params)))
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result
  }
  const starter = call('documents.get').tables[0]

  const tasks = call('tables.create', {
    title: 'Tasks',
    fields: [
      { name: 'task', title: 'Task', type: 'text', primary: true },
      { name: 'status', title: 'Status', type: 'single_select', options: { choices: ['Backlog', 'In progress', 'In review', 'Done'] } },
      { name: 'priority', title: 'Priority', type: 'single_select', options: { choices: ['High', 'Medium', 'Low'] } },
      { name: 'area', title: 'Area', type: 'single_select', options: { choices: ['Product', 'Design', 'Marketing', 'Support'] } },
      { name: 'due', title: 'Due', type: 'date' },
      { name: 'estimate', title: 'Estimate (h)', type: 'number' },
      { name: 'confidence', title: 'Confidence', type: 'rating', options: { max: 5, icon: 'star' } },
    ],
    views: [{ title: 'All tasks', type: 'grid' }, { title: 'Board', type: 'kanban' }],
  })
  const team = call('tables.create', {
    title: 'Team',
    fields: [
      { name: 'name', title: 'Name', type: 'text', primary: true },
      { name: 'role', title: 'Role', type: 'single_select', options: { choices: ['Product manager', 'Engineer', 'Designer', 'Marketing lead', 'Support lead'] } },
      { name: 'email', title: 'Email', type: 'text' },
    ],
    views: [{ title: 'Everyone', type: 'grid' }],
  })
  const milestones = call('tables.create', {
    title: 'Milestones',
    fields: [
      { name: 'milestone', title: 'Milestone', type: 'text', primary: true },
      { name: 'date', title: 'Date', type: 'date' },
    ],
    views: [{ title: 'Timeline', type: 'grid' }],
  })
  call('tables.delete', { tableId: starter.id })

  const owner = call('fields.create', { tableId: tasks.id, name: 'owner', title: 'Owner', type: 'link_to_record', options: { target_table_id: team.id } })
  const milestone = call('fields.create', { tableId: tasks.id, name: 'milestone', title: 'Milestone', type: 'link_to_record', options: { target_table_id: milestones.id } })
  const teamTasks = call('fields.create', { tableId: team.id, name: 'tasks', title: 'Tasks', type: 'link_to_record', options: { target_table_id: tasks.id, allow_multiple: true, symmetric_field_id: owner.id } })
  call('fields.create', { tableId: team.id, name: 'open_hours', title: 'Hours planned', type: 'rollup', options: { link_field_id: teamTasks.id, target_field_id: tasks.fields.find((field) => field.name === 'estimate').id, aggregation: 'sum' } })
  const milestoneTasks = call('fields.create', { tableId: milestones.id, name: 'tasks', title: 'Tasks', type: 'link_to_record', options: { target_table_id: tasks.id, allow_multiple: true, symmetric_field_id: milestone.id } })
  call('fields.create', { tableId: milestones.id, name: 'task_count', title: 'Task count', type: 'rollup', options: { link_field_id: milestoneTasks.id, aggregation: 'count' } })
  call('fields.create', { tableId: tasks.id, name: 'owner_role', title: 'Owner role', type: 'lookup', options: { link_field_id: owner.id, target_field_id: team.fields.find((field) => field.name === 'role').id } })

  // Column widths that fit each table in the shots' 1440-pixel view.
  const widths = (table, sizes) => {
    const { fields, views } = call('documents.get').tables.find((candidate) => candidate.id === table)
    for (const [name, width] of Object.entries(sizes)) {
      const field = fields.find((candidate) => candidate.name === name)
      for (const view of views) call('views.patchField', { tableId: table, viewId: view.id, fieldId: field.id, width })
    }
  }
  widths(tasks.id, { task: 225, status: 120, priority: 100, area: 110, due: 110, estimate: 110, confidence: 120, owner: 125, milestone: 170, owner_role: 135 })
  widths(team.id, { name: 180, role: 170, email: 220, tasks: 300, open_hours: 140 })
  widths(milestones.id, { milestone: 220, date: 140, tasks: 300, task_count: 130 })
  call('views.update', { tableId: tasks.id, viewId: tasks.views[0].id, title: 'All tasks' })
  // Extra-tall rows on Team, so each person's linked tasks show one per line.
  call('views.update', { tableId: team.id, viewId: team.views[0].id, config: { row_height: 'extra_tall' } })

  const people = {}
  for (const [name, role] of [
    ['Maya Chen', 'Product manager'],
    ['Sam Ortiz', 'Engineer'],
    ['Lena Park', 'Designer'],
    ['Theo Grant', 'Marketing lead'],
    ['Iris Novak', 'Support lead'],
    ['Omar Haddad', 'Engineer'],
  ]) {
    const email = `${name.split(' ')[0].toLowerCase()}@launch.example`
    people[name] = call('records.create', { tableId: team.id, values: { name, role, email } }).id
  }
  const stages = {}
  for (const [name, date] of [
    ['Beta', '2026-11-10'],
    ['Preview', '2026-11-24'],
    ['General availability', '2026-12-08'],
  ]) {
    stages[name] = call('records.create', { tableId: milestones.id, values: { milestone: name, date } }).id
  }
  for (const [task, status, priority, area, due, estimate, confidence, who, stage] of [
    ['Freeze the feature list', 'Done', 'High', 'Product', '2026-11-03', 4, 5, 'Maya Chen', 'Beta'],
    ['Invite the beta testers', 'Done', 'Medium', 'Marketing', '2026-11-06', 6, 5, 'Theo Grant', 'Beta'],
    ['Fix the import edge cases', 'In review', 'High', 'Product', '2026-11-14', 16, 4, 'Sam Ortiz', 'Beta'],
    ['Design the onboarding tour', 'In progress', 'High', 'Design', '2026-11-18', 12, 3, 'Lena Park', 'Preview'],
    ['Write the help articles', 'In progress', 'Medium', 'Support', '2026-11-20', 10, 4, 'Iris Novak', 'Preview'],
    ['Record the demo video', 'Backlog', 'Medium', 'Marketing', '2026-11-21', 8, 3, 'Theo Grant', 'Preview'],
    ['Load test the sync service', 'In progress', 'High', 'Product', '2026-11-22', 14, 2, 'Omar Haddad', 'Preview'],
    ['Pricing page copy', 'In review', 'Medium', 'Marketing', '2026-11-25', 5, 4, 'Theo Grant', 'General availability'],
    ['Brief the support team', 'Backlog', 'Medium', 'Support', '2026-12-01', 3, 3, 'Iris Novak', 'General availability'],
    ['Final accessibility pass', 'Backlog', 'High', 'Design', '2026-12-02', 9, 2, 'Lena Park', 'General availability'],
    ['Launch announcement', 'Backlog', 'High', 'Marketing', '2026-12-08', 4, 3, 'Maya Chen', 'General availability'],
    ['Release notes', 'Backlog', 'Low', 'Product', '2026-12-08', 3, 4, 'Sam Ortiz', 'General availability'],
  ]) {
    call('records.create', {
      tableId: tasks.id,
      values: { task, status, priority, area, due, estimate, confidence, owner: [people[who]], milestone: [stages[stage]] },
    })
  }

  const dashboard = 'launch-overview'
  call('dashboards.upsert', {
    dashboardId: dashboard,
    title: 'Launch overview',
    source_table_id: tasks.id,
    layout: [
      { widget_id: 'total', x: 0, y: 0, w: 3, h: 2, min_w: 2, min_h: 2 },
      { widget_id: 'done', x: 3, y: 0, w: 3, h: 2, min_w: 2, min_h: 2 },
      { widget_id: 'review', x: 6, y: 0, w: 3, h: 2, min_w: 2, min_h: 2 },
      { widget_id: 'high', x: 9, y: 0, w: 3, h: 2, min_w: 2, min_h: 2 },
      { widget_id: 'by-status', x: 0, y: 2, w: 4, h: 4, min_w: 3, min_h: 3 },
      { widget_id: 'by-area', x: 4, y: 2, w: 4, h: 4, min_w: 3, min_h: 3 },
      { widget_id: 'open-by-priority', x: 8, y: 2, w: 4, h: 4, min_w: 3, min_h: 3 },
    ],
    // Dashboards count records (`count`, or `count_true` of a checkbox).
    widgets: [
      { id: 'total', title: 'Tasks', description: 'Everything on the launch plan', type: 'metric', query: { aggregation: 'count' }, config: { value_label: 'tasks', color: '#5645d4' } },
      { id: 'done', title: 'Done', description: 'Finished and shipped', type: 'metric', query: { aggregation: 'count', filters: [{ field_name: 'status', operator: 'equals', value: 'Done' }] }, config: { value_label: 'tasks', color: '#1aae39' } },
      { id: 'review', title: 'In review', description: 'Waiting for a second look', type: 'metric', query: { aggregation: 'count', filters: [{ field_name: 'status', operator: 'equals', value: 'In review' }] }, config: { value_label: 'tasks', color: '#2a9d99' } },
      { id: 'high', title: 'High priority open', description: 'Not done yet', type: 'metric', query: { aggregation: 'count', filters: [{ field_name: 'priority', operator: 'equals', value: 'High' }, { field_name: 'status', operator: 'not_equals', value: 'Done' }] }, config: { value_label: 'tasks', color: '#dd5b00' } },
      { id: 'by-status', title: 'Tasks by status', description: 'Where the work stands', type: 'pie', query: { aggregation: 'count', group_by_field_name: 'status' }, config: { category_order: ['Backlog', 'In progress', 'In review', 'Done'] } },
      { id: 'by-area', title: 'Tasks by area', description: 'Who the launch needs most', type: 'bar', query: { aggregation: 'count', group_by_field_name: 'area' }, config: { color: '#5645d4' } },
      { id: 'open-by-priority', title: 'Open tasks by priority', description: 'What is left before launch day', type: 'bar', query: { aggregation: 'count', group_by_field_name: 'priority', filters: [{ field_name: 'status', operator: 'not_equals', value: 'Done' }] }, config: { color: '#2a9d99', category_order: ['High', 'Medium', 'Low'] } },
    ],
  })

  doc.markSaved()
  const bytes = Buffer.from(doc.save()).toString('base64')
  doc.free()
  return { bytes, tasks: tasks.id, team: team.id, milestones: milestones.id, dashboard }
}
const SAMPLE = sample()

const server = await createServer({ configFile: path.join(web, 'vite.config.ts'), server: { port: 0, host: '127.0.0.1' }, logLevel: 'error' })
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chromium.launch()

/** A page with the sample open in the development host at `view`; screenshots take only the kit's frame. */
async function open(view, height = 862) {
  // The development host's header bar above the kit's frame is 38 pixels high.
  const context = await browser.newContext({ viewport: { width: 1440, height: height + 38 }, deviceScaleFactor: 2, timezoneId: 'UTC' })
  const page = await context.newPage()
  await page.goto(`${origin}/dev.html?locale=en-US`)
  await page.waitForFunction(() => document.getElementById('status')?.textContent?.includes(' open in '), null, { timeout: 90000 })
  await page.evaluate(async ({ name, bytes, view }) => {
    const binary = atob(bytes)
    const data = new Uint8Array(binary.length)
    for (let index = 0; index < binary.length; index += 1) data[index] = binary.charCodeAt(index)
    await window.devHost.open(name, data.buffer, { view })
  }, { name: NAME, bytes: SAMPLE.bytes, view })
  await page.waitForFunction((name) => document.getElementById('status')?.textContent?.startsWith(`${name} open in `), NAME, { timeout: 90000 })
  const kit = page.frameLocator('#kit')
  await page.waitForTimeout(2500)
  const shot = async (name) => {
    await page.waitForTimeout(800)
    await page.locator('#kit').screenshot({ path: path.join(out, `${name}.png`) })
    console.log(`wrote assets/readme/${name}.png`)
  }
  return { context, page, kit, shot }
}

try {
  if (wanted('hero')) {
    // The tasks in the grid: selects, dates, numbers, a rating, links to Team and Milestones, a lookup.
    const { context, kit, shot } = await open({ path: `tables/${SAMPLE.tasks}` }, 900)
    await kit.getByTestId('ktable-editor').waitFor({ timeout: 60000 })
    await shot('hero')
    await context.close()
  }

  if (wanted('grid')) {
    // The Team table: each person's linked tasks, and a rollup of their hours.
    const { context, kit, shot } = await open({ path: `tables/${SAMPLE.team}` })
    await kit.getByTestId('ktable-editor').waitFor({ timeout: 60000 })
    await shot('grid')
    await context.close()
  }

  if (wanted('kanban')) {
    // The tasks on a board, one column per status.
    const { context, kit, shot } = await open({ path: `tables/${SAMPLE.tasks}` })
    await kit.getByTestId('ktable-editor').waitFor({ timeout: 60000 })
    await kit.getByRole('button', { name: 'Board' }).first().click()
    await kit.locator('.data-inline-kanban-card').first().waitFor({ timeout: 30000 })
    await shot('kanban')
    await context.close()
  }

  if (wanted('dashboard')) {
    // The launch dashboard: four counts and three charts over the tasks.
    const { context, kit, shot } = await open({ path: `dashboards/${SAMPLE.dashboard}` })
    await kit.getByTestId('dashboard-editor-pane').waitFor({ timeout: 60000 })
    await shot('dashboard')
    await context.close()
  }

  if (wanted('templates')) {
    // The template library, opened from the add-table button.
    const { context, kit, shot } = await open({ path: `tables/${SAMPLE.tasks}` })
    await kit.getByTestId('ktable-editor').waitFor({ timeout: 60000 })
    await kit.getByTestId('table-add').click()
    await kit.getByTestId('table-template-library-dialog').waitFor({ timeout: 30000 })
    await shot('templates')
    await context.close()
  }
} finally {
  await browser.close()
  await server.close()
}
