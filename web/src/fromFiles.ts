import { toBase64, type EngineTableDataSource } from './dataSource'
import type { HostServices } from './hostRequests'
import enUS from '../locales/en-US/table.json'

/**
 * A table from files: the person gives files and says what they want, and the document gets a new
 * table with one record a file (its name, and the file in the file's attachment field), and the AI
 * fields the request calls for, each filled from that record's own file.
 *
 * Everything is the engine's own commands. The model is asked twice, through the host
 * (`host/generate`): once for the table's design, under the kit's own purpose `table.schema`, and
 * then for each AI field's cells as any AI field run (`ai-field`). Without a host that runs models,
 * or when the host refuses the design (`not-allowed`, `unavailable`), the table gets the plain
 * design; an answer that is no design the kit can build fails the run. A file the model cannot read
 * stays a record to review and does not stop the others; the result says which AI fields left
 * which cells empty.
 *
 * The model's answer is untrusted (the files' names are in its prompt): the design is checked and
 * only what the kit allows is taken from it (see `planFromAnswer`, `fieldSeeds`, `viewSeeds`).
 *
 * History: making the table is one undo step, `table.add` as the person; the records go in as
 * the host feature `system:fromfiles` (outside the undo history), so Undo takes back the table and
 * the engine's repair removes its records with it. AI cells are written by `ai.apply` as
 * `system:aifield`, outside the undo history, as every AI field run is. Every file is read before
 * anything is written; after a failure or a cancel the new table is deleted again as
 * `system:fromfiles`, so the document holds what it held before and Undo finds no step of the run
 * (the engine passes over a step whose objects are gone). The files' blobs stay in the file: the
 * engine stores blobs outside the document and has no way to release one (`blobs.rs`).
 */

/** At most this many files in one run. */
export const MAX_FILES = 50
/** At most this many bytes a file: the engine's largest blob (`blobs::MAX_BLOB_BYTES`). */
export const MAX_FILE_BYTES = 50 << 20
/** At most this many bytes of all files of one run: every file is read before anything is written. */
export const MAX_TOTAL_BYTES = 100 << 20
/** At most this many fields a design names (the kit adds the file's name and attachment fields to them). */
export const MAX_FIELDS = 30
/** At most this many views a design names. */
export const MAX_VIEWS = 10
/** At most this many characters in a table's, field's, view's or choice's name. */
export const MAX_NAME_LENGTH = 100
/** At most this many characters in an AI field's instruction. */
export const MAX_PROMPT_LENGTH = 2000
/** At most this many choices a select field takes from a design. */
export const MAX_CHOICES = 50

/** The writes of a run that are no undo step: the records, and the cleanup after a failure. */
const ORIGIN = 'system:fromfiles'

/** The engine's names of the two fields the kit adds, the same in every locale. */
const FILE_NAME_KEY = 'file_name'
const ATTACHMENT_KEY = 'attachment'

/** The titles and instructions a run gives what it makes itself: the person's locale's (`kit.fromFiles.defaults`). */
export type FromFilesTitles = typeof enUS.kit.fromFiles.defaults
/** The en-US titles, for callers without the person's locale. */
export const EN_TITLES: FromFilesTitles = enUS.kit.fromFiles.defaults

/** Why a run failed, as a key of `kit.fromFiles.errors` with what its text names. */
export type FromFilesErrorCode = keyof typeof enUS.kit.fromFiles.errors
export const FROM_FILES_ERROR_CODES = Object.keys(enUS.kit.fromFiles.errors) as FromFilesErrorCode[]

/** A run's failure: `code` and `params` are what the dialog shows (localized); `message` is for developers. */
export class FromFilesError extends Error {
  constructor(
    readonly code: FromFilesErrorCode,
    message: string,
    readonly params: Record<string, string | number> = {},
    options?: { cause?: unknown },
  ) {
    super(message, options)
    this.name = 'FromFilesError'
  }
}

const FIELD_TYPES = ['text', 'long_text', 'number', 'date', 'single_select', 'multi_select', 'checkbox', 'url', 'attachment', 'ai']
const VIEW_TYPES = ['grid', 'kanban', 'calendar', 'gallery']
const DATE_FORMATS = [
  'year_month_day_slash',
  'year_month_day_time_slash',
  'year_month_day_time_zone_slash',
  'year_month_day_dash',
  'year_month_day_time_dash',
  'year_month_day_time_zone_dash',
  'month_day_dash',
  'month_day_year_slash',
  'day_month_year_slash',
]

