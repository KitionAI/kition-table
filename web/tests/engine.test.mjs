// The built engine (public/engine) from Node, through the JavaScript interface the editor's worker
// uses: requests answer as Kition's runtime routes do, saves reopen, and two copies of one file
// exchange changes. Build the engine first (pnpm engine).
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'

const engine = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'public', 'engine')
const sdk = await import(pathToFileURL(path.join(engine, 'kition_table_kit.js')).href)
await sdk.default({ module_or_path: fs.readFileSync(path.join(engine, 'kition_table_kit_bg.wasm')) })
const { TableDocument } = sdk

const call = (doc, method, params = {}) => JSON.parse(doc.call(method, JSON.stringify(params)))
const firstTable = (doc) => call(doc, 'documents.get').result.tables[0]
const titles = (doc) => {
  const table = firstTable(doc)
  const primary = table.fields.find((field) => field.id === table.primary_field_id).name
  return call(doc, 'records.list', { tableId: table.id }).result.items.map((record) => record.values[primary])
}
const add = (doc, title) => {
  const table = firstTable(doc)
  const primary = table.fields.find((field) => field.id === table.primary_field_id).name
  return call(doc, 'records.create', { tableId: table.id, values: { [primary]: title } })
}

test('a new table has the starter table, and a write answers its change', () => {
  const doc = TableDocument.create('Tasks.ktable', 0, 0)
  assert.equal(firstTable(doc).fields.length, 3)
  assert.equal(doc.dirty, false)
  const answer = add(doc, 'Ada')
  assert.equal(answer.change.origin, 'person:kit')
  assert.equal(answer.change.recordsUpserted.length, 1)
  assert.equal(doc.dirty, true)
  assert.equal(call(doc, 'historyState').result.canUndo, true)
  doc.free()
})

test('saved bytes reopen with the edits, the same replica, and pass as a kit module document', () => {
  const doc = TableDocument.create('Tasks.ktable', 0, 0)
  add(doc, 'Ada')
  const bytes = doc.save()
  assert.equal(doc.dirty, true)
  doc.markSaved()
  assert.equal(doc.dirty, false)
  const reopened = TableDocument.open('Tasks.ktable', bytes)
  assert.deepEqual(titles(reopened), ['Ada'])
  assert.equal(reopened.docId, doc.docId)
  assert.equal(reopened.hasReplica, true)
  assert.equal(TableDocument.contractVersion(), 1)
  assert.throws(() => TableDocument.open('Tasks.ktable', new Uint8Array([1, 2, 3])))
  doc.free()
  reopened.free()
})

test('opening a file and the reads the editor makes on load leave it saved', () => {
  const doc = TableDocument.open('Sample.ktable', fs.readFileSync(path.join(engine, '..', 'sample', 'Sample.ktable')))
  assert.equal(doc.dirty, false)
  const revision = doc.revision
  const opened = call(doc, 'documents.get').result
  call(doc, 'historyState')
  call(doc, 'dashboards.list')
  for (const table of opened.tables) {
    call(doc, 'records.list', { tableId: table.id, limit: 200, offset: 0 })
    for (const view of table.views) {
      call(doc, 'views.listFields', { tableId: table.id, viewId: view.id })
      call(doc, 'records.list', { tableId: table.id, viewId: view.id })
      // What the editor sends back for a view it only opened: the config as it read it.
      call(doc, 'views.update', { tableId: table.id, viewId: view.id, config: view.config })
    }
  }
  assert.equal(doc.revision, revision)
  assert.equal(doc.dirty, false)
  assert.equal(call(doc, 'historyState').result.canUndo, false)
  doc.free()
})

test('an edit makes the document unsaved, and undo back to the saved state saved again', () => {
  const doc = TableDocument.open('Tasks.ktable', TableDocument.create('Tasks.ktable', 0, 0).save())
  add(doc, 'Ada')
  assert.equal(doc.dirty, true)
  call(doc, 'undo')
  assert.equal(doc.dirty, false)
  call(doc, 'redo')
  assert.equal(doc.dirty, true)
  doc.save()
  doc.markSaved()
  call(doc, 'undo')
  assert.equal(doc.dirty, true)
  call(doc, 'redo')
  assert.equal(doc.dirty, false)
  doc.free()
})

