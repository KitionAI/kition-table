import { toBase64, type EngineTableDataSource } from './dataSource'
import type { HostServices } from './hostRequests'

/**
 * A table from files: the person gives files and says what they want, and the document gets a new
 * table with one record a file (its name, and the file attached once), and the AI fields the
 * request calls for, each filled from that record's own file.
 *
 * Everything is the engine's own commands. The model is asked twice, through the host
 * (`host/generate`): once for the table's design, under the kit's own purpose `table.schema`, and
 * then for each AI field's cells as any AI field run (`ai-field`). Without a host that runs models
 * the table gets a plain design and its AI fields stay empty. A file the model cannot read stays a
 * record to review and does not stop the others. Canceling removes the table it was making.
 */

const FILE_NAME_FIELD = 'File Name'
const ATTACHMENT_FIELD = 'Attachment'
const DEFAULT_TABLE = 'Untitled Table'
const DEFAULT_VIEW = 'Overall'
/** At most this many files in one run. */
export const MAX_FILES = 50
/** At most this many bytes a file. */
export const MAX_FILE_BYTES = 50 << 20

const FIELD_TYPES = ['text', 'long_text', 'number', 'date', 'single_select', 'multi_select', 'checkbox', 'url', 'attachment', 'ai']
const VIEW_TYPES = ['grid', 'kanban', 'calendar', 'gallery']

type FieldPlan = { name: string; type: string; options?: Record<string, unknown>; isAIField: boolean; aiSource: string; aiPrompt: string; isPrimary: boolean }
type ViewPlan = { name: string; kind: string; config?: Record<string, unknown> }
export type TablePlan = { tableName: string; fields: FieldPlan[]; views: ViewPlan[] }

/** The design a table gets without a model: the file, a summary, and a date read from it. */
export const PLAIN_PLAN = JSON.stringify({
  tableName: 'Files',
  fields: [
    { name: FILE_NAME_FIELD, type: 'text', isPrimary: true },
    { name: ATTACHMENT_FIELD, type: 'attachment' },
    { name: 'Summary', type: 'text', isAIField: true, aiPrompt: 'Summarize the contents of the attached file.' },
    { name: 'Detected Date', type: 'date', isAIField: true, aiPrompt: 'Extract a date from the attached file in ISO format if present.' },
  ],
  views: [{ name: DEFAULT_VIEW, kind: 'grid' }],
})

const DESIGN_PROMPT = `You are a database schema designer. Given a user request and a list of input files,
design a single table that fits the request. Reply with a JSON object only, no prose and no
markdown fences, with exactly these keys:

  tableName (string)
  fields    (array of field objects)
  views     (array of view objects)

Each field object has:
  name      (string, required)
  type      (string, required, one of: ${FIELD_TYPES.join(', ')})
  options   (object, optional, field-type-specific options)
  isAIField (boolean, optional, true if the value must be extracted by an AI vision model from an
            attachment column)
  aiSource  (string, optional, when isAIField is true: the name of the attachment column the
            vision model should read. Defaults to "${ATTACHMENT_FIELD}".)
  aiPrompt  (string, required when isAIField is true: a concise instruction the vision model can
            act on, e.g. "Extract the merchant name from the receipt image.")
  isPrimary (boolean, optional, at most one field per table)

Each view object has:
  name   (string, required)
  kind   (string, required, one of: grid, calendar, kanban, gallery)
  config (object, optional, view-kind-specific config, e.g. {"startField": "Date"} for a calendar
          view)

Reply with the JSON object and nothing else.`

function text(value: unknown) {
  return typeof value === 'string' ? value.trim() : ''
}

function withoutFences(answer: string) {
  const trimmed = answer.trim()
  if (!trimmed.startsWith('```')) return trimmed
  const rest = trimmed.slice(3).replace(/^json/i, '')
  const end = rest.lastIndexOf('```')
  return (end < 0 ? rest : rest.slice(0, end)).trim()
}

/**
 * A model's answer as a table's design, completed and checked: the first field is the file's
 * name and the primary one, there is a field the file is attached in, every AI field has an
 * instruction and reads a field that exists, and there is a view. Throws what is wrong.
 */
