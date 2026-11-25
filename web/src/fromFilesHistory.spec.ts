// A table from files over the built engine (public/engine: `pnpm engine` first), with the host's
// model scripted: a failure partway leaves the document exactly as it was (no table, no records);
// making the table is one undo step, so Undo after a run takes all of it back; Undo (and Redo)
// after a cancel bring nothing back; AI fields that leave cells empty are reported.
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { engineAIRun } from './aiFields'
import { createWorkerTableDataSource, type EngineTableDataSource, type TableEngineAccess } from './dataSource'
import { tableFromFiles } from './fromFiles'
import type { GenerateParams, GenerateResult, HostServices } from './hostRequests'

type Doc = { execute(command: string, params: string): string; save(): Uint8Array; readonly revision: bigint | number; free(): void }
type DocClass = { create(name: string, width: number, height: number): Doc }

const engine = path.resolve(__dirname, '..', 'public', 'engine')
let TableDocument: DocClass

beforeAll(async () => {
  const sdk = await import(/* @vite-ignore */ pathToFileURL(path.join(engine, 'kition_table_kit.js')).href)
  await sdk.default({ module_or_path: fs.readFileSync(path.join(engine, 'kition_table_kit_bg.wasm')) })
  TableDocument = sdk.TableDocument
})

function sourceOf(doc: Doc, refuse: (method: string, nth: number) => boolean = () => false): EngineTableDataSource {
  const counts = new Map<string, number>()
  const access: TableEngineAccess = {
    run: async (command, params) => {
      const nth = (counts.get(command) ?? 0) + 1
      counts.set(command, nth)
      if (refuse(command, nth)) throw new Error(`The engine refused ${command}`)
      return doc.execute(command, JSON.stringify(params))
    },
    invoke: async <T,>(name: string) => (doc as unknown as Record<string, () => T>)[name]!.call(doc),
    revision: () => Number(doc.revision),
  }
  return createWorkerTableDataSource(access, 'Receipts.ktable')
}

const call = <T,>(doc: Doc, method: string, params: Record<string, unknown> = {}) => (JSON.parse(doc.execute(method, JSON.stringify(params))) as { result: T }).result

/** What the document holds: every table with its title, its fields' titles and its records' values. */
function contents(doc: Doc) {
  const tables = call<{ tables: Array<{ id: number; title: string; fields: Array<{ title: string }> }> }>(doc, 'file.get').tables
  return tables.map((table) => ({
    id: table.id,
    title: table.title,
    fields: table.fields.map((field) => field.title),
    records: call<{ items: Array<{ values: Record<string, unknown> }> }>(doc, 'record.list', { tableId: table.id }).items.map((row) => row.values),
  }))
}

/** A document with one person's edit in it (a table "Earlier" with one record), the step Undo would take back. */
function documentWithAnEdit() {
  const doc = TableDocument.create('Receipts.ktable', 0, 0)
  const earlier = call<{ id: number; fields: Array<{ name: string }> }>(doc, 'table.add', { title: 'Earlier' })
  call(doc, 'record.add', { tableId: earlier.id, values: { [earlier.fields[0].name]: 'kept' } })
  return doc
}

const receipt = (name: string, text: string) => new File([text], name, { type: 'image/png' })
const RECEIPTS = [receipt('cafe.png', 'merchant=Blue Cafe;total=12.50'), receipt('torn.png', 'merchant=unreadable'), receipt('books.png', 'merchant=Page One;total=48.00')]
const DESIGN = JSON.stringify({
  tableName: 'Receipt Archive',
  fields: [
    { name: 'Receipt', type: 'attachment' },
    { name: 'Merchant', type: 'text', isAIField: true, aiPrompt: 'Extract the merchant.' },
    { name: 'Total', type: 'text', isAIField: true, aiPrompt: 'Extract the total.' },
  ],
})

/** The host's model, scripted: it designs the receipt table, reads merchants, cannot read a torn receipt, and refuses every total. */
function model(): Pick<HostServices, 'generates' | 'generate'> {
  return {
    generates: true,
    generate: vi.fn(async (params: GenerateParams): Promise<GenerateResult> => {
      if (params.purpose === 'table.schema') return { text: DESIGN }
      const parts = params.messages.flatMap((message) => message.parts)
      const instruction = parts.map((part) => (part.type === 'text' ? part.text : '')).join(' ')
      if (instruction.includes('total')) throw new Error('The model refused to read totals')
      const image = parts.find((part): part is { type: 'image'; mime: string; bytes: ArrayBuffer } => part.type === 'image' && 'bytes' in part)
      const read = image ? new TextDecoder().decode(image.bytes) : ''
      if (read.includes('unreadable')) throw new Error('The model could not read the image')
      return { text: /merchant=([^;]*)/.exec(read)?.[1] ?? '' }
    }),
  }
}

