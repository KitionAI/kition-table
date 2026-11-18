/**
 * The grid's selection on the page (editor state: never in the engine's pictures). Cells are named
 * by their linear row (the axes' rows: records, group headers and add rows in display order) and
 * their shown column (the axes' columns without the add-field column). Only record rows hold cells:
 * what a selection covers on other rows is drawn but never acted on.
 *
 * - `cells`: a range from `anchor` (where it started) to `focus` (where it ends; Shift moves it).
 * - `rows`: whole records, as inclusive ranges of linear rows (checkboxes, Shift for a range).
 * - `columns`: whole columns from `anchor` to `focus`.
 *
 * Every function here is pure; `GridInteraction` (interaction.ts) holds the state.
 */
import type { GridRows } from './rows'

/** A cell: its linear row and shown column. */
export type CellRef = { row: number; col: number }

export type GridSelection =
  | { kind: 'none' }
  | { kind: 'cells'; anchor: CellRef; focus: CellRef }
  | { kind: 'rows'; ranges: Array<[number, number]> }
  | { kind: 'columns'; anchor: number; focus: number }

export const NO_SELECTION: GridSelection = { kind: 'none' }

/** Inclusive bounds of a cell range. */
export type CellBounds = { top: number; bottom: number; left: number; right: number }

export function cellSelection(anchor: CellRef, focus: CellRef = anchor): GridSelection {
  return { kind: 'cells', anchor: { ...anchor }, focus: { ...focus } }
}

export function boundsOf(anchor: CellRef, focus: CellRef): CellBounds {
  return {
    top: Math.min(anchor.row, focus.row),
    bottom: Math.max(anchor.row, focus.row),
    left: Math.min(anchor.col, focus.col),
    right: Math.max(anchor.col, focus.col),
  }
}

export function sameCell(a: CellRef | null, b: CellRef | null): boolean {
  return a === b || (a != null && b != null && a.row === b.row && a.col === b.col)
}

export function sameSelection(a: GridSelection, b: GridSelection): boolean {
  if (a.kind !== b.kind) return false
  switch (a.kind) {
    case 'none':
      return true
    case 'cells': {
      const other = b as typeof a
      return sameCell(a.anchor, other.anchor) && sameCell(a.focus, other.focus)
    }
    case 'rows': {
      const other = b as typeof a
      return a.ranges.length === other.ranges.length && a.ranges.every(([x, y], i) => other.ranges[i][0] === x && other.ranges[i][1] === y)
    }
    case 'columns': {
      const other = b as typeof a
      return a.anchor === other.anchor && a.focus === other.focus
    }
  }
}

/** Sorted, merged ranges (adjacent ones joined). */
export function normalizeRanges(ranges: Array<[number, number]>): Array<[number, number]> {
  const sorted = ranges.map(([a, b]): [number, number] => [Math.min(a, b), Math.max(a, b)]).sort((x, y) => x[0] - y[0])
  const out: Array<[number, number]> = []
  for (const range of sorted) {
    const last = out[out.length - 1]
    if (last && range[0] <= last[1] + 1) last[1] = Math.max(last[1], range[1])
    else out.push([range[0], range[1]])
  }
  return out
}

/** Whether linear row `row` is a selected whole row. */
export function isRowSelected(selection: GridSelection, row: number): boolean {
  return selection.kind === 'rows' && selection.ranges.some(([from, to]) => row >= from && row <= to)
}

/** Whether shown column `col` is a selected whole column. */
export function isColumnSelected(selection: GridSelection, col: number): boolean {
  return selection.kind === 'columns' && col >= Math.min(selection.anchor, selection.focus) && col <= Math.max(selection.anchor, selection.focus)
}

/** Whether every record row is selected as a whole row (the header's checkbox shows checked). */
export function allRowsSelected(selection: GridSelection, rows: GridRows): boolean {
  if (selection.kind !== 'rows' || rows.firstRecord < 0) return false
  for (let row = 0; row < rows.count; row += 1) if (rows.isRecord(row) && !isRowSelected(selection, row)) return false
  return true
}