export function planFromAnswer(answer: string): TablePlan {
  let raw: { tableName?: unknown; fields?: unknown; views?: unknown }
  try {
    raw = JSON.parse(withoutFences(answer)) as typeof raw
  } catch (error) {
    throw new Error(`The design is not JSON: ${error instanceof Error ? error.message : String(error)}`)
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('The design is not a JSON object')
  const fields: FieldPlan[] = (Array.isArray(raw.fields) ? raw.fields : []).map((item: Record<string, unknown>) => ({
    name: text(item?.name),
    type: text(item?.type),
    ...(item?.options && typeof item.options === 'object' && !Array.isArray(item.options) ? { options: item.options as Record<string, unknown> } : {}),
    isAIField: item?.isAIField === true,
    aiSource: text(item?.aiSource),
    aiPrompt: text(item?.aiPrompt),
    isPrimary: item?.isPrimary === true,
  }))
  for (const field of fields) {
    if (!FIELD_TYPES.includes(field.type)) throw new Error(`Field ${JSON.stringify(field.name)} has unknown type ${JSON.stringify(field.type)}`)
  }
  const fileName: FieldPlan = { name: FILE_NAME_FIELD, type: 'text', isAIField: false, aiSource: '', aiPrompt: '', isPrimary: true }
  if (fields[0]?.name === FILE_NAME_FIELD) fields[0] = fileName
  else fields.unshift(fileName)
  for (const field of fields.slice(1)) field.isPrimary = false
  if (!fields.some((field) => field.type === 'attachment')) {
    fields.splice(1, 0, { name: ATTACHMENT_FIELD, type: 'attachment', isAIField: false, aiSource: '', aiPrompt: '', isPrimary: false })
  }
  const attached = fields.find((field) => field.type === 'attachment')?.name ?? ''
  const names = new Set(fields.map((field) => field.name))
  for (const field of fields) {
    if (!field.name) throw new Error('A field has an empty name')
    if (!field.isAIField && field.type !== 'ai') continue
    field.isAIField = true
    if (!field.aiSource) field.aiSource = attached
    if (!field.aiPrompt) throw new Error(`AI field ${JSON.stringify(field.name)} has an empty aiPrompt`)
    if (!names.has(field.aiSource)) throw new Error(`AI field ${JSON.stringify(field.name)} reads ${JSON.stringify(field.aiSource)}, which is not a field`)
  }
  const views: ViewPlan[] = (Array.isArray(raw.views) ? raw.views : []).map((item: Record<string, unknown>) => ({
    name: text(item?.name),
    kind: text(item?.kind),
    ...(item?.config && typeof item.config === 'object' && !Array.isArray(item.config) ? { config: item.config as Record<string, unknown> } : {}),
  }))
  if (!views.length) views.push({ name: DEFAULT_VIEW, kind: 'grid' })
  for (const view of views) {
    if (!view.name || !view.kind) throw new Error('A view has an empty name or kind')
  }
  return { tableName: text(raw.tableName) || DEFAULT_TABLE, fields, views }
}

/** The engine's name for a field titled `title`: lowercase letters, digits and `_`, not yet taken. */
function internalName(title: string, taken: Set<string>) {
  let base = title
    .trim()
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase()
  if (!base) base = 'field'
  else if (!/^[a-z]/.test(base)) base = `f_${base}`
  let name = base
  for (let counter = 2; taken.has(name); counter += 1) name = `${base}_${counter}`
  taken.add(name)
  return name
}

/** The fields of `tables.create`: an AI field is long text that reads its source field (`extract`). */
export function fieldSeeds(plan: TablePlan) {
  const taken = new Set<string>()
  const names = plan.fields.map((field) => internalName(field.name, taken))
  const nameOf = (title: string) => names[plan.fields.findIndex((field) => field.name === title)] ?? ''
  return plan.fields.map((field, index) => ({
    name: names[index],
    title: field.name,
    type: field.isAIField ? 'long_text' : field.type,
    primary: field.isPrimary,
    required: field.isPrimary,
    readonly: field.isAIField,
    options: {
      ...(field.options ?? {}),
      ...(field.isAIField
        ? { ai_config: { enabled: true, type: 'extract', source_field_name: nameOf(field.aiSource), extract_instruction: field.aiPrompt, output_format: 'text' } }
        : {}),
    },
    ai_prompt: field.aiPrompt,
  }))
}

/** The views of `tables.create`: a calendar only when the table has a date field. */
export function viewSeeds(plan: TablePlan) {
  const hasDate = plan.fields.some((field) => field.type === 'date')
  return plan.views
    .filter((view) => view.kind !== 'calendar' || hasDate)
    .map((view) => ({ title: view.name, type: VIEW_TYPES.includes(view.kind) ? view.kind : 'grid', config: view.config ?? null }))
}

/** How far a run is, for the person watching it. */
export type FromFilesProgress =
  | { stage: 'design' }
  | { stage: 'table'; tableId: number; title: string }
  | { stage: 'records'; done: number; total: number }
  | { stage: 'ai'; field: string; done: number; total: number }

type CreatedField = { id: number; name: string; title?: string; type: string; is_primary?: boolean; options?: { ai_config?: unknown } | null }
type CreatedTable = { id: number; title: string; primary_field_id?: number | null; fields: CreatedField[] }

/** Runs an AI field on records (`engineAIRun` of `aiFields.ts`). */
export type AIFieldRun = (params: { tableId: number; fieldId: number; record_ids: number[] }, signal?: AbortSignal) => Promise<unknown>

export type FromFilesOptions = {
  source: EngineTableDataSource
  services: Pick<HostServices, 'generates' | 'generate'>
  /** Fills an AI field; without it (a host that runs no models) the AI fields stay empty. */
  runField?: AIFieldRun
  files: File[]
  /** What the person asked for. */
  request: string
  onProgress?: (progress: FromFilesProgress) => void
  signal?: AbortSignal
}

function canceled(signal?: AbortSignal) {
  if (signal?.aborted) throw new DOMException('Canceled', 'AbortError')
}

/** The table's design: the model's for the request and the files, or the plain one without a model. */
async function design({ services, files, request }: Pick<FromFilesOptions, 'services' | 'files' | 'request'>): Promise<TablePlan> {
  if (!services.generates) return planFromAnswer(PLAIN_PLAN)
  const listed = files.map((file) => `- ${file.name} • ${file.type || 'application/octet-stream'} • ${file.size} bytes`).join('\n')
  const answer = await services.generate({
    purpose: 'table.schema',
    output: 'text',
    messages: [
      { role: 'system', parts: [{ type: 'text', text: DESIGN_PROMPT }] },
      { role: 'user', parts: [{ type: 'text', text: `User request:\n${request.trim()}\n\nUploaded files:\n${listed}\n` }] },
    ],
  })
  return planFromAnswer(answer.text ?? '')
}

/**
 * Makes the table and answers its id. A file that is too large, and more files than `MAX_FILES`,
 * are refused before anything is made. When the run is canceled, the table it made is deleted and
 * the cancel is thrown on (`AbortError`).
 */
export async function tableFromFiles(options: FromFilesOptions): Promise<{ tableId: number; records: number; aiFields: number }> {
  const { source, files, onProgress, signal, runField } = options
  if (!files.length) throw new Error('Choose at least one file')
  if (files.length > MAX_FILES) throw new Error(`At most ${MAX_FILES} files at once`)
  const large = files.find((file) => file.size > MAX_FILE_BYTES)
  if (large) throw new Error(`${large.name} is larger than ${MAX_FILE_BYTES >> 20} MB`)

  onProgress?.({ stage: 'design' })
  const plan = await design(options)
  canceled(signal)
  const table = await source.command<CreatedTable>('tables.create', { title: plan.tableName, fields: fieldSeeds(plan), views: viewSeeds(plan) })
  try {
    onProgress?.({ stage: 'table', tableId: table.id, title: table.title })
    const primary = table.fields.find((field) => field.is_primary || field.id === table.primary_field_id)?.name
    const attachmentFields = table.fields.filter((field) => field.type === 'attachment').map((field) => field.name)
    const recordIds: number[] = []
    for (const [index, file] of files.entries()) {
      canceled(signal)
      // Held in the file as a blob, like an attachment put in a cell.
      const attachment = await source.command<unknown>('attachments.put', {
        tableId: table.id,
        name: file.name,
        mimeType: file.type || 'application/octet-stream',
        data: toBase64(new Uint8Array(await file.arrayBuffer())),
      })
      const values: Record<string, unknown> = {}
      if (primary) values[primary] = file.name
      for (const name of attachmentFields) values[name] = [attachment]
      const record = await source.command<{ id: number }>('records.create', { tableId: table.id, values })
      recordIds.push(record.id)
      onProgress?.({ stage: 'records', done: index + 1, total: files.length })
    }
    const aiFields = table.fields.filter((field) => field.options?.ai_config != null)
    if (runField && options.services.generates) {
      for (const [index, field] of aiFields.entries()) {
        canceled(signal)
        onProgress?.({ stage: 'ai', field: field.title || field.name, done: index, total: aiFields.length })
        try {
          await runField({ tableId: table.id, fieldId: field.id, record_ids: recordIds }, signal)
        } catch (error) {
          if (error instanceof DOMException && error.name === 'AbortError') throw error
          // A field that could not be run stays empty, for the person to run again; the rest go on.
        }
      }
      onProgress?.({ stage: 'ai', field: '', done: aiFields.length, total: aiFields.length })
    }
    canceled(signal)
    return { tableId: table.id, records: recordIds.length, aiFields: aiFields.length }
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') {
      // No half-filled table is left behind.
      await source.command('tables.delete', { tableId: table.id }).catch(() => undefined)
    }
    throw error
  }
}
