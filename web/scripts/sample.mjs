#!/usr/bin/env node
// Writes public/sample/Sample.ktable for the development host (dev.html): a small task list made
// by the built engine (public/engine), so it is exactly a file the kit writes.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const engine = path.join(web, 'public', 'engine')
const sdk = await import(pathToFileURL(path.join(engine, 'kition_table_kit.js')).href)
await sdk.default({ module_or_path: fs.readFileSync(path.join(engine, 'kition_table_kit_bg.wasm')) })

const doc = sdk.TableDocument.create('Sample.ktable', 0, 0)
const call = (method, params = {}) => JSON.parse(doc.call(method, JSON.stringify(params))).result
const table = call('documents.get').tables[0]
const fields = Object.fromEntries(table.fields.map((field) => [field.title, field.name]))
for (const [title, status, notes] of [
  ['Write the brief', 'Done', 'Shared with the team'],
  ['Collect feedback', 'In progress', ''],
  ['Plan the launch', 'To do', 'After feedback'],
]) {
  call('records.create', { tableId: table.id, values: { [fields.Title]: title, [fields.Status]: status, [fields.Notes]: notes } })
}
fs.mkdirSync(path.join(web, 'public', 'sample'), { recursive: true })
fs.writeFileSync(path.join(web, 'public', 'sample', 'Sample.ktable'), (doc.prepareSave(), doc.save()))
doc.free()
console.log('wrote public/sample/Sample.ktable')
