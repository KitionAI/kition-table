// Editing on the new grid, the page's pure parts (case table.editor.grid/editing): the cells a
// selection covers, the clipboard text, the ops of a paste, a clear, a fill and the moves, where a
// moved column lands among hidden fields, the drags' geometry (resize, column and row drops, the
// freeze count, the fill span), and what an editor starts from.
import { describe, expect, it } from 'vitest'
import type { DataField } from '../../types/dataDocument'
import { Axes, type AxesInput } from './axes'
import { clearOps, coveredCells, fieldDropIndex, fillOps, moveFieldOps, moveRecordOps, pasteOps, recordsFrom, serializeCells, writable } from './editOps'
import { columnDrop, fillSpan, freezeCount, freezeEdge, freezeGuide, MIN_COLUMN_WIDTH, resizedWidth, rowDrop } from './gestures'
import type { GridRecord } from './gridData'
import { aiButtonArea, editText } from './gridEditing'
import { GridRows, RowKind } from './rows'
import { cellSelection } from './selection'

const field = (id: number, name: string, type: DataField['type'], extra: Partial<DataField> = {}): DataField =>
  ({ id, name, title: name, type, readonly: false, is_primary: false, order: id, required: false, unique: false, user_id: 0, document_id: 0, table_id: 1, created_at: '', updated_at: '', ...extra }) as DataField

const TITLE = field(1, 'title', 'text', { is_primary: true })
const AMOUNT = field(2, 'amount', 'number')
const DUE = field(3, 'due', 'datetime')
const TAGS = field(4, 'tags', 'multi_select')
const DONE = field(5, 'done', 'checkbox')
const FORMULA = field(6, 'total', 'formula')
const LOCKED = field(7, 'locked', 'text', { readonly: true })

const record = (id: number, values: GridRecord['values']): GridRecord => ({ id, row_key: `r${id}`, values })

// A group header, three records, the add row; a group header, two records, the add row.
const ROWS = GridRows.of(1, [
  [RowKind.Group, 0],
  [RowKind.Record, 11],
  [RowKind.Record, 12],
  [RowKind.Record, 13],
  [RowKind.Append, 13],
  [RowKind.Group, 1],
  [RowKind.Record, 14],
  [RowKind.Record, 15],
  [RowKind.Append, 15],
])

describe('what a selection covers', () => {
  it('names the records of a cell range in display order, skipping group headers and add rows', () => {
    expect(coveredCells(cellSelection({ row: 2, col: 1 }, { row: 6, col: 0 }), ROWS, 4)).toEqual({ recordIds: [12, 13, 14], cols: [0, 1] })
  })
  it('covers every column of checked rows, and every record of selected columns', () => {
    expect(coveredCells({ kind: 'rows', ranges: [[7, 7], [1, 1]] }, ROWS, 3)).toEqual({ recordIds: [11, 15], cols: [0, 1, 2] })
    expect(coveredCells({ kind: 'columns', anchor: 2, focus: 1 }, ROWS, 3)).toEqual({ recordIds: [11, 12, 13, 14, 15], cols: [1, 2] })
    expect(coveredCells({ kind: 'none' }, ROWS, 3)).toEqual({ recordIds: [], cols: [] })
  })
  it('walks the records from a row on, over other rows', () => {
    expect(recordsFrom(ROWS, 3, 3)).toEqual([13, 14, 15])
    expect(recordsFrom(ROWS, 7, 5)).toEqual([15])
    expect(recordsFrom(ROWS, 8, 2)).toEqual([])
  })
})

