// Co-editing in the editor: it declares the replica, a viewer's session refuses every write the
// table makes and lets every read through, and changes another replica of the document makes on
// the same line of changes reach the table editor through the data source. Over the built engine
// (public/engine: `pnpm engine` first).
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { editsDocument } from '@kition/sdk/editor'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { CAPABILITIES } from './capabilities'
import { READS, createWorkerTableDataSource, type TableEngineAccess } from './dataSource'

type Doc = {
  execute(command: string, params: string): string
  commands(): string
  takeImported(): string
  version(): Uint8Array
  exportChanges(since: Uint8Array): Uint8Array
  importChanges(changes: Uint8Array): string
  blobsSince(since: Uint8Array): Uint8Array
  addBlobs(blobs: Uint8Array): void
  blobs(): Uint8Array
  snapshot(): Uint8Array
  save(): Uint8Array
  readonly revision: bigint | number
  free(): void
}
type DocClass = {
  create(name: string, width: number, height: number): Doc
  open(name: string, bytes: Uint8Array): Doc
  openWithReplica(name: string, bytes: Uint8Array, snapshot: Uint8Array, blobs: Uint8Array): Doc
}

const engine = path.resolve(__dirname, '..', 'public', 'engine')
let TableDocument: DocClass

beforeAll(async () => {
  const sdk = await import(/* @vite-ignore */ pathToFileURL(path.join(engine, 'kition_table_kit.js')).href)
  await sdk.default({ module_or_path: fs.readFileSync(path.join(engine, 'kition_table_kit_bg.wasm')) })
  TableDocument = sdk.TableDocument
})

/** The engine as the session gives it to the data source. */
function access(doc: Doc): TableEngineAccess {
  return {
    run: async (command, params) => doc.execute(command, JSON.stringify(params)),
    invoke: async <T,>(name: string) => (doc as unknown as Record<string, () => T>)[name]!.call(doc),
    revision: () => Number(doc.revision),
  }
}

/** What the sync relay does: the blobs the changes name, then the changes. */
function relay(from: Doc, to: Doc) {
  const since = to.version()
  to.addBlobs(from.blobsSince(since))
  to.importChanges(from.exportChanges(since))
}

describe('co-editing a table', () => {
  it('declares the replica, which a co-editing host needs to relay the document', () => {
    expect(CAPABILITIES).toContain('replica')
    expect(CAPABILITIES).toContain('ai')
  })

  it("refuses every write in a viewer's session and lets the editor's reads through", () => {
    const doc = TableDocument.create('Tasks.ktable', 0, 0)
    const commands = JSON.parse(doc.commands()) as Array<{ id: string; mutates?: boolean }>
    expect(commands.length).toBeGreaterThan(40)
    for (const command of commands) {
      const method = command.id.replace(/^table\./, '')
      // The read-only rules every editor runs under refuse exactly what the engine says edits.
      expect([command.id, editsDocument(command)]).toEqual([command.id, command.mutates === true])
      if (READS.has(method)) expect([command.id, editsDocument(command)]).toEqual([command.id, false])
    }
    const byId = new Map(commands.map((command) => [command.id, command]))
    // The edit a viewer tries in Kition One's case, and the ones the grid makes.
    for (const id of ['record.add', 'record.set', 'record.delete', 'field.add', 'table.delete', 'edit.undo']) {
      expect([id, editsDocument(byId.get(id)!)]).toEqual([id, true])
    }
    for (const method of READS) expect([method, byId.has(`table.${method}`)]).toEqual([method, true])
    doc.free()
  })

  it("hears another replica's records once they are imported, and not its own", async () => {
    const bytes = TableDocument.create('Tasks.ktable', 0, 0).save()
    const ann = TableDocument.open('Tasks.ktable', bytes)
    // Bob's replica follows Ann's line of changes (the KDM takes changes in order).
    const bob = TableDocument.openWithReplica('Tasks.ktable', new Uint8Array(), ann.snapshot(), ann.blobs())
    const source = createWorkerTableDataSource(access(bob), 'Tasks.ktable')
    const heard: unknown[] = []
    source.onChange(1, (event) => heard.push(event))
    source.observe(Number(bob.revision))

    // Ann's edit arrives.
    ann.execute('record.add', JSON.stringify({ tableId: 1, values: { title: 'Ann wrote this' } }))
    relay(ann, bob)
    source.observe(Number(bob.revision))
    await vi.waitFor(() => expect(heard).toHaveLength(1))
    const event = heard[0] as { type: string; change: { documentPath: string; origin: string; recordsUpserted: unknown[] } }
    expect(event.type).toBe('changed')
    expect(event.change.documentPath).toBe('Tasks.ktable')
    expect(event.change.origin).toBe('import:sync')
    expect(event.change.recordsUpserted).toHaveLength(1)

    // Bob's own edit through the editor: nothing more to hear.
    await source.records.create({ documentId: 1, tableId: 1, values: { title: 'Bob wrote this' } } as never)
    source.observe(Number(bob.revision))
    expect(heard).toHaveLength(1)

    // And Bob's edit reaches Ann: both show both records.
    relay(bob, ann)
    const titles = async (doc: Doc) => {
      const page = JSON.parse(doc.execute('record.list', JSON.stringify({ tableId: 1 }))).result as { items: Array<{ values: { title: string } }> }
      return page.items.map((record) => record.values.title).sort()
    }
    expect(await titles(ann)).toEqual(['Ann wrote this', 'Bob wrote this'])
    expect(await titles(bob)).toEqual(await titles(ann))
    ann.free()
    bob.free()
  })

  it('reads the table again when the document was opened anew (the room arrived after the file)', () => {
    const doc = TableDocument.create('Tasks.ktable', 0, 0)
    const source = createWorkerTableDataSource(access(doc), 'Tasks.ktable')
    const heard: Array<{ type: string }> = []
    source.onChange(1, (event) => heard.push(event as { type: string }))
    source.observe(3)
    source.reopened(0)
    expect(heard).toEqual([{ type: 'resync' }])
    doc.free()
  })
})