/**
 * The option keys a design may set, by field type, each with its check (answers the value to keep,
 * or `undefined` to drop it). `ai_config` is never taken from a design: the kit builds it from a
 * checked `isAIField`, `aiPrompt` and `aiSource` (`fieldSeeds`).
 */
const OPTION_KEYS: Record<string, Record<string, (value: unknown) => unknown>> = {
  single_select: { choices: choices },
  multi_select: { choices: choices },
  date: { date_format: (value) => (typeof value === 'string' && DATE_FORMATS.includes(value) ? value : undefined) },
  long_text: { showAs: (value) => (value === 'text' || value === 'markdown' ? value : undefined) },
}

/**
 * Every option key the engine or the editor reads for some field type (`agent/commands.json`'s
 * `field.add`, `web/src/editor/types/dataDocument.ts`): a design sets them only where
 * `OPTION_KEYS` allows. Other keys are inert data the engine keeps as they are; a design keeps them
 * only as short scalars (`inertOption`).
 * TODO(owner): the gating test "seeds the engine" (fromFiles.spec.ts) expects an inert key
 * (`options.width`) and an inert view key (`config.startField`) to be kept, so they are not
 * dropped; dropping every key outside `OPTION_KEYS` needs that assertion changed.
 */
const ENGINE_OPTION_KEYS = new Set([
  'ai_config',
  'choices',
  'choice_tones',
  'column_tone',
  'date_format',
  'showAs',
  'description',
  'precision',
  'max',
  'icon',
  'label',
  'action',
  'target_table_id',
  'allow_multiple',
  'symmetric_field_id',
  'link_field_id',
  'target_field_id',
  'aggregation',
  'formula',
])
/** The view config keys the engine or the editor reads: a design names fields by title, so only `viewSeeds` sets them. */
const ENGINE_VIEW_KEYS = new Set([
  'filter_tree',
  'sorts',
  'groups',
  'row_height',
  'frozen_column_count',
  'search_query',
  'hidden_field_names',
  'cover_field_name',
  // Keys a view config no longer has: the engine refuses them.
  'sort',
  'filter',
  'group_field_name',
  'group_field',
])
/** At most this many inert option or config keys a design sets on one field or view. */
const MAX_INERT_KEYS = 8

function choices(value: unknown) {
  if (!Array.isArray(value)) return undefined
  const kept: string[] = []
  for (const item of value) {
    const choice = text(item)
    if (choice && choice.length <= MAX_NAME_LENGTH && !kept.includes(choice)) kept.push(choice)
    if (kept.length === MAX_CHOICES) break
  }
  return kept.length ? kept : undefined
}

function inertValue(value: unknown) {
  if (typeof value === 'number' || typeof value === 'boolean') return Number.isFinite(value) || typeof value === 'boolean' ? value : undefined
  if (typeof value === 'string' && value.length <= MAX_NAME_LENGTH) return value
  return undefined
}

/** A design's options for a field of `type`: the keys `OPTION_KEYS` allows, checked, and short inert scalars. */
function fieldOptions(type: string, raw: unknown): Record<string, unknown> | undefined {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return undefined
  const allowed = OPTION_KEYS[type] ?? {}
  const options: Record<string, unknown> = {}
  let inert = 0
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    const check = allowed[key]
    const kept = check ? check(value) : ENGINE_OPTION_KEYS.has(key) || key.length > 40 || inert >= MAX_INERT_KEYS ? undefined : inertValue(value)
    if (kept === undefined) continue
    if (!check) inert += 1
    options[key] = kept
  }
  return Object.keys(options).length ? options : undefined
}

type FieldPlan = {
  name: string
  type: string
  options?: Record<string, unknown>
  isAIField: boolean
  aiSource: string
  aiPrompt: string
  isPrimary: boolean
  /** The engine's name for a field the kit adds (`file_name`, `attachment`), the same in every locale. */
  key?: string
}
type ViewPlan = { name: string; kind: string; config?: Record<string, unknown> }
export type TablePlan = { tableName: string; fields: FieldPlan[]; views: ViewPlan[] }

