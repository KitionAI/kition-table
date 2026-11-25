// A table from files: the design a model answers is completed and checked, the table gets one
// record a file with the file attached once, every AI field is filled from its own record's
// file, a file the model cannot read stays a record and does not stop the others, and a
// canceled run leaves no table behind. The receipts run over the built engine (public/engine:
// `pnpm engine` first), with the host's model scripted.
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { engineAIRun } from './aiFields'
import { createWorkerTableDataSource, type EngineTableDataSource, type TableEngineAccess } from './dataSource'
import { MAX_FILES, PLAIN_PLAN, fieldSeeds, planFromAnswer, tableFromFiles, viewSeeds, type FromFilesProgress } from './fromFiles'
import type { GenerateParams, GenerateResult, HostServices } from './hostRequests'

describe('the design of a table from files', () => {
  it('completes the plain design', () => {
    const plan = planFromAnswer(PLAIN_PLAN)
    expect(plan.tableName).toBe('Files')
    expect(plan.fields.map((field) => [field.name, field.type, field.isPrimary, field.isAIField])).toEqual([
      ['File Name', 'text', true, false],
      ['Attachment', 'attachment', false, false],
      ['Summary', 'text', false, true],
      ['Detected Date', 'date', false, true],
    ])
    // An AI field reads the attachment field unless it names another.
    expect(plan.fields.filter((field) => field.isAIField).map((field) => field.aiSource)).toEqual(['Attachment', 'Attachment'])
    expect(plan.views).toEqual([{ name: 'Overall', kind: 'grid' }])
  })

  it('adds the file name, the attachment field and a view, and reads an answer in a code fence', () => {
    const plan = planFromAnswer('```json\n{"tableName":" Receipts ","fields":[{"name":"Merchant","type":"ai","aiPrompt":"Who sold it?","isPrimary":true}]}\n```')
    expect(plan.tableName).toBe('Receipts')
    expect(plan.fields.map((field) => [field.name, field.type, field.isPrimary])).toEqual([
      ['File Name', 'text', true],
      ['Attachment', 'attachment', false],
      ['Merchant', 'ai', false],
    ])
    expect(plan.fields[2]).toMatchObject({ isAIField: true, aiSource: 'Attachment', aiPrompt: 'Who sold it?' })
    expect(plan.views).toEqual([{ name: 'Overall', kind: 'grid' }])
    expect(planFromAnswer('{}').tableName).toBe('Untitled Table')
  })

  it('refuses a design it cannot build, saying what is wrong', () => {
    expect(() => planFromAnswer('a table, please')).toThrow('The design is not JSON')
    expect(() => planFromAnswer('[]')).toThrow('not a JSON object')
    expect(() => planFromAnswer('{"fields":[{"name":"Total","type":"money"}]}')).toThrow('Field "Total" has unknown type "money"')
    expect(() => planFromAnswer('{"fields":[{"name":"","type":"text"}]}')).toThrow('A field has an empty name')
    expect(() => planFromAnswer('{"fields":[{"name":"Total","type":"number","isAIField":true}]}')).toThrow('AI field "Total" has an empty aiPrompt')
    expect(() => planFromAnswer('{"fields":[{"name":"Total","type":"number","isAIField":true,"aiPrompt":"Read it","aiSource":"Scan"}]}')).toThrow('reads "Scan", which is not a field')
    expect(() => planFromAnswer('{"views":[{"name":"","kind":"grid"}]}')).toThrow('A view has an empty name or kind')
  })

  it('seeds the engine: unique names, an AI field as long text that reads its source, a calendar only with a date', () => {
    const plan = planFromAnswer(
      JSON.stringify({
        fields: [
          { name: 'Total (USD)', type: 'number' },
          { name: 'total usd', type: 'text' },
          { name: '2nd scan', type: 'attachment' },
          { name: 'Merchant', type: 'text', isAIField: true, aiPrompt: 'Who sold it?', aiSource: '2nd scan', options: { width: 2 } },
        ],
        views: [{ name: 'By day', kind: 'calendar', config: { startField: 'Date' } }, { name: 'Board', kind: 'board' }],
      }),
    )
    const seeds = fieldSeeds(plan)
    expect(seeds.map((seed) => seed.name)).toEqual(['file_name', 'total_usd', 'total_usd_2', 'f_2nd_scan', 'merchant'])
    expect(seeds[0]).toMatchObject({ title: 'File Name', type: 'text', primary: true, required: true, readonly: false })
    expect(seeds[4]).toMatchObject({
      title: 'Merchant',
      type: 'long_text',
      readonly: true,
      options: { width: 2, ai_config: { enabled: true, type: 'extract', source_field_name: 'f_2nd_scan', extract_instruction: 'Who sold it?', output_format: 'text' } },
    })
    // No date field: the calendar is left out; a kind the engine has no view for is a grid.
    expect(viewSeeds(plan)).toEqual([{ title: 'Board', type: 'grid', config: null }])
    plan.fields.push({ name: 'Date', type: 'date', isAIField: false, aiSource: '', aiPrompt: '', isPrimary: false })
    expect(viewSeeds(plan)[0]).toEqual({ title: 'By day', type: 'calendar', config: { startField: 'Date' } })
  })
})

