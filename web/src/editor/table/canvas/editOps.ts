/**
 * The grid's data actions as engine operations (grid plan F10, F11, F12, F13), pure: what a copy
 * puts on the clipboard, and the `transact` ops of a paste, a clear, a fill, a row move and a
 * column move. `GridEditing` (gridEditing.ts) reads the records they need and sends the ops; each
 * action is one `transact`, so one undo step.
 *
 * Cells are named as the selection names them (`selection.ts`): linear rows and shown columns.
 * Only record rows hold cells; group headers and add rows in a range are skipped.
 */
import type { DataField, DataRecordValue } from '../../types/dataDocument'
import { formatTableFieldValue } from '../lib/dateFormatting'
import { coerceValue, parsePastedTable } from '../lib/tableEditorShared'
import type { GridRecord, TableOp } from './gridData'
import type { GridRows } from './rows'
import { boundsOf, type GridSelection, normalizeRanges } from './selection'

/** Fields whose values a person cannot write: computed, system or link fields. */
const COMPUTED = new Set<string>([
  'auto_number',
  'created_time',
  'last_modified_time',
  'created_by',
  'last_modified_by',
  'lookup',
  'rollup',
  'formula',
  'link_to_record',
  'user',
  'button',
])

/** Whether a person can write `field`'s cells (edit, paste, fill, clear). */
export function writable(field: DataField | undefined): field is DataField {
  return field != null && !field.readonly && !COMPUTED.has(field.type)
}

/** The records (in display order) and shown columns a selection covers. */
export function coveredCells(selection: GridSelection, rows: GridRows, columnCount: number): { recordIds: number[]; cols: number[] } {
  const records = (from: number, to: number, out: number[]) => {
    for (let row = Math.max(0, from); row <= Math.min(rows.count - 1, to); row += 1) {
      const id = rows.recordId(row)
      if (id != null) out.push(id)
    }
  }
  const span = (from: number, to: number) => Array.from({ length: Math.max(0, to - from + 1) }, (_, index) => from + index)
  const recordIds: number[] = []
  switch (selection.kind) {
    case 'cells': {
      const b = boundsOf(selection.anchor, selection.focus)
      records(b.top, b.bottom, recordIds)
      return { recordIds, cols: span(Math.max(0, b.left), Math.min(columnCount - 1, b.right)) }
    }
    case 'rows':
      for (const [from, to] of normalizeRanges(selection.ranges)) records(from, to, recordIds)
      return { recordIds, cols: span(0, columnCount - 1) }
    case 'columns':
      records(0, rows.count - 1, recordIds)
      return { recordIds, cols: span(Math.max(0, Math.min(selection.anchor, selection.focus)), Math.min(columnCount - 1, Math.max(selection.anchor, selection.focus))) }
    case 'none':
      return { recordIds, cols: [] }
  }
}

