import {
  normalizeAIConfig,
  type AnyAIConfig,
  type DataAttachment,
  type DataDocument,
  type DataField,
  type DataTable,
  type TableDataSource,
} from './editor'
import type { GridFieldSummary, GridRecordRead } from './editor/table/ports/TableDataSource'
import type { RecordHistoryResponse } from './editor/types/dataDocument'
import { decodeGridAxis, decodeGridHits } from './editor/table/lib/gridAxis'
import type { GridEngineFonts } from './editor/table/canvas/gridFonts'

/** What a data-document request answers from the engine: the route's answer, and what it changed. */
type Answer<T> = { result: T; change: unknown }

/** The session calls the data source makes (`useKitSession`'s `run`, and the open engine). */
export type TableEngineAccess = {
  /** Runs an engine command; `refresh: 'none'` for reads, which change nothing the shell shows. */
  run(command: string, params: Record<string, unknown>, refresh: 'all' | 'none'): Promise<string | null>
  /** Calls one of the engine's extras (`KitEngine.invoke`). */
  invoke<T>(name: string, ...args: unknown[]): Promise<T>
  /** The engine's revision now. */
  revision(): number
  /** The open engine's lazy fonts, which the grid follows. */
  fonts?: GridEngineFonts
}

/** The methods that only read (the engine's `READ_METHODS`): they run without refreshing the shell. */
export const READS: ReadonlySet<string> = new Set([
  'file.get',
  'field.linkCandidates',
  'tableView.listFields',
  'record.list',
  'record.history',
  'file.exportCsv',
  'history.state',
  'dashboard.list',
  'attachment.get',
  'file.previewImport',
  'ai.prepare',
  'change.list',
])

export function toBase64(bytes: Uint8Array) {
  let binary = ''
  for (let start = 0; start < bytes.length; start += 0x8000) binary += String.fromCharCode(...bytes.subarray(start, start + 0x8000))
  return btoa(binary)
}

