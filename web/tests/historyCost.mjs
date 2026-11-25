// What a record's history before the session costs on a large table (feature
// table.records.history-kept): node tests/historyCost.mjs <file.ktable> [checkpoints]
//
// The built engine in Node (pnpm engine) opens the file, takes `checkpoints` (default 3) snapshots
// of it with one record's title edited between them, as a host's checkpoints keep them, reopens it
// with the last as its stored replica, and asks `records.history` for that record with the
// checkpoints as the editor sends them (base64 in the request's JSON). It prints what each part
// took and how large the snapshots are; it checks nothing. tests/perf.mjs runs it on the 100,000 x
// 50 fixture and reports it.
import fs from 'node:fs'
import path from 'node:path'
import { nodeEngine } from './gridPage.mjs'

const file = process.argv[2]
const count = Number(process.argv[3] ?? 3)
if (!file || !fs.existsSync(file)) {
  console.log(`usage: node tests/historyCost.mjs <file.ktable> [checkpoints]`)
  process.exit(2)
}
const { TableDocument } = await nodeEngine()
const call = (doc, method, params = {}) => JSON.parse(doc.call(method, JSON.stringify(params))).result
const ms = (from) => Math.round(performance.now() - from)
const base64 = (bytes) => Buffer.from(bytes).toString('base64')
const name = path.basename(file)

let at = performance.now()
const doc = TableDocument.open(name, fs.readFileSync(file))
console.log(`open ${ms(at)} ms`)
const table = call(doc, 'documents.get').tables[0]
const primary = table.fields.find((field) => field.id === table.primary_field_id).name
const record = call(doc, 'records.list', { tableId: table.id, limit: 1 }).items[0]
const points = []
for (let n = 0; n < count; n += 1) {
  if (n > 0) call(doc, 'records.update', { tableId: table.id, recordId: record.id, values: { [primary]: `History cost ${n}` } })
  at = performance.now()
  const snapshot = doc.snapshot()
  const taken = ms(at)
  points.push({ version: base64(doc.version()), time: 1_000 * (n + 1), author: 'user:cost', snapshot: base64(snapshot) })
  console.log(`checkpoint ${n + 1}: snapshot ${(snapshot.length / 1e6).toFixed(1)} MB in ${taken} ms, base64 ${(points[n].snapshot.length / 1e6).toFixed(1)} MB`)
}
const saved = doc.save()
const stored = doc.snapshot()
doc.free()
at = performance.now()
const reopened = TableDocument.openWithReplica(name, saved, stored, new Uint8Array())
console.log(`reopen with the stored replica ${ms(at)} ms`)
let params
try {
  at = performance.now()
  params = JSON.stringify({ tableId: table.id, recordId: record.id, checkpoints: points })
  console.log(`request JSON ${(params.length / 1e6).toFixed(1)} MB in ${ms(at)} ms`)
} catch (error) {
  console.log(`request JSON of ${count} checkpoints cannot be made: ${error instanceof Error ? error.message : error}`)
  process.exit(0)
}
at = performance.now()
const told = JSON.parse(reopened.call('records.history', params)).result
console.log(`records.history over ${count} checkpoints ${ms(at)} ms: ${told.items.length} entries`)
console.log(`memory: rss ${(process.memoryUsage().rss / 1e6).toFixed(0)} MB`)
