/**
 * The grid's `GridData` in the editor: the editor's fields (marked read-only for a person who may
 * only view the table), and every read and write through the editor's data source, so the shell
 * sees each write (its revision, unsaved state) and co-editors and undo follow it as they follow
 * the editor's other writes. A clear of many records goes through `grid.clear` (`gridClear`);
 * the frozen count is the editor's view setting, which it saves; uploads are the editor's
 * (`TableBlobScope`: the host's, else the document's blobs), and without one nothing is added.
 */
import type { DataAttachment, DataField } from '../../types/dataDocument'
import type { TableDataSource } from '../ports/TableDataSource'
import type { GridData } from './gridData'

export type EditorDataOptions = {
  dataSource: TableDataSource
  documentId: number
  tableId: number
  viewId: number | null
  /** The table's fields as the editor holds them now. */
  fields(): DataField[]
  /** The view freezes `count` columns (the editor's setting). */
  setFrozen(count: number): void
  /** The editor's upload, when it has one. */
  upload?(file: File): Promise<DataAttachment>
  /** Runs around each write the grid sends, so the editor tells its writes from other changes. */
  writing?<T>(write: () => Promise<T>): Promise<T>
}

export function editorData(options: EditorDataOptions): GridData {
  const { dataSource, documentId, tableId, viewId } = options
  const writing = options.writing ?? (<T>(write: () => Promise<T>) => write())
  const attachments = dataSource.attachments
  return {
    tableId,
    viewId,
    field: (fieldId) => options.fields().find((field) => field.id === fieldId),
    fields: () => [...options.fields()].sort((a, b) => a.order - b.order),
    records: (ids, fieldNames) => (ids.length ? dataSource.grid.records({ tableId, ids, ...(fieldNames ? { fields: fieldNames } : {}) }) : Promise.resolve([])),
    async transact(ops) {
      if (ops.length) await writing(() => dataSource.transact({ documentId, ops }))
    },
    async update(recordId, values) {
      await writing(() => dataSource.records.update({ documentId, tableId, recordId, values }))
    },
    async patchFieldWidth(fieldId, width) {
      if (viewId != null) await writing(() => dataSource.views.patchField({ documentId, tableId, viewId, fieldId, width }))
    },
    async setFrozen(count) {
      options.setFrozen(count)
    },
    async blob(hash) {
      if (!attachments) return null
      try {
        const held = await attachments.read({ documentId, hash })
        return { bytes: held.data, mimeType: held.mimeType }
      } catch {
        return null
      }
    },
    async clear(ids, values) {
      await writing(() => dataSource.grid.clear({ tableId, ids, values }))
    },
    upload: options.upload,
  }
}