export function fromBase64(text: string) {
  const binary = atob(text)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

/** What `attachment.get` answers: a blob held in the file, its bytes in base64. */
type StoredBlob = { hash: string; name: string; mimeType: string; sizeBytes: number; data: string }

/**
 * A field as the editor reads it: an AI field's configuration from its legacy place in `options`
 * moved to `ai_config` and normalized, as Kition's HTTP client does with every field it reads.
 */
function normalizeField(field: DataField): DataField {
  if (!field) return field
  const options = (field.options ?? {}) as Record<string, unknown>
  const legacy = 'ai_config' in options
  const config = normalizeAIConfig((legacy ? options.ai_config : field.ai_config) as AnyAIConfig | null | undefined)
  if (!legacy && config === field.ai_config) return field
  const rest = { ...options }
  delete rest.ai_config
  return { ...field, options: legacy ? rest : field.options, ai_config: config ?? null }
}

function normalizeTable<T extends Pick<DataTable, 'fields'>>(table: T): T {
  return table && Array.isArray(table.fields) ? { ...table, fields: table.fields.map(normalizeField) } : table
}

/**
 * The editor's data source, plus the engine commands the host glue runs itself (`table.<method>`:
 * file import, AI field runs) and the revision feed of the shell.
 */
export type EngineTableDataSource = TableDataSource & {
  /** Runs `table.<method>` and answers its result; the methods in `READS` refresh nothing. */
  command<T>(method: string, params?: Record<string, unknown>): Promise<T>
  observe(revision: number): void
  /** The shell opened the document again from its file (it changed on disk): everything is read again. */
  reopened(revision: number): void
}

type ChangeListener = Parameters<TableDataSource['onChange']>[1]
type ChangeEvent = Parameters<ChangeListener>[0]

/**
 * Separates the message of the engine's refusal of a call's parameters (kition-sdk's
 * `schema::refusal`, such as a record `values` key that names no field) from its errors as JSON.
 */
const REFUSAL_SEPARATOR = '\u001e'

/** An engine error as the editor shows it: a refusal's message without its errors' JSON. */
export function refusalMessage(error: unknown): unknown {
  if (error instanceof Error && error.message.includes(REFUSAL_SEPARATOR)) {
    error.message = error.message.slice(0, error.message.indexOf(REFUSAL_SEPARATOR))
  }
  return error
}

/**
 * Where a record's history before this session comes from (Kit UI protocol 7, 7m): `kept`, the
 * host keeps the document's replica; `checkpoints`, the host's checkpoints, oldest first, each
 * with its snapshot (base64): the history before this session is what differs between
 * consecutive ones.
 */
export type RecordHistorySource = {
  kept?: boolean
  checkpoints?: () => Promise<Array<{ version: string; time: number; author?: string; snapshot: string }> | null>
}

/** `params` without `documentId`: the engine holds one document. */
function own<T extends { documentId?: number }>({ documentId: _documentId, ...params }: T) {
  return params as Record<string, unknown>
}

/**
 * The table editor's data over the kit's engine (`TableDataSource`): each member is the engine
 * command `table.<method>`, which runs the same `kition-table-api` request Kition's runtime
 * routes run, so the editor gets the answers it gets in Kition. The engine holds one document,
 * `path`, which `documents.*` answer. Attachments are blobs held in the file
 * (`attachment.add`, `attachment.get`, bytes in base64): the editor shows them from object
 * URLs and uploads into the file, with nothing outside the document.
 *
 * `onChange` reports what the editor did not do through this source: changes imported from
 * co-editors (the engine queues their `TableChange`s), or, when the document moved otherwise (an
 * Undo from the shell, an agent's command), a `resync`. `observe(revision)` is told every engine
 * revision the shell sees.
 */
export function createWorkerTableDataSource(engine: TableEngineAccess, path: string, history: RecordHistorySource = {}) {
  const listeners = new Set<ChangeListener>()
  /** The editor's arrangement, told after every change to the document. */
  const gridListeners = new Set<() => void>()
  const changed = () => {
    for (const listener of gridListeners) listener()
  }
  /** The last revision this source saw or made. */
  let seen: number | null = null

  async function call<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    const read = READS.has(method)
    const answer = await engine.run(method, params, read ? 'none' : 'all').catch((error: unknown) => {
      throw refusalMessage(error)
    })
    if (answer == null) throw new Error('The table is not open.')
    if (!read) {
      seen = engine.revision()
      changed()
    }
    return (JSON.parse(answer) as Answer<T>).result
  }

  async function document() {
    const opened = await call<DataDocument>('file.get')
    return { ...opened, path, tables: opened.tables?.map(normalizeTable) }
  }

  function emit(event: ChangeEvent) {
    for (const listener of listeners) listener(event)
  }

  async function foreign() {
    const imported = JSON.parse(await engine.invoke<string>('takeImported')) as Array<Extract<ChangeEvent, { type: 'changed' }>['change']>
    if (!imported.length) {
      emit({ type: 'resync' })
      return
    }
    for (const change of imported) emit({ type: 'changed', change: { ...change, documentPath: path } })
  }

  const source: EngineTableDataSource = {
    documents: {
      get: () => document(),
      open: () => document(),
      list: async () => {
        const opened = await document()
        return { items: [opened], total: 1, offset: 0 }
      },
    },
    tables: {
      create: async (params) => normalizeTable(await call<DataTable>('table.add', own(params))),
      update: async (params) => normalizeTable(await call<DataTable>('table.set', own(params))),
      delete: (params) => call('table.delete', own(params)),
    },
    fields: {
      create: async (params) => normalizeField(await call<DataField>('field.add', own(params))),
      update: async (params) => normalizeField(await call<DataField>('field.set', own(params))),
      delete: (params) => call('field.delete', own(params)),
      duplicate: async (params) => normalizeField(await call<DataField>('field.duplicate', own(params))),
      linkCandidates: (params) => call('field.linkCandidates', own(params)),
    },
    views: {
      create: (params) => call('tableView.add', own(params)),
      update: (params) => call('tableView.set', own(params)),
      delete: (params) => call('tableView.delete', own(params)),
      listFields: (params) => call('tableView.listFields', own(params)),
      patchField: (params) => call('tableView.setField', own(params)),
    },
    records: {
      list: (params) => call('record.list', own(params)),
      create: (params) => call('record.add', own(params)),
      update: (params) => call('record.set', own(params)),
      delete: (params) => call('record.delete', own(params)),
      history: async (params) => {
        // The host's checkpoints tell the history before this session; a host that answers none
        // (or fails) leaves only this session's.
        const checkpoints = (await history.checkpoints?.().catch(() => null)) ?? null
        if (checkpoints?.length) {
          // Checkpoints the engine cannot take (a document too large to send its snapshots, one
          // that is no longer this document's) leave this session's history, said as such.
          const told = await call<RecordHistoryResponse>('record.history', { ...own(params), checkpoints }).catch(() => null)
          if (told) return { ...told, kept: history.kept ?? false }
          return { ...(await call<RecordHistoryResponse>('record.history', own(params))), kept: false }
        }
        const answer = await call<RecordHistoryResponse>('record.history', own(params))
        // A stored snapshot holds no changes: without checkpoints nothing tells what came before.
        return { ...answer, kept: (history.kept ?? false) && checkpoints !== null }
      },
      bulkDelete: (params) => call('record.bulkDelete', own(params)),
      bulkCopy: (params) => call('record.bulkCopy', own(params)),
    },
    links: {
      replace: (params) => call('recordLink.replace', own(params)),
    },
    transact: (params) => call('edit.transact', own(params)),
    undo: () => call('history.undo'),
    redo: () => call('history.redo'),
    historyState: () => call('history.state'),
    dashboards: {
      list: () => call('dashboard.list'),
      upsert: ({ documentId: _documentId, dashboardId, id: _id, ...dashboard }) => call('dashboard.set', { ...dashboard, dashboardId }),
    },
    attachments: {
      read: async ({ hash }) => {
        const blob = await call<StoredBlob>('attachment.get', { hash })
        return { data: fromBase64(blob.data), mimeType: blob.mimeType, name: blob.name }
      },
      put: async ({ tableId, file }) => {
        const data = toBase64(new Uint8Array(await file.arrayBuffer()))
        return call<DataAttachment>('attachment.add', { tableId, name: file.name, mimeType: file.type, data })
      },
    },
    onChange(_documentId, listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    grid: {
      open: async (params) => {
        const bytes = await engine.invoke<Uint8Array>('gridOpen', JSON.stringify(params))
        return { ...decodeGridAxis(bytes), bytes }
      },
      sync: async () => {
        const bytes = await engine.invoke<Uint8Array>('gridSync')
        return bytes.byteLength ? { ...decodeGridAxis(bytes), bytes } : null
      },
      hits: async (params) => decodeGridHits(await engine.invoke<Uint8Array>('gridHits', JSON.stringify(params)), params.fieldIds),
      subscribe(listener) {
        gridListeners.add(listener)
        return () => {
          gridListeners.delete(listener)
        }
      },
      layout: async ({ axis, ...params }) => {
        const axes = JSON.parse(await engine.invoke<string>('gridAxes', axis, JSON.stringify(params))) as unknown
        return { axes, rows: await engine.invoke<Uint8Array>('gridRows') }
      },
      bands: (epoch, keys, dpr, session, draft) => engine.invoke<Uint8Array>('gridBands', epoch, keys, dpr, session, draft),
      records: async (params) => JSON.parse(await engine.invoke<string>('gridRecords', JSON.stringify(params))) as GridRecordRead[],
      clear: async ({ ids, ...params }) => {
        if (!ids.length) return
        await engine.invoke<string>('gridClear', JSON.stringify(params), Float64Array.from(ids)).catch((error: unknown) => {
          throw refusalMessage(error)
        })
        // The engine wrote outside a command: a read through the session brings its state (the
        // revision, unsaved) up to date.
        await engine.run('history.state', {}, 'all')
        seen = engine.revision()
        changed()
      },
      summary: async (params) => JSON.parse(await engine.invoke<string>('gridSummary', JSON.stringify(params))) as GridFieldSummary[],
      picture: (bytes, maxSide) => engine.invoke<Uint8Array>('gridPicture', bytes, maxSide),
      fonts: engine.fonts,
      opticalSizing: (sizing) => engine.invoke<boolean>('gridOpticalSizing', sizing),
    },
    command: <T>(method: string, params?: Record<string, unknown>) => call<T>(method, params),
    observe(revision) {
      if (seen === revision) return
      const first = seen == null
      seen = revision
      if (!first) {
        changed()
        void foreign().catch(() => emit({ type: 'resync' }))
      }
    },
    reopened(revision) {
      // A new engine's revisions start again, so `observe` alone may see nothing new.
      seen = revision
      changed()
      emit({ type: 'resync' })
    },
  }
  return source
}