type Doc = { execute(command: string, params: string): string; save(): Uint8Array; readonly revision: bigint | number; free(): void }
type DocClass = { create(name: string, width: number, height: number): Doc; open(name: string, bytes: Uint8Array): Doc }

const engine = path.resolve(__dirname, '..', 'public', 'engine')
let TableDocument: DocClass

beforeAll(async () => {
  const sdk = await import(/* @vite-ignore */ pathToFileURL(path.join(engine, 'kition_table_kit.js')).href)
  await sdk.default({ module_or_path: fs.readFileSync(path.join(engine, 'kition_table_kit_bg.wasm')) })
  TableDocument = sdk.TableDocument
})

function sourceOf(doc: Doc): EngineTableDataSource {
  const access: TableEngineAccess = {
    run: async (command, params) => doc.execute(command, JSON.stringify(params)),
    invoke: async <T,>(name: string) => (doc as unknown as Record<string, () => T>)[name]!.call(doc),
    revision: () => Number(doc.revision),
  }
  return createWorkerTableDataSource(access, 'Receipts.ktable')
}

const call = <T,>(doc: Doc, method: string, params: Record<string, unknown> = {}) => (JSON.parse(doc.execute(`table.${method}`, JSON.stringify(params))) as { result: T }).result

/** A receipt: a file whose bytes say who sold what for how much. */
const receipt = (name: string, merchant: string, total: string) => new File([`merchant=${merchant};total=${total}`], name, { type: 'image/png' })

const RECEIPT_DESIGN = JSON.stringify({
  tableName: 'Receipt Archive',
  fields: [
    { name: 'File Name', type: 'text', isPrimary: true },
    { name: 'Receipt', type: 'attachment' },
    { name: 'Merchant', type: 'text', isAIField: true, aiPrompt: 'Extract the merchant.' },
    { name: 'Total', type: 'text', isAIField: true, aiPrompt: 'Extract the total.' },
  ],
  views: [{ name: 'Overall', kind: 'grid' }],
})

/** The host's model, scripted: it designs the receipt table, and reads a receipt's bytes. */
function model() {
  const asked: GenerateParams[] = []
  const generate = vi.fn(async (params: GenerateParams): Promise<GenerateResult> => {
    asked.push(params)
    if (params.purpose === 'table.schema') return { text: RECEIPT_DESIGN }
    const parts = params.messages.flatMap((message) => message.parts)
    const image = parts.find((part): part is { type: 'image'; mime: string; bytes: ArrayBuffer } => part.type === 'image' && 'bytes' in part)
    if (!image) throw new Error('no receipt was sent')
    const read = new TextDecoder().decode(image.bytes)
    if (read.includes('unreadable')) throw new Error('The model could not read the image')
    const instruction = parts.map((part) => (part.type === 'text' ? part.text : '')).join(' ')
    const wanted = instruction.includes('merchant') ? 'merchant' : 'total'
    return { text: new RegExp(`${wanted}=([^;]*)`).exec(read)?.[1] ?? '' }
  })
  const services: Pick<HostServices, 'generates' | 'generate'> = { generates: true, generate }
  return { services, asked, generate }
}

type Row = { id: number; values: Record<string, unknown> }
function rows(doc: Doc, tableId: number) {
  const table = call<{ tables: Array<{ id: number; title: string; fields: Array<{ name: string; title: string }> }> }>(doc, 'documents.get').tables.find((each) => each.id === tableId)!
  const name = (title: string) => table.fields.find((field) => field.title === title)!.name
  return call<{ items: Row[] }>(doc, 'records.list', { tableId }).items.map((row) => ({
    file: row.values[name('File Name')],
    attached: (row.values[name('Receipt')] as Array<{ name?: string }> | null)?.map((item) => item.name) ?? [],
    merchant: row.values[name('Merchant')] ?? null,
    total: row.values[name('Total')] ?? null,
  }))
}