test('two copies of a file exchange their changes and converge', () => {
  const bytes = TableDocument.create('Tasks.ktable', 0, 0).save()
  const left = TableDocument.open('Tasks.ktable', bytes)
  const right = TableDocument.open('Tasks.ktable', bytes)
  add(left, 'Left')
  add(right, 'Right')
  const damage = JSON.parse(right.importChanges(left.exportChanges(right.version())))
  assert.equal(damage.kind, 'full')
  left.importChanges(right.exportChanges(left.version()))
  assert.deepEqual(titles(left).sort(), ['Left', 'Right'])
  assert.deepEqual(titles(right).sort(), ['Left', 'Right'])
  const imported = JSON.parse(right.takeImported())
  assert.equal(imported.length, 1)
  assert.equal(imported[0].origin, 'import:sync')
  assert.deepEqual(JSON.parse(right.takeImported()), [])
  left.free()
  right.free()
})

test('attachments are put into the file, read back, and synced to a copy beside their change', () => {
  const doc = TableDocument.create('Photos.ktable', 0, 0)
  add(doc, 'Beach')
  const table = firstTable(doc)
  const files = call(doc, 'fields.create', { tableId: table.id, title: 'Files', type: 'attachment' }).result.name
  const bytes = doc.save()
  const left = TableDocument.open('Photos.ktable', bytes)
  const right = TableDocument.open('Photos.ktable', bytes)
  doc.free()

  const picture = new Uint8Array(300).map((_, index) => (index * 7) % 256)
  picture.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
  const attachment = call(left, 'attachments.put', { tableId: table.id, name: 'beach.png', mimeType: 'image/png', data: Buffer.from(picture).toString('base64') }).result
  assert.match(attachment.url, /^kition-blob:[0-9a-f]{64}$/)
  const hash = attachment.url.slice('kition-blob:'.length)
  assert.equal(attachment.sha256, hash)
  const record = call(left, 'records.list', { tableId: table.id }).result.items[0]
  call(left, 'records.update', { tableId: table.id, recordId: record.id, values: { [files]: [attachment] } })
  const read = call(left, 'attachments.read', { hash }).result
  assert.deepEqual(new Uint8Array(Buffer.from(read.data, 'base64')), picture)
  assert.equal(read.mimeType, 'image/png')

  // What a relay does: the blobs the changes name go first, then the changes.
  const since = right.version()
  const blobs = left.blobsSince(since)
  assert.ok(blobs.length > picture.length)
  assert.deepEqual(left.blobs(), blobs)
  right.addBlobs(blobs)
  assert.equal(JSON.parse(right.importChanges(left.exportChanges(since))).kind, 'full')
  assert.deepEqual(call(right, 'records.list', { tableId: table.id }).result.items[0].values[files], [attachment])
  assert.deepEqual(new Uint8Array(Buffer.from(call(right, 'attachments.read', { hash }).result.data, 'base64')), picture)
  // Saved, the copy keeps the bytes.
  const reopened = TableDocument.open('Photos.ktable', right.save())
  assert.equal(call(reopened, 'attachments.read', { hash }).result.sizeBytes, picture.length)
  left.free()
  right.free()
  reopened.free()
})

