/**
 * What the new grid edits through (grid plan 3.8): the table's fields, the records it touches, and
 * the engine's commands. The grid holds no records: a write first reads just the records it acts
 * on (`gridRecords`), then sends the engine's own commands (`transact` with `cells.set`,
 * `records.create`, `records.move`, `fields.move`; `views.patchField`; `views.patchConfig`), so
 * undo, collaboration and the agent's receipts are what they always were. A clear of many records
 * (a whole column) hands the engine the records' ids (`gridClear`), which writes the same one
 * `cells.set` without the page reading the records. Uploads go through the host's or the kit's
 * path for attachments (`attachments.put`), as the old grid's did.
 *
 * `engineData` is the implementation over the kit's engine (`KitEngine.invoke`), for the
 * development page; the editor's is `editorData`, over its data source.
 */
import type { KitEngine } from '@kition/kit-editor'
import { type AnyAIConfig, normalizeAIConfig } from '../../types/aiConfig'
import type { DataAttachment, DataField, DataRecordValue } from '../../types/dataDocument'
import type { TableOp } from '../ports/TableDataSource'

/** A record as the grid reads it to edit: its id, its row key (what writes name) and values by field name. */
export type GridRecord = { id: number; row_key: string; values: Record<string, DataRecordValue> }

export type { TableOp }

export type GridData = {
  readonly tableId: number
  readonly viewId: number | null
  /** A field of the table by id (as the editor reads it), or undefined. */
  field(fieldId: number): DataField | undefined
  /** The table's fields in their order. */
  fields(): DataField[]
  /** The records `ids` (in that order; ids that name no record are left out), with only `fieldNames` when given. */
  records(ids: number[], fieldNames?: string[]): Promise<GridRecord[]>
  /** One engine transaction: one undo step. */
  transact(ops: TableOp[]): Promise<void>
  /** One cell edit (`records.update`). */
  update(recordId: number, values: Record<string, DataRecordValue>): Promise<void>
  /** Saves a column's width in the view (`views.patchField`). */
  patchFieldWidth(fieldId: number, width: number): Promise<void>
  /** Saves how many columns the view freezes (`views.patchConfig`). */
  setFrozen(count: number): Promise<void>
  /** A blob the document holds (`attachments.read`), or null. */
  blob(hash: string): Promise<{ bytes: Uint8Array; mimeType: string } | null>
  /** Sets `values` (field name → empty value) on the records `ids` in one transaction, the records found by the engine (`gridClear`). */
  clear(ids: number[], values: Record<string, DataRecordValue>): Promise<void>
  /** Stores a file as an attachment of the table; answers what an attachment cell holds for it. Without it, attachments cannot be added. */
  upload?(file: File): Promise<DataAttachment>
}

type Answer<T> = { result?: T; error?: unknown }

/** A field as the editor reads it: an AI field's configuration moved from `options` to `ai_config` (as the editor's data source does). */
export function editorField(field: DataField): DataField {
  const options = (field.options ?? {}) as Record<string, unknown>
  if (!('ai_config' in options)) return field
  const { ai_config: config, ...rest } = options
  return { ...field, options: rest as DataField['options'], ai_config: normalizeAIConfig(config as AnyAIConfig | null | undefined) ?? null }
}

function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let index = 0; index < bytes.length; index += 0x8000) binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000))
  return btoa(binary)
}

function fromBase64(text: string): Uint8Array {
  const binary = atob(text)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

/**
 * A `GridData` over the kit's engine for table `tableId` and view `viewId`; `reload()` reads the
 * fields again. Uploads store the file in the document (`attachments.put`) unless the host's
 * `upload` is given (as the editor's `uploadAttachment` is).
 */
export async function engineData(
  engine: KitEngine,
  tableId: number,
  viewId: number | null,
  options: { upload?: (file: File) => Promise<DataAttachment> } = {},
): Promise<GridData & { reload(): Promise<void> }> {
  const call = async <T>(method: string, params: unknown): Promise<T> => {
    const answer = JSON.parse(await engine.invoke<string>('call', method, JSON.stringify(params))) as Answer<T>
    if (answer.error) throw new Error(`${method}: ${JSON.stringify(answer.error)}`)
    return answer.result as T
  }
  let fields = new Map<number, DataField>()
  const reload = async () => {
    const document = await call<{ tables: Array<{ id: number; fields: DataField[] }> }>('documents.get', {})
    const table = document.tables.find((item) => item.id === tableId)
    fields = new Map([...(table?.fields ?? [])].sort((a, b) => a.order - b.order).map((field) => [field.id, editorField(field)]))
  }
  await reload()
  return {
    tableId,
    viewId,
    reload,
    field: (fieldId) => fields.get(fieldId),
    fields: () => [...fields.values()],
    async records(ids, fieldNames) {
      if (!ids.length) return []
      return JSON.parse(await engine.invoke<string>('gridRecords', JSON.stringify({ tableId, ids, ...(fieldNames ? { fields: fieldNames } : {}) }))) as GridRecord[]
    },
    async transact(ops) {
      if (ops.length) await call('transact', { ops })
    },
    async update(recordId, values) {
      await call('records.update', { tableId, recordId, values })
    },
    async patchFieldWidth(fieldId, width) {
      if (viewId != null) await call('views.patchField', { tableId, viewId, fieldId, width })
    },
    async setFrozen(count) {
      if (viewId != null) await call('views.patchConfig', { tableId, viewId, frozen_column_count: count })
    },
    async blob(hash) {
      try {
        const blob = await call<{ data: string; mimeType: string }>('attachments.read', { hash })
        return { bytes: fromBase64(blob.data), mimeType: blob.mimeType }
      } catch {
        return null
      }
    },
    async clear(ids, values) {
      if (ids.length) await engine.invoke<string>('gridClear', JSON.stringify({ tableId, values }), Float64Array.from(ids))
    },
    upload:
      options.upload ??
      (async (file) => call<DataAttachment>('attachments.put', { tableId, name: file.name, mimeType: file.type, data: toBase64(new Uint8Array(await file.arrayBuffer())) })),
  }
}