describe('a table from receipts, over the engine', () => {
  it('makes one record a receipt, reads each AI field from its own receipt, keeps a receipt the model cannot read, and reopens with all of it', async () => {
    const doc = TableDocument.create('Receipts.ktable', 0, 0)
    const source = sourceOf(doc)
    const { services, asked } = model()
    const progress: FromFilesProgress[] = []
    const files = [receipt('cafe.png', 'Blue Cafe', '12.50'), receipt('torn.png', 'unreadable', '?'), receipt('books.png', 'Page One', '48.00')]

    const made = await tableFromFiles({ source, services, runField: engineAIRun(source, services), files, request: 'Archive my receipts', onProgress: (step) => progress.push(step) })

    expect(made).toMatchObject({ records: 3, aiFields: 2 })
    // The design was asked once, under the kit's own purpose, with the request and the files' names.
    const designs = asked.filter((params) => params.purpose === 'table.schema')
    expect(designs).toHaveLength(1)
    const sent = designs[0].messages.flatMap((message) => message.parts).map((part) => (part.type === 'text' ? part.text : '')).join('\n')
    expect(sent).toContain('Archive my receipts')
    expect(sent).toContain('- cafe.png • image/png')
    // One record a receipt, traced by its file name, the receipt attached once; every AI column
    // from that record's own receipt; the one the model could not read is a record to review.
    const expected = [
      { file: 'cafe.png', attached: ['cafe.png'], merchant: 'Blue Cafe', total: '12.50' },
      { file: 'torn.png', attached: ['torn.png'], merchant: null, total: null },
      { file: 'books.png', attached: ['books.png'], merchant: 'Page One', total: '48.00' },
    ]
    expect(rows(doc, made.tableId)).toEqual(expected)
    expect(progress[0]).toEqual({ stage: 'design' })
    expect(progress).toContainEqual({ stage: 'table', tableId: made.tableId, title: 'Receipt Archive' })
    expect(progress).toContainEqual({ stage: 'records', done: 3, total: 3 })
    expect(progress.at(-1)).toEqual({ stage: 'ai', field: '', done: 2, total: 2 })

    // The file reopens with all of it.
    const reopened = TableDocument.open('Receipts.ktable', doc.save())
    expect(rows(reopened, made.tableId)).toEqual(expected)
    reopened.free()
    doc.free()
  })

  it('makes the plain table and fills nothing when the host runs no models', async () => {
    const doc = TableDocument.create('Receipts.ktable', 0, 0)
    const source = sourceOf(doc)
    const generate = vi.fn()
    const made = await tableFromFiles({ source, services: { generates: false, generate }, files: [receipt('cafe.png', 'Blue Cafe', '12.50')], request: '' })
    expect(generate).not.toHaveBeenCalled()
    expect(made).toMatchObject({ records: 1, aiFields: 2 })
    const table = call<{ tables: Array<{ id: number; title: string }> }>(doc, 'documents.get').tables.find((each) => each.id === made.tableId)
    expect(table?.title).toBe('Files')
    doc.free()
  })

  it('leaves no table behind when the run is canceled, and makes nothing for a design it cannot build', async () => {
    const doc = TableDocument.create('Receipts.ktable', 0, 0)
    const source = sourceOf(doc)
    const before = call<{ tables: Array<{ id: number }> }>(doc, 'documents.get').tables.map((table) => table.id)
    const { services } = model()
    const stop = new AbortController()
    const canceled = tableFromFiles({
      source,
      services,
      runField: engineAIRun(source, services),
      files: [receipt('cafe.png', 'Blue Cafe', '12.50'), receipt('books.png', 'Page One', '48.00')],
      request: 'Archive my receipts',
      // The person cancels once the first record is in.
      onProgress: (step) => (step.stage === 'records' && step.done === 1 ? stop.abort() : undefined),
      signal: stop.signal,
    })
    await expect(canceled).rejects.toMatchObject({ name: 'AbortError' })
    expect(call<{ tables: Array<{ id: number }> }>(doc, 'documents.get').tables.map((table) => table.id)).toEqual(before)

    const bad = { generates: true, generate: vi.fn(async () => ({ text: 'I would make a nice table.' })) }
    await expect(tableFromFiles({ source, services: bad, files: [receipt('cafe.png', 'Blue Cafe', '12.50')], request: 'x' })).rejects.toThrow('The design is not JSON')
    expect(call<{ tables: Array<{ id: number }> }>(doc, 'documents.get').tables.map((table) => table.id)).toEqual(before)
    doc.free()
  })

  it('refuses no files and too many before anything is made', async () => {
    const doc = TableDocument.create('Receipts.ktable', 0, 0)
    const source = sourceOf(doc)
    const { services, generate } = model()
    await expect(tableFromFiles({ source, services, files: [], request: 'x' })).rejects.toThrow('Choose at least one file')
    const many = Array.from({ length: MAX_FILES + 1 }, (_, index) => receipt(`r${index}.png`, 'A', '1'))
    await expect(tableFromFiles({ source, services, files: many, request: 'x' })).rejects.toThrow(`At most ${MAX_FILES} files at once`)
    expect(generate).not.toHaveBeenCalled()
    doc.free()
  })
})