test('a large table opens, pages, and edits within budget', () => {
  const doc = TableDocument.create('Large.ktable', 0, 0)
  const table = firstTable(doc)
  const primary = table.fields.find((field) => field.id === table.primary_field_id).name
  for (let chunk = 0; chunk < 10; chunk += 1) {
    const records = Array.from({ length: 1000 }, (_, index) => ({ [primary]: `Item ${chunk * 1000 + index}` }))
    call(doc, 'transact', { ops: [{ op: 'records.create', tableId: table.id, at: null, records }] })
  }
  const bytes = doc.save()
  doc.free()
  let started = performance.now()
  const opened = TableDocument.open('Large.ktable', bytes)
  const openMs = performance.now() - started
  started = performance.now()
  const page = call(opened, 'records.list', { tableId: table.id, limit: 200, offset: 5000 }).result
  const pageMs = performance.now() - started
  assert.equal(page.total, 10000)
  assert.equal(page.items.length, 200)
  started = performance.now()
  call(opened, 'transact', { ops: [{ op: 'cells.set', tableId: table.id, records: [{ rowKey: page.items[0].row_key, values: { [primary]: 'Edited' } }] }] })
  const editMs = performance.now() - started
  // Undo, redo, and a co-editor's change cost what they change, not the table's size.
  started = performance.now()
  call(opened, 'undo')
  const undoMs = performance.now() - started
  started = performance.now()
  call(opened, 'redo')
  const redoMs = performance.now() - started
  started = performance.now()
  const saved = opened.save()
  const saveMs = performance.now() - started
  const peer = TableDocument.open('Large.ktable', saved)
  const since = opened.version()
  call(peer, 'transact', { ops: [{ op: 'cells.set', tableId: table.id, records: [{ rowKey: page.items[1].row_key, values: { [primary]: 'From the peer' } }] }] })
  started = performance.now()
  opened.importChanges(peer.exportChanges(since))
  const importMs = performance.now() - started
  assert.equal(call(opened, 'records.list', { tableId: table.id, limit: 1, offset: 5001 }).result.items[0].values[primary], 'From the peer')
  console.log(
    `10,000 records: file ${(bytes.length / 1e6).toFixed(1)} MB, open ${openMs.toFixed(0)} ms, page of 200 ${pageMs.toFixed(0)} ms, edit ${editMs.toFixed(0)} ms, ` +
      `undo ${undoMs.toFixed(0)} ms, redo ${redoMs.toFixed(0)} ms, import ${importMs.toFixed(0)} ms, save ${saveMs.toFixed(0)} ms (${(saved.length / 1e6).toFixed(1)} MB)`,
  )
  assert.ok(openMs < 5000 && editMs < 5000 && undoMs < 5000 && importMs < 5000)
  opened.free()
  peer.free()
})

test('a step run as the agent is the agent’s: the agent’s undo and redo take it back and put it back', () => {
  // The editor's worker runs an agent's step through executeAs (KitModel M12a): without it the
  // step lands in the person's history and `edit.undo` with `scope: agent` finds nothing.
  const doc = TableDocument.create('Tasks.ktable', 0, 0)
  const agent = JSON.stringify(['agent', 'agent:kition', null])
  const table = firstTable(doc)
  const primary = table.fields.find((field) => field.id === table.primary_field_id).name
  assert.equal(typeof doc.executeAs, 'function')
  const edit = JSON.parse(doc.executeAs('table.records.create', JSON.stringify({ tableId: table.id, values: { [primary]: 'From the agent' } }), agent))
  assert.equal(edit.change.origin, 'agent:agent:kition')
  assert.deepEqual(titles(doc), ['From the agent'])
  const undone = JSON.parse(doc.executeAs('edit.undo', JSON.stringify({ scope: 'agent' }), agent))
  assert.equal(undone.result.done, true)
  assert.deepEqual(titles(doc), [])
  const redone = JSON.parse(doc.executeAs('edit.redo', JSON.stringify({ scope: 'agent' }), agent))
  assert.equal(redone.result.done, true)
  assert.deepEqual(titles(doc), ['From the agent'])
  doc.free()
})

test('table.file.export answers the table as CSV or Markdown bytes, for the host to write, and changes nothing', () => {
  const doc = TableDocument.create('Team.ktable', 0, 0)
  add(doc, 'Ada, Countess')
  doc.markSaved()
  const table = firstTable(doc)
  const revision = doc.revision
  const csv = JSON.parse(doc.execute('table.file.export', JSON.stringify({ format: 'csv', tableId: table.id })))
  assert.deepEqual([csv.extension, csv.mime], ['csv', 'text/csv'])
  const text = Buffer.from(csv.data, 'base64').toString('utf8')
  assert.equal(text, call(doc, 'csv.export', { tableId: table.id }).result.content)
  assert.match(text, /"Ada, Countess"/)
  const md = JSON.parse(doc.execute('table.file.export', JSON.stringify({ format: 'md' })))
  assert.deepEqual([md.extension, md.mime, md.tableId], ['md', 'text/markdown', table.id])
  assert.ok(Buffer.from(md.data, 'base64').toString('utf8').startsWith('|'))
  assert.throws(() => doc.execute('table.file.export', JSON.stringify({ format: 'pdf' })))
  assert.equal(doc.revision, revision)
  assert.equal(doc.dirty, false)
  doc.free()
})