describe('the clipboard', () => {
  it('copies cells as tab-separated text, dates formatted, awkward cells quoted', () => {
    const records = [record(1, { title: 'Plain', amount: 5, due: null }), record(2, { title: 'Tab\there "q"', amount: null, due: null })]
    expect(serializeCells(records, [TITLE, AMOUNT])).toBe('Plain\t5\n"Tab\there ""q"""\t')
    const date = new Date('2026-07-23T04:14:47Z')
    const pad = (value: number) => String(value).padStart(2, '0')
    const shown = `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
    expect(serializeCells([record(3, { due: '2026-07-23T04:14:47Z' })], [DUE])).toBe(shown)
  })

  it('pastes over the records there are and creates the rest, in one cells.set and one records.create', () => {
    const ops = pasteOps(1, 'a\t1\tx\nb\t2\ty\nc\t3\tz', [record(11, {}), record(12, {})], [TITLE, AMOUNT, TAGS])
    expect(ops).toEqual([
      { op: 'cells.set', tableId: 1, records: [{ rowKey: 'r11', values: { title: 'a', amount: 1, tags: ['x'] } }, { rowKey: 'r12', values: { title: 'b', amount: 2, tags: ['y'] } }] },
      { op: 'records.create', tableId: 1, at: null, records: [{ title: 'c', amount: 3, tags: ['z'] }] },
    ])
  })

  it('skips read-only and computed fields, and cells past the last field', () => {
    expect(pasteOps(1, 'a\tb\tc', [record(11, {})], [LOCKED, FORMULA])).toEqual([])
    expect(pasteOps(1, 'a\tb\tc', [record(11, {})], [TITLE])).toEqual([{ op: 'cells.set', tableId: 1, records: [{ rowKey: 'r11', values: { title: 'a' } }] }])
    expect(writable(TITLE) && !writable(FORMULA) && !writable(LOCKED) && !writable(undefined)).toBe(true)
  })
})

describe('clearing and filling', () => {
  it('clears the writable fields to their empty values in one cells.set', () => {
    expect(clearOps(1, [record(11, {}), record(12, {})], [TITLE, DONE, TAGS, FORMULA])).toEqual([
      {
        op: 'cells.set',
        tableId: 1,
        records: [
          { rowKey: 'r11', values: { title: null, done: false, tags: [] } },
          { rowKey: 'r12', values: { title: null, done: false, tags: [] } },
        ],
      },
    ])
    expect(clearOps(1, [record(11, {})], [FORMULA])).toEqual([])
  })

  it('fills down by repeating the selection from its top, up from its bottom, values as stored', () => {
    const sources = [record(1, { amount: 10, title: 'a' }), record(2, { amount: 20, title: 'b' })]
    const targets = [record(3, {}), record(4, {}), record(5, {})]
    expect(fillOps(1, sources, targets, [AMOUNT], true)).toEqual([
      { op: 'cells.set', tableId: 1, records: [{ rowKey: 'r3', values: { amount: 10 } }, { rowKey: 'r4', values: { amount: 20 } }, { rowKey: 'r5', values: { amount: 10 } }] },
    ])
    expect(fillOps(1, sources, targets, [AMOUNT, FORMULA], false)).toEqual([
      { op: 'cells.set', tableId: 1, records: [{ rowKey: 'r3', values: { amount: 20 } }, { rowKey: 'r4', values: { amount: 10 } }, { rowKey: 'r5', values: { amount: 20 } }] },
    ])
  })
})

describe('moves', () => {
  it('moves a record before another, or to the end', () => {
    expect(moveRecordOps(1, 'r3', 'r1')).toEqual([{ op: 'records.move', tableId: 1, rowKeys: ['r3'], before: 'r1' }])
    expect(moveRecordOps(1, 'r3', null)).toEqual([{ op: 'records.move', tableId: 1, rowKeys: ['r3'], before: null }])
    expect(moveFieldOps(1, [4], 0)).toEqual([{ op: 'fields.move', tableId: 1, fieldIds: [4], toIndex: 0 }])
  })

  it('places moved columns among the hidden fields, which keep their places', () => {
    // Fields 1..6; 3 and 5 hidden: the grid shows 1, 2, 4, 6.
    const all = [1, 2, 3, 4, 5, 6]
    const shown = [1, 2, 4, 6]
    expect(fieldDropIndex(all, shown, 1, 1, 0)).toBe(0)
    expect(fieldDropIndex(all, shown, 0, 0, 4)).toBe(5)
    expect(fieldDropIndex(all, shown, 2, 3, 1)).toBe(1)
    // Dropped where it is: nothing moves.
    expect(fieldDropIndex(all, shown, 1, 1, 1)).toBeNull()
    expect(fieldDropIndex(all, shown, 1, 1, 2)).toBeNull()
  })
})

// The row heads (72), two frozen columns of 100, two of 200, the add-field column.
const INPUT: AxesInput = {
  epoch: 1,
  rowRuns: [[40, 9]],
  columns: [
    { x: 72, width: 100, frozen: true, fieldId: 1 },
    { x: 172, width: 100, frozen: true, fieldId: 2 },
    { x: 272, width: 200, frozen: false, fieldId: 3 },
    { x: 472, width: 200, frozen: false, fieldId: 4 },
    { x: 672, width: 100, frozen: false, fieldId: null },
  ],
  headerHeight: 32,
  rowHeadWidth: 72,
  bandRows: 32,
  bandWidth: 2048,
}

describe("the drags' geometry", () => {
  const axes = new Axes(INPUT)
  const viewport = { scrollX: 0, scrollY: 0, width: 800, height: 400 }

  it('resizes by the pointer, never below the narrowest column', () => {
    expect(resizedWidth(200, 80.4)).toBe(280)
    expect(resizedWidth(200, -500)).toBe(MIN_COLUMN_WIDTH)
  })

  it('drops a column before the column whose middle is right of the pointer', () => {
    expect(columnDrop(axes, 80)).toBe(0)
    expect(columnDrop(axes, 130)).toBe(1)
    expect(columnDrop(axes, 380)).toBe(3)
    expect(columnDrop(axes, 700)).toBe(4)
  })

  it('drops a row in the place of the record under the pointer', () => {
    expect(rowDrop(ROWS, 3, 1)).toEqual({ target: 1, after: false })
    expect(rowDrop(ROWS, 1, 6)).toEqual({ target: 6, after: true })
    // Over a group header above the source: the record below it.
    expect(rowDrop(ROWS, 7, 5)).toEqual({ target: 6, after: false })
    expect(rowDrop(ROWS, 2, 2)).toBeNull()
  })

  it('freezes the leading columns whose middle is left of the pointer', () => {
    expect(freezeCount(axes, viewport, 100)).toBe(0)
    expect(freezeCount(axes, viewport, 130)).toBe(1)
    expect(freezeCount(axes, viewport, 400)).toBe(3)
    // Scrolled sideways, the scrolling columns are where they show.
    expect(freezeCount(axes, { ...viewport, scrollX: 150 }, 280)).toBe(3)
    expect(freezeEdge(axes, viewport, 0)).toBe(72)
    expect(freezeEdge(axes, viewport, 2)).toBe(272)
    expect(freezeEdge(axes, { ...viewport, scrollX: 150 }, 3)).toBe(322)
  })

  it('runs the freeze guide from the header to the last row, never shorter than the header', () => {
    // Nine rows of 40 below a 32 px header end at 392, above the viewport's bottom.
    expect(freezeGuide(axes, viewport)).toEqual({ top: 32, bottom: 392 })
    expect(freezeGuide(axes, { ...viewport, height: 300 })).toEqual({ top: 32, bottom: 300 })
    expect(freezeGuide(axes, { ...viewport, scrollY: 100 })).toEqual({ top: 32, bottom: 292 })
    expect(freezeGuide(axes, { ...viewport, scrollY: 380 })).toEqual({ top: 32, bottom: 32 })
  })

  it('fills the rows beyond the range the handle was dragged over', () => {
    const bounds = { top: 2, bottom: 3, left: 0, right: 1 }
    expect(fillSpan(bounds, 6)).toEqual({ down: true, from: 4, to: 6 })
    expect(fillSpan(bounds, 0)).toEqual({ down: false, from: 1, to: 0 })
    expect(fillSpan(bounds, 3)).toBeNull()
  })
})

describe('editors', () => {
  it("starts a text editor from the value as typed, a link's address", () => {
    expect(editText(TITLE, 'Hello')).toBe('Hello')
    expect(editText(AMOUNT, 42)).toBe('42')
    expect(editText(TITLE, null)).toBe('')
    expect(editText(field(9, 'site', 'url'), { href: 'https://example.com', label: 'Example' } as never)).toBe('https://example.com')
  })

  it("puts an AI cell's regenerate button in its top right corner", () => {
    expect(aiButtonArea({ x: 100, y: 40, width: 200, height: 40 })).toEqual({ x: 278, y: 44, width: 18, height: 18 })
  })
})