/** The design a table gets without a model: the file, a summary, and a date read from it, titled in `titles`' language. */
export function plainPlan(titles: FromFilesTitles = EN_TITLES) {
  return JSON.stringify({
    tableName: titles.plainTable,
    fields: [
      { name: titles.fileName, type: 'text', isPrimary: true },
      { name: titles.attachment, type: 'attachment' },
      { name: titles.summary, type: 'text', isAIField: true, aiPrompt: titles.summaryPrompt },
      { name: titles.detectedDate, type: 'date', isAIField: true, aiPrompt: titles.detectedDatePrompt },
    ],
    views: [{ name: titles.view, kind: 'grid' }],
  })
}

/** The plain design in en-US. */
export const PLAIN_PLAN = plainPlan()

const DESIGN_PROMPT = `You are a database schema designer. Given a user request and a list of input files,
design a single table that fits the request. Reply with a JSON object only, no prose and no
markdown fences, with exactly these keys:

  tableName (string)
  fields    (array of field objects, at most ${MAX_FIELDS})
  views     (array of view objects, at most ${MAX_VIEWS})

Every name is at most ${MAX_NAME_LENGTH} characters, and no two fields share a name.

Each field object has:
  name      (string, required)
  type      (string, required, one of: ${FIELD_TYPES.join(', ')})
  options   (object, optional: {"choices": [string, ...]} for single_select and multi_select)
  isAIField (boolean, optional, true if the value must be extracted by an AI vision model from an
            attachment column; an attachment field is never an AI field)
  aiSource  (string, optional, when isAIField is true: the name of the attachment column the
            vision model should read. Defaults to the attachment column the file is put in.)
  aiPrompt  (string, required when isAIField is true: a concise instruction the vision model can
            act on, e.g. "Extract the merchant name from the receipt image.")
  isPrimary (boolean, optional, at most one field per table)

Each view object has:
  name   (string, required)
  kind   (string, required, one of: grid, calendar, kanban, gallery)
  config (object, optional: {"groupField": "<a single_select or checkbox field's name>"} for a
          kanban view, {"coverField": "<an attachment field's name>"} for a gallery view)

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

function checkLength(name: string) {
  if (name.length > MAX_NAME_LENGTH) {
    throw new FromFilesError('nameTooLong', `The name ${JSON.stringify(name.slice(0, 40))}… is longer than ${MAX_NAME_LENGTH} characters`, { max: MAX_NAME_LENGTH })
  }
}

/** `name`, or `name 2`, `name 3`, … when a name before it took it (compared without case). */
function unique(name: string, taken: Set<string>) {
  let candidate = name
  for (let counter = 2; taken.has(candidate.toLowerCase()); counter += 1) candidate = `${name} ${counter}`
  taken.add(candidate.toLowerCase())
  return candidate
}

/**
 * A model's answer as a table's design, completed and checked; throws a `FromFilesError` saying
 * what is wrong. The rules:
 * - The answer is a JSON object; at most `MAX_FIELDS` fields and `MAX_VIEWS` views; every name is
 *   at most `MAX_NAME_LENGTH` characters; a field or view without a name, and an AI field without
 *   an instruction, are refused.
 * - The first field is the file's name and the primary one (the kit's own, replacing a field of
 *   that title in first place). A later field whose title an earlier one has (without case) is
 *   renamed `<title> 2`, `<title> 3`, …; so is a view.
 * - An attachment field is never an AI field: a design that makes one an AI field is refused.
 *   The first attachment field is the file's field; when the design has none, the kit adds one
 *   after the file's name.
 * - An AI field (`isAIField`, or type `ai`) reads an attachment field: its `aiSource`, or the
 *   file's field when it names none. A source that is no field, or no attachment field, is refused.
 *   (Refused rather than repaired: a design that reads something else asked for something the kit
 *   cannot make, and the person should see that instead of a table that fills nothing.)
 * - Options: only what `OPTION_KEYS` allows for the field's type (and short inert scalars); a view's
 *   config is checked by `viewSeeds`.
 */
export function planFromAnswer(answer: string, titles: FromFilesTitles = EN_TITLES): TablePlan {
  let raw: { tableName?: unknown; fields?: unknown; views?: unknown }
  try {
    raw = JSON.parse(withoutFences(answer)) as typeof raw
  } catch (error) {
    throw new FromFilesError('designNotJson', `The design is not JSON: ${error instanceof Error ? error.message : String(error)}`)
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new FromFilesError('designNotObject', 'The design is not a JSON object')
  const rawFields = Array.isArray(raw.fields) ? (raw.fields as unknown[]) : []
  const rawViews = Array.isArray(raw.views) ? (raw.views as unknown[]) : []
  if (rawFields.length > MAX_FIELDS) throw new FromFilesError('tooManyFields', `The design has more than ${MAX_FIELDS} fields`, { max: MAX_FIELDS })
  if (rawViews.length > MAX_VIEWS) throw new FromFilesError('tooManyViews', `The design has more than ${MAX_VIEWS} views`, { max: MAX_VIEWS })
  const fields: FieldPlan[] = rawFields.map((value) => {
    const item = (value && typeof value === 'object' ? value : {}) as Record<string, unknown>
    const type = text(item.type)
    const options = fieldOptions(type, item.options)
    return {
      name: text(item.name),
      type,
      ...(options ? { options } : {}),
      isAIField: item.isAIField === true,
      aiSource: text(item.aiSource),
      aiPrompt: text(item.aiPrompt),
      isPrimary: item.isPrimary === true,
    }
  })
  for (const field of fields) {
    if (!FIELD_TYPES.includes(field.type)) {
      throw new FromFilesError('unknownFieldType', `Field ${JSON.stringify(field.name)} has unknown type ${JSON.stringify(field.type)}`, { name: field.name, type: field.type })
    }
    if (!field.name) throw new FromFilesError('emptyFieldName', 'A field has an empty name')
    checkLength(field.name)
    if (field.type === 'attachment' && (field.isAIField || field.aiPrompt || field.aiSource)) {
      throw new FromFilesError('attachmentIsAI', `Attachment field ${JSON.stringify(field.name)} is made an AI field`, { name: field.name })
    }
  }
  const fileName: FieldPlan = { name: titles.fileName, type: 'text', isAIField: false, aiSource: '', aiPrompt: '', isPrimary: true, key: FILE_NAME_KEY }
  if (fields[0]?.name === titles.fileName) fields[0] = fileName
  else fields.unshift(fileName)
  for (const field of fields.slice(1)) field.isPrimary = false
  if (!fields.some((field) => field.type === 'attachment')) {
    fields.splice(1, 0, { name: titles.attachment, type: 'attachment', isAIField: false, aiSource: '', aiPrompt: '', isPrimary: false, key: ATTACHMENT_KEY })
  }
  // An AI field's source is read by the titles the design gave, before any is renamed.
  const byTitle = new Map<string, FieldPlan>()
  for (const field of fields) if (!byTitle.has(field.name)) byTitle.set(field.name, field)
  const attached = fields.find((field) => field.type === 'attachment')!
  const sources = new Map<FieldPlan, FieldPlan>()
  for (const field of fields) {
    if (!field.isAIField && field.type !== 'ai') continue
    field.isAIField = true
    if (!field.aiPrompt) throw new FromFilesError('aiFieldNoPrompt', `AI field ${JSON.stringify(field.name)} has an empty aiPrompt`, { name: field.name })
    if (field.aiPrompt.length > MAX_PROMPT_LENGTH) {
      throw new FromFilesError('promptTooLong', `AI field ${JSON.stringify(field.name)} has an aiPrompt longer than ${MAX_PROMPT_LENGTH} characters`, { name: field.name, max: MAX_PROMPT_LENGTH })
    }
    const source = field.aiSource ? byTitle.get(field.aiSource) : attached
    if (!source) {
      throw new FromFilesError('aiSourceMissing', `AI field ${JSON.stringify(field.name)} reads ${JSON.stringify(field.aiSource)}, which is not a field`, { name: field.name, source: field.aiSource })
    }
    if (source.type !== 'attachment') {
      throw new FromFilesError('aiSourceNotAttachment', `AI field ${JSON.stringify(field.name)} reads ${JSON.stringify(field.aiSource)}, which is not an attachment field`, {
        name: field.name,
        source: field.aiSource,
      })
    }
    sources.set(field, source)
  }
  const takenFields = new Set<string>()
  for (const field of fields) field.name = unique(field.name, takenFields)
  for (const [field, source] of sources) field.aiSource = source.name

  const views: ViewPlan[] = rawViews.map((value) => {
    const item = (value && typeof value === 'object' ? value : {}) as Record<string, unknown>
    const config = item.config && typeof item.config === 'object' && !Array.isArray(item.config) ? (item.config as Record<string, unknown>) : undefined
    return { name: text(item.name), kind: text(item.kind), ...(config ? { config } : {}) }
  })
  if (!views.length) views.push({ name: titles.view, kind: 'grid' })
  const takenViews = new Set<string>()
  for (const view of views) {
    if (!view.name || !view.kind) throw new FromFilesError('emptyView', 'A view has an empty name or kind')
    checkLength(view.name)
    view.name = unique(view.name, takenViews)
  }
  const tableName = text(raw.tableName) || titles.table
  checkLength(tableName)
  return { tableName, fields, views }
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

/** The engine's names of a plan's fields, in order: the kit's own fields keep theirs. */
function engineNames(plan: TablePlan) {
  const taken = new Set<string>(plan.fields.flatMap((field) => (field.key ? [field.key] : [])))
  return plan.fields.map((field) => field.key ?? internalName(field.name, taken))
}

/**
 * The fields of `table.add`: an AI field is long text that reads its source field (`extract`).
 * Its `ai_config` is the kit's own, built from the checked plan; a design's options never carry one.
 */
export function fieldSeeds(plan: TablePlan) {
  const names = engineNames(plan)
  const nameOf = (title: string) => names[plan.fields.findIndex((field) => field.name === title)] ?? ''
  return plan.fields.map((field, index) => {
    const options: Record<string, unknown> = { ...(field.options ?? {}) }
    delete options.ai_config
    if (field.isAIField) {
      options.ai_config = { enabled: true, type: 'extract', source_field_name: nameOf(field.aiSource), extract_instruction: field.aiPrompt, output_format: 'text' }
    }
    return {
      name: names[index],
      title: field.name,
      type: field.isAIField ? 'long_text' : field.type,
      primary: field.isPrimary,
      required: field.isPrimary,
      readonly: field.isAIField,
      options,
      ai_prompt: field.aiPrompt,
    }
  })
}

/**
 * The views of `table.add`: a calendar only when the table has a date field; a kind the engine
 * has no view for is a grid. A view's config takes `groupField` (kanban: a single_select or checkbox
 * field) and `coverField` (gallery: an attachment or url field) by the field's title, as the
 * engine's first `groups` entry and `cover_field_name`; a title that names no such field is left out.
 * Config keys the engine reads are never taken from a design as given; other keys only as short
 * inert scalars.
 */
export function viewSeeds(plan: TablePlan) {
  const names = engineNames(plan)
  const fieldNamed = (title: unknown, types: string[]) => {
    const index = plan.fields.findIndex((field) => field.name === text(title) && !field.isAIField && types.includes(field.type))
    return index < 0 ? undefined : names[index]
  }
  const hasDate = plan.fields.some((field) => field.type === 'date')
  return plan.views
    .filter((view) => view.kind !== 'calendar' || hasDate)
    .map((view) => {
      const type = VIEW_TYPES.includes(view.kind) ? view.kind : 'grid'
      let config: Record<string, unknown> | null = null
      if (view.config) {
        const kept: Record<string, unknown> = {}
        let inert = 0
        for (const [key, value] of Object.entries(view.config)) {
          if (key === 'groupField' || key === 'coverField') continue
          if (ENGINE_VIEW_KEYS.has(key) || key.length > 40 || inert >= MAX_INERT_KEYS) continue
          const scalar = inertValue(value)
          if (scalar === undefined) continue
          kept[key] = scalar
          inert += 1
        }
        const group = type === 'kanban' ? fieldNamed(view.config.groupField, ['single_select', 'checkbox']) : undefined
        if (group) kept.groups = [{ field_name: group, direction: 'asc' }]
        const cover = type === 'gallery' ? fieldNamed(view.config.coverField, ['attachment', 'url']) : undefined
        if (cover) kept.cover_field_name = cover
        config = Object.keys(kept).length ? kept : null
      }
      return { title: view.name, type, config }
    })
}

/** How far a run is, for the person watching it. */
export type FromFilesProgress =
  | { stage: 'design' }
  | { stage: 'table'; tableId: number; title: string }
  | { stage: 'records'; done: number; total: number }
  | { stage: 'ai'; field: string; done: number; total: number }

type CreatedField = { id: number; name: string; title?: string; type: string; is_primary?: boolean; options?: { ai_config?: unknown } | null; ai_config?: unknown }
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
  /** The titles of what the run makes itself, in the person's language (en-US without them). */
  titles?: FromFilesTitles
  /** The host refused a model call as `unavailable` (no model set up): its message. */
  onUnavailable?: (message: string) => void
  onProgress?: (progress: FromFilesProgress) => void
  signal?: AbortSignal
}

/** An AI field that left cells empty: how many of the run's records, and why when the whole field failed. */
export type UnfilledField = { field: string; records: number; error?: string }

export type FromFilesResult = {
  tableId: number
  records: number
  aiFields: number
  /** Why the plain design was used though the host runs models: the host refused the design. */
  fallback?: 'not-allowed' | 'unavailable'
  /** The AI fields that left cells empty. */
  unfilled: UnfilledField[]
}

function isAbort(error: unknown) {
  return error instanceof DOMException && error.name === 'AbortError'
}

function canceled(signal?: AbortSignal) {
  if (signal?.aborted) throw new DOMException('Canceled', 'AbortError')
}

/** `promise`, or the cancel as soon as `signal` aborts (the host's answer is then not waited for). */
function untilCanceled<T>(promise: Promise<T>, signal?: AbortSignal): Promise<T> {
  if (!signal) return promise
  canceled(signal)
  return new Promise<T>((resolve, reject) => {
    const abort = () => reject(new DOMException('Canceled', 'AbortError'))
    signal.addEventListener('abort', abort, { once: true })
    promise.then(
      (value) => {
        signal.removeEventListener('abort', abort)
        resolve(value)
      },
      (error: unknown) => {
        signal.removeEventListener('abort', abort)
        reject(error)
      },
    )
  })
}

function refusalReason(error: unknown) {
  const reason = (error as { data?: { reason?: unknown } } | null)?.data?.reason
  return typeof reason === 'string' ? reason : ''
}

function messageOf(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

/**
 * The table's design: the model's for the request and the files, or the plain one without a model
 * and when the host refuses the purpose (`not-allowed`) or has no model (`unavailable`). Any other
 * failure, and an answer that is no design, fail the run.
 */
async function design(options: FromFilesOptions, titles: FromFilesTitles): Promise<{ plan: TablePlan; fallback?: FromFilesResult['fallback'] }> {
  const { services, files, request, signal } = options
  if (!services.generates) return { plan: planFromAnswer(plainPlan(titles), titles) }
  const listed = files.map((file) => `- ${file.name} • ${file.type || 'application/octet-stream'} • ${file.size} bytes`).join('\n')
  let answer: { text?: string }
  try {
    answer = await untilCanceled(
      services.generate(
        {
          purpose: 'table.schema',
          output: 'text',
          messages: [
            { role: 'system', parts: [{ type: 'text', text: DESIGN_PROMPT }] },
            { role: 'user', parts: [{ type: 'text', text: `User request:\n${request.trim()}\n\nUploaded files:\n${listed}\n` }] },
          ],
        },
        signal,
      ),
      signal,
    )
  } catch (error) {
    if (isAbort(error)) throw error
    const reason = refusalReason(error)
    if (reason === 'not-allowed' || reason === 'unavailable') {
      if (reason === 'unavailable') options.onUnavailable?.(messageOf(error))
      return { plan: planFromAnswer(plainPlan(titles), titles), fallback: reason }
    }
    throw new FromFilesError('designFailed', `The design could not be made: ${messageOf(error)}`, {}, { cause: error })
  }
  return { plan: planFromAnswer(answer.text ?? '', titles) }
}

/** Every file's bytes, read before anything is written: a file that cannot be read fails the run with nothing made. */
async function readAll(files: File[], signal?: AbortSignal) {
  const read: Uint8Array[] = []
  for (const file of files) {
    canceled(signal)
    try {
      read.push(new Uint8Array(await file.arrayBuffer()))
    } catch (error) {
      throw new FromFilesError('fileUnreadable', `${file.name} could not be read: ${messageOf(error)}`, { name: file.name }, { cause: error })
    }
  }
  return read
}

/**
 * Makes the table and answers its id, how many records and AI fields it has, and what it could not
 * do (the plain design used after a refusal, AI fields that left cells empty). No files, more than
 * `MAX_FILES`, an empty file, a file over `MAX_FILE_BYTES` and files over `MAX_TOTAL_BYTES` together
 * are refused before anything is asked or made. A failure or a cancel after the table is made
 * deletes it again (see the module's history notes) and throws on: a cancel as `AbortError`, the
 * rest as `FromFilesError`.
 */
export async function tableFromFiles(options: FromFilesOptions): Promise<FromFilesResult> {
  const { source, files, onProgress, signal, runField } = options
  const titles = options.titles ?? EN_TITLES
  if (!files.length) throw new FromFilesError('noFiles', 'Choose at least one file')
  if (files.length > MAX_FILES) throw new FromFilesError('tooManyFiles', `At most ${MAX_FILES} files at once`, { max: MAX_FILES })
  const large = files.find((file) => file.size > MAX_FILE_BYTES)
  if (large) throw new FromFilesError('fileTooLarge', `${large.name} is larger than ${MAX_FILE_BYTES >> 20} MB`, { name: large.name, max: MAX_FILE_BYTES >> 20 })
  const empty = files.find((file) => file.size === 0)
  if (empty) throw new FromFilesError('fileEmpty', `${empty.name} is empty`, { name: empty.name })
  if (files.reduce((sum, file) => sum + file.size, 0) > MAX_TOTAL_BYTES) {
    throw new FromFilesError('filesTooLarge', `The files are larger than ${MAX_TOTAL_BYTES >> 20} MB together`, { max: MAX_TOTAL_BYTES >> 20 })
  }

  onProgress?.({ stage: 'design' })
  const { plan, fallback } = await design(options, titles)
  const bytes = await readAll(files, signal)
  canceled(signal)
  let table: CreatedTable
  try {
    table = await source.command<CreatedTable>('table.add', { title: plan.tableName, fields: fieldSeeds(plan), views: viewSeeds(plan) })
  } catch (error) {
    throw new FromFilesError('engineRefused', `The table could not be made: ${messageOf(error)}`, {}, { cause: error })
  }
  try {
    onProgress?.({ stage: 'table', tableId: table.id, title: table.title })
    const primary = table.fields.find((field) => field.is_primary || field.id === table.primary_field_id)?.name
    const attachmentField = table.fields.find((field) => field.type === 'attachment')?.name
    const recordIds: number[] = []
    for (const [index, file] of files.entries()) {
      canceled(signal)
      // Held in the file as a blob, like an attachment put in a cell.
      const attachment = await source.command<unknown>('attachment.add', {
        tableId: table.id,
        name: file.name,
        mimeType: file.type || 'application/octet-stream',
        data: toBase64(bytes[index]),
      })
      const values: Record<string, unknown> = {}
      if (primary) values[primary] = file.name
      if (attachmentField) values[attachmentField] = [attachment]
      const record = await source.command<{ id: number }>('record.add', { tableId: table.id, values, asOrigin: ORIGIN })
      recordIds.push(record.id)
      onProgress?.({ stage: 'records', done: index + 1, total: files.length })
    }
    const aiFields = table.fields.filter((field) => field.options?.ai_config != null || field.ai_config != null)
    const unfilled: UnfilledField[] = []
    if (runField && options.services.generates && fallback !== 'unavailable') {
      for (const [index, field] of aiFields.entries()) {
        canceled(signal)
        const title = field.title || field.name
        onProgress?.({ stage: 'ai', field: title, done: index, total: aiFields.length })
        try {
          const answer = (await runField({ tableId: table.id, fieldId: field.id, record_ids: recordIds }, signal)) as { completed?: unknown } | null
          const completed = typeof answer?.completed === 'number' ? answer.completed : 0
          if (completed < recordIds.length) unfilled.push({ field: title, records: recordIds.length - completed })
        } catch (error) {
          if (isAbort(error)) throw error
          // A field that could not be run stays empty, for the person to run again; the rest go on.
          if (refusalReason(error) === 'unavailable') options.onUnavailable?.(messageOf(error))
          unfilled.push({ field: title, records: recordIds.length, error: messageOf(error) })
        }
      }
      onProgress?.({ stage: 'ai', field: '', done: aiFields.length, total: aiFields.length })
    }
    canceled(signal)
    return { tableId: table.id, records: recordIds.length, aiFields: aiFields.length, ...(fallback ? { fallback } : {}), unfilled }
  } catch (error) {
    // No half-made table is left behind, and no undo step of it: the delete is outside the history.
    try {
      await source.command('table.delete', { tableId: table.id, asOrigin: ORIGIN })
    } catch (cleanup) {
      throw new FromFilesError('leftBehind', `The table ${table.title} could not be deleted again: ${messageOf(cleanup)}`, { title: table.title }, { cause: error })
    }
    if (isAbort(error) || error instanceof FromFilesError) throw error
    throw new FromFilesError('engineRefused', `The table could not be made: ${messageOf(error)}`, {}, { cause: error })
  }
}