function tsvCell(field: DataField, value: unknown): string {
  const text = formatTableFieldValue(field, value).replace(/\r\n?/g, '\n')
  return /[\t\n"]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
}

/**
 * What a copy puts on the clipboard: the cells as tab-separated values, one line per record, each
 * cell as the grid shows it (dates formatted); a cell holding a tab, a line break or a quote is
 * quoted. The same text is the plain text and the TSV.
 */
export function serializeCells(records: GridRecord[], fields: DataField[]): string {
  return records.map((record) => fields.map((field) => tsvCell(field, record.values[field.name] ?? null)).join('\t')).join('\n')
}

/** The record rows from `row` on, in display order (`count` of them at most). */
export function recordsFrom(rows: GridRows, row: number, count: number): number[] {
  const ids: number[] = []
  let at = rows.recordFrom(Math.max(0, row), 1)
  while (at >= 0 && ids.length < count) {
    const id = rows.recordId(at)
    if (id != null) ids.push(id)
    at = at + 1 < rows.count ? rows.recordFrom(at + 1, 1) : -1
  }
  return ids
}

/**
 * A paste at a record: the text's rows go to that record and the ones after it (in display
 * order), its columns to the field from `fields[0]` on; rows past the last record become new
 * records. One `cells.set` for the records there are and one `records.create` for the rest, in one
 * transaction. Read-only and computed fields are skipped.
 */
export function pasteOps(tableId: number, text: string, targets: GridRecord[], fields: DataField[]): TableOp[] {
  const table = parsePastedTable(text)
  const updates: Array<{ rowKey: string; values: Record<string, DataRecordValue> }> = []
  const created: Array<Record<string, DataRecordValue>> = []
  table.forEach((cells, index) => {
    const values: Record<string, DataRecordValue> = {}
    cells.forEach((cell, col) => {
      const field = fields[col]
      if (writable(field)) values[field.name] = coerceValue(field, cell)
    })
    if (!Object.keys(values).length) return
    const record = targets[index]
    if (record) updates.push({ rowKey: record.row_key, values })
    else created.push(values)
  })
  const ops: TableOp[] = []
  if (updates.length) ops.push({ op: 'cells.set', tableId, records: updates } as TableOp)
  if (created.length) ops.push({ op: 'records.create', tableId, at: null, records: created } as TableOp)
  return ops
}

/** How many rows a pasted text has (so the paste reads that many records). */
export function pastedRowCount(text: string): number {
  return parsePastedTable(text).length
}

/** Clearing cells: each writable field of each record set to its empty value, as one `cells.set`. */
export function clearOps(tableId: number, records: GridRecord[], fields: DataField[]): TableOp[] {
  const writableFields = fields.filter(writable)
  if (!writableFields.length || !records.length) return []
  const empty = Object.fromEntries(writableFields.map((field) => [field.name, coerceValue(field, null)]))
  return [{ op: 'cells.set', tableId, records: records.map((record) => ({ rowKey: record.row_key, values: { ...empty } })) } as TableOp]
}

/**
 * The fill handle: `targets` (the records the handle was dragged over, nearest the selection
 * first) take the values of `sources` (the selected records, top to bottom) in a cycle, as a
 * spreadsheet fills: down repeats the selection from its top, up from its bottom.
 */
export function fillOps(tableId: number, sources: GridRecord[], targets: GridRecord[], fields: DataField[], down: boolean): TableOp[] {
  const writableFields = fields.filter(writable)
  if (!sources.length || !targets.length || !writableFields.length) return []
  const records = targets.map((target, step) => {
    const source = down ? sources[step % sources.length] : sources[sources.length - 1 - (step % sources.length)]
    // The values as they are stored (a number stays a number): a fill copies, it does not parse.
    const values = Object.fromEntries(writableFields.map((field) => [field.name, (source.values[field.name] ?? null) as DataRecordValue]))
    return { rowKey: target.row_key, values }
  })
  return [{ op: 'cells.set', tableId, records } as TableOp]
}

/** Moving a record before another (or to the end, `before` null). */
export function moveRecordOps(tableId: number, rowKey: string, before: string | null): TableOp[] {
  return [{ op: 'records.move', tableId, rowKeys: [rowKey], before } as TableOp]
}

/** Moving fields (in order) so that the first is the `toIndex`-th of the table's fields. */
export function moveFieldOps(tableId: number, fieldIds: number[], toIndex: number): TableOp[] {
  return [{ op: 'fields.move', tableId, fieldIds, toIndex } as TableOp]
}

/**
 * Where moved shown columns land among all of the table's fields: the shown columns are `shown`
 * (field ids, in order), all fields `all` (in order); the shown columns `left`..`right` are
 * dropped before shown index `drop` (`shown.length` for the end). Hidden fields keep their places.
 * Answers the first moved field's new index in `all` (`fields.move`'s `toIndex`), or null when
 * nothing moves.
 */
export function fieldDropIndex(all: number[], shown: number[], left: number, right: number, drop: number): number | null {
  if (left < 0 || right >= shown.length || left > right || drop < 0 || drop > shown.length) return null
  if (drop >= left && drop <= right + 1) return null
  const block = shown.slice(left, right + 1)
  const rest = shown.filter((_, index) => index < left || index > right)
  const at = drop < left ? drop : drop - block.length
  const moved = [...rest.slice(0, at), ...block, ...rest.slice(at)]
  const visible = new Set(shown)
  let next = 0
  const order = all.map((id) => (visible.has(id) ? moved[next++] : id))
  return order.indexOf(block[0])
}