describe('a table from files leaves no half-made table', () => {
  it('after a file that cannot be read, the document holds what it held before', async () => {
    const doc = documentWithAnEdit()
    const before = contents(doc)
    const unreadable = Object.assign(receipt('moved.png', 'merchant=Gone'), { arrayBuffer: () => Promise.reject(new DOMException('The file could not be read', 'NotReadableError')) })
    const source = sourceOf(doc)
    await expect(tableFromFiles({ source, services: model(), files: [RECEIPTS[0], unreadable, RECEIPTS[2]], request: 'Archive my receipts' })).rejects.toThrow()
    expect(contents(doc)).toEqual(before)
    doc.free()
  })

  it('after the engine refuses the second record, the document holds what it held before', async () => {
    const doc = documentWithAnEdit()
    const before = contents(doc)
    const source = sourceOf(doc, (command, nth) => command === 'record.add' && nth === 2)
    await expect(tableFromFiles({ source, services: model(), files: RECEIPTS, request: 'Archive my receipts' })).rejects.toThrow()
    expect(contents(doc)).toEqual(before)
    doc.free()
  })
})

describe('making a table from files is one undo step', () => {
  it('one Undo after a run takes back the table with its records, and nothing else', async () => {
    const doc = documentWithAnEdit()
    const before = contents(doc)
    const source = sourceOf(doc)
    const services = model()
    const made = await tableFromFiles({ source, services, runField: engineAIRun(source, services), files: RECEIPTS, request: 'Archive my receipts' })
    expect(contents(doc).find((table) => table.id === made.tableId)?.records).toHaveLength(3)
    expect(call<{ done: boolean }>(doc, 'history.undo').done).toBe(true)
    expect(contents(doc)).toEqual(before)
    // The person's earlier edit (the record in "Earlier") is the next step.
    expect(call<{ done: boolean }>(doc, 'history.undo').done).toBe(true)
    expect(contents(doc).find((table) => table.title === 'Earlier')?.records).toEqual([])
    doc.free()
  })

  it('Undo and Redo after a cancel bring nothing of the run back', async () => {
    const doc = documentWithAnEdit()
    const before = contents(doc)
    const source = sourceOf(doc)
    const services = model()
    const stop = new AbortController()
    const run = tableFromFiles({
      source,
      services,
      runField: engineAIRun(source, services),
      files: RECEIPTS,
      request: 'Archive my receipts',
      // The person cancels while the AI fields are filled.
      onProgress: (step) => (step.stage === 'ai' ? stop.abort() : undefined),
      signal: stop.signal,
    })
    await expect(run).rejects.toMatchObject({ name: 'AbortError' })
    expect(contents(doc)).toEqual(before)
    call(doc, 'history.undo')
    expect(contents(doc).some((table) => table.title === 'Receipt Archive')).toBe(false)
    call(doc, 'history.redo')
    expect(contents(doc).some((table) => table.title === 'Receipt Archive')).toBe(false)
    expect(contents(doc)).toEqual(before)
    doc.free()
  })

  it('Undo after a failure brings nothing of the run back', async () => {
    const doc = documentWithAnEdit()
    const source = sourceOf(doc, (command, nth) => command === 'record.add' && nth === 2)
    await expect(tableFromFiles({ source, services: model(), files: RECEIPTS, request: 'Archive my receipts' })).rejects.toThrow()
    call(doc, 'history.undo')
    expect(contents(doc).some((table) => table.title === 'Receipt Archive')).toBe(false)
    doc.free()
  })
})

describe('AI fields that leave cells empty', () => {
  it('are reported: the field the model refused, and the record it could not read', async () => {
    const doc = TableDocument.create('Receipts.ktable', 0, 0)
    const source = sourceOf(doc)
    const services = model()
    const made = await tableFromFiles({ source, services, runField: engineAIRun(source, services), files: RECEIPTS, request: 'Archive my receipts' })
    expect(made).toMatchObject({
      records: 3,
      aiFields: 2,
      unfilled: [
        { field: 'Merchant', records: 1 },
        { field: 'Total', records: 3 },
      ],
    })
    doc.free()
  })
})