/** Every record row as one whole-row selection (the header's checkbox). */
export function selectAllRows(rows: GridRows): GridSelection {
  return rows.firstRecord < 0 ? NO_SELECTION : { kind: 'rows', ranges: [[rows.firstRecord, rows.lastRecord]] }
}

/** Every cell (Cmd/Ctrl+A). */
export function selectAllCells(rows: GridRows, columns: number): GridSelection {
  if (rows.firstRecord < 0 || columns <= 0) return NO_SELECTION
  return cellSelection({ row: rows.firstRecord, col: 0 }, { row: rows.lastRecord, col: columns - 1 })
}

/**
 * A row's checkbox clicked: the row joins or leaves the whole-row selection; with `extendFrom`
 * (Shift, the row clicked before) every record row between the two joins.
 */
export function toggleRow(selection: GridSelection, row: number, extendFrom: number | null = null): GridSelection {
  const ranges = selection.kind === 'rows' ? selection.ranges.map(([a, b]): [number, number] => [a, b]) : []
  if (extendFrom != null) return { kind: 'rows', ranges: normalizeRanges([...ranges, [extendFrom, row]]) }
  if (!isRowSelected(selection, row)) return { kind: 'rows', ranges: normalizeRanges([...ranges, [row, row]]) }
  const out: Array<[number, number]> = []
  for (const [from, to] of ranges) {
    if (row < from || row > to) out.push([from, to])
    else {
      if (from < row) out.push([from, row - 1])
      if (row < to) out.push([row + 1, to])
    }
  }
  return out.length ? { kind: 'rows', ranges: out } : NO_SELECTION
}

/** The record ids a selection acts on, in display order (rows that are not records are left out). */
export function selectedRecordIds(selection: GridSelection, rows: GridRows): number[] {
  const ids: number[] = []
  const take = (from: number, to: number) => {
    for (let row = Math.max(0, from); row <= Math.min(rows.count - 1, to); row += 1) {
      const id = rows.recordId(row)
      if (id != null) ids.push(id)
    }
  }
  switch (selection.kind) {
    case 'cells': {
      const b = boundsOf(selection.anchor, selection.focus)
      take(b.top, b.bottom)
      break
    }
    case 'rows':
      for (const [from, to] of normalizeRanges(selection.ranges)) take(from, to)
      break
    case 'columns':
      take(0, rows.count - 1)
      break
    case 'none':
      break
  }
  return ids
}

/** The shown columns a selection acts on, left to right. */
export function selectedColumns(selection: GridSelection, columns: number): number[] {
  const range = (from: number, to: number) => Array.from({ length: Math.max(0, to - from + 1) }, (_, i) => from + i)
  switch (selection.kind) {
    case 'cells': {
      const b = boundsOf(selection.anchor, selection.focus)
      return range(Math.max(0, b.left), Math.min(columns - 1, b.right))
    }
    case 'rows':
      return range(0, columns - 1)
    case 'columns':
      return range(Math.max(0, Math.min(selection.anchor, selection.focus)), Math.min(columns - 1, Math.max(selection.anchor, selection.focus)))
    case 'none':
      return []
  }
}

/**
 * Where the keyboard moves a cell: `dx`/`dy` cells (rows that are not records are stepped over),
 * or with `jump` to the first or last record or column in that direction. Stays put at an edge.
 */
export function moveCell(rows: GridRows, columns: number, cell: CellRef, dx: number, dy: number, jump = false): CellRef {
  let { row, col } = cell
  if (dx !== 0) col = jump ? (dx < 0 ? 0 : columns - 1) : Math.min(columns - 1, Math.max(0, col + dx))
  if (dy !== 0) {
    if (jump) row = dy < 0 ? rows.firstRecord : rows.lastRecord
    else {
      const step = dy < 0 ? -1 : 1
      let at = row
      for (let n = 0; n < Math.abs(dy); n += 1) {
        const next = rows.recordFrom(at + step, step)
        if (next < 0) break
        at = next
      }
      row = at
    }
  }
  return row < 0 ? cell : { row, col }
}
