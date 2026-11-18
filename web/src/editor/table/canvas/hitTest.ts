/**
 * Where a point of the grid's viewport is, synchronously, without asking the worker: the axes give
 * the row and the column, the linear rows what the row is, and the hit records of the band under
 * the point (from the band cache) the clickable part there, as the engine laid it out (a checkbox,
 * a rating's symbol, a column's menu caret or edge, a row head control, a group's toggle). The page
 * never computes where a part is.
 *
 * Also the other way round: where a document rectangle (a cell, a range, a row head) shows on the
 * viewport. The row heads and the frozen columns do not scroll sideways and the header does not
 * scroll down, so one rectangle can show as up to four pieces, each clipped to its region.
 */
import type { Area, Axes } from './axes'
import { type BandKey, type DisplayList, type Hit, Layer } from './displayList'
import { type GridRows, RowKind } from './rows'
import type { CellRef } from './selection'

/** What the engine's hit records name (`HitKind` in kition-table-display). */
export enum HitKind {
  Checkbox = 0,
  Rating = 1,
  Attachment = 2,
  Button = 3,
  Link = 4,
  Expand = 5,
  DragHandle = 6,
  GroupToggle = 7,
  ColumnMenu = 8,
  ColumnEdge = 9,
  FreezeHandle = 10,
  AppendRow = 11,
  AppendColumn = 12,
}

/** What part of the grid a point is over. */
export type Region =
  /** A record's cell. */
  | 'cell'
  /** A record's row head (number and controls). */
  | 'rowHead'
  /** A column's header. */
  | 'header'
  /** The header above the row heads (the select-all checkbox). */
  | 'corner'
  /** A group header row. */
  | 'group'
  /** An add-record row. */
  | 'appendRow'
  /** The add-field column's header. */
  | 'appendColumn'
  | 'none'

export type GridHit = {
  region: Region
  /** The linear row, or -1 (the header, outside the rows). */
  row: number
  /** The shown column, or -1 (row heads, outside the columns). */
  col: number
  /** The engine's hit record under the point, or null. */
  part: Hit | null
  /** The point in document CSS pixels, and in the viewport's. */
  docX: number
  docY: number
  x: number
  y: number
}

/** The viewport over the document: the scroll position and the size, in CSS pixels. */
export type Viewport = { scrollX: number; scrollY: number; width: number; height: number }

/** What hit testing reads: the axes, the linear rows, the viewport and the cached bands. */
export type HitSource = {
  axes: Axes
  rows: GridRows | null
  viewport: Viewport
  list(key: BandKey): DisplayList | undefined
}

const NONE = (x: number, y: number): GridHit => ({ region: 'none', row: -1, col: -1, part: null, docX: x, docY: y, x, y })

/** How many columns hold fields (the axes end with the add-field column). */
export function fieldColumnCount(axes: Axes): number {
  const last = axes.columns[axes.columns.length - 1]
  return last && last.fieldId == null ? axes.columns.length - 1 : axes.columns.length
}

/** The document point under viewport point (`x`, `y`): the header does not scroll down, the row heads and frozen columns not sideways. */
export function toDocument(axes: Axes, viewport: Viewport, x: number, y: number): { docX: number; docY: number } {
  return { docX: x < axes.bodyLeft ? x : x + viewport.scrollX, docY: y < axes.headerHeight ? y : y + viewport.scrollY }
}

/** The band that draws document point (`docX`, `docY`) of `row`, for a point on the viewport's side of the header and the frozen edge. */
function bandAt(axes: Axes, header: boolean, x: number, docX: number, row: number): BandKey {
  const strip = Math.max(0, Math.floor((docX - axes.bodyLeft) / axes.bandWidth))
  if (header) return x < axes.bodyLeft ? { layer: Layer.HeaderFrozen, colBand: 0, rowBand: 0 } : { layer: Layer.Header, colBand: strip, rowBand: 0 }
  const rowBand = Math.floor(Math.max(0, row) / axes.bandRows)
  if (x < axes.rowHeadWidth) return { layer: Layer.RowHead, colBand: 0, rowBand }
  if (x < axes.bodyLeft) return { layer: Layer.Frozen, colBand: 0, rowBand }
  return { layer: Layer.Body, colBand: strip, rowBand }
}

/** The engine's hit record at a document point of a band, or null. */
export function partAt(list: DisplayList | undefined, docX: number, docY: number): Hit | null {
  if (!list) return null
  for (const hit of list.hits) if (docX >= hit.x && docX < hit.x + hit.w && docY >= hit.y && docY < hit.y + hit.h) return hit
  return null
}

/** What is at viewport point (`x`, `y`). */
export function hitTest(source: HitSource, x: number, y: number): GridHit {
  const { axes, rows, viewport } = source
  if (x < 0 || y < 0 || x >= viewport.width || y >= viewport.height) return NONE(x, y)
  const { docX, docY } = toDocument(axes, viewport, x, y)
  const header = y < axes.headerHeight
  const row = header ? -1 : axes.rowAt(docY)
  if (!header && (row < 0 || row >= axes.rows)) return { ...NONE(x, y), docX, docY }
  const part = partAt(source.list(bandAt(axes, header, x, docX, row)), docX, docY)
  const fields = fieldColumnCount(axes)
  const column = x < axes.rowHeadWidth ? -1 : axes.columnAt(docX)
  const col = column >= 0 && column < fields ? column : -1
  const base = { row, col, part, docX, docY, x, y }
  if (header) {
    if (x < axes.rowHeadWidth) return { ...base, region: 'corner' }
    if (column >= fields) return { ...base, region: 'appendColumn' }
    return { ...base, region: col >= 0 ? 'header' : 'none' }
  }
  const kind = rows ? rows.kind(row) : RowKind.Record
  if (kind === RowKind.Group) return { ...base, region: 'group' }
  if (kind === RowKind.Append) return { ...base, region: 'appendRow' }
  if (x < axes.rowHeadWidth) return { ...base, region: 'rowHead' }
  return { ...base, region: col >= 0 ? 'cell' : 'none' }
}

/**
 * The cell nearest viewport point (`x`, `y`) among the rows and field columns, for a drag that
 * left the cells (over the header, the row heads, or outside the viewport): the point is first
 * held inside the scrolling cells.
 */
export function cellNear(axes: Axes, viewport: Viewport, rows: GridRows | null, x: number, y: number): CellRef | null {
  const fields = fieldColumnCount(axes)
  if (axes.rows === 0 || fields === 0) return null
  const cy = Math.min(Math.max(y, axes.headerHeight), viewport.height - 1)
  const cx = Math.min(Math.max(x, axes.rowHeadWidth), viewport.width - 1)
  const { docX, docY } = toDocument(axes, viewport, cx, cy)
  let row = Math.min(axes.rows - 1, Math.max(0, axes.rowAt(docY)))
  if (rows && !rows.isRecord(row)) {
    const near = rows.recordFrom(row, -1)
    row = near >= 0 ? near : rows.recordFrom(row, 1)
    if (row < 0) return null
  }
  let col = axes.columnAt(docX)
  if (col < 0 || col >= fields) col = docX < axes.rowHeadWidth ? 0 : fields - 1
  return { row, col }
}

/** A cell's document rectangle (its lines included). */
export function cellArea(axes: Axes, cell: CellRef): Area {
  const column = axes.columns[cell.col]
  return { x: column?.x ?? 0, y: axes.rowTop(cell.row), width: column?.width ?? 0, height: axes.rowHeight(cell.row) }
}

/** The document rectangle of cells `top`..`bottom` rows and `left`..`right` columns (inclusive). */
export function rangeArea(axes: Axes, top: number, bottom: number, left: number, right: number): Area {
  const a = axes.columns[left]
  const b = axes.columns[right]
  const x = a?.x ?? axes.rowHeadWidth
  const y = axes.rowTop(top)
  return { x, y, width: (b ? b.x + b.width : x) - x, height: axes.rowTop(bottom + 1) - y }
}

function clip(area: Area, bounds: Area): Area | null {
  const x = Math.max(area.x, bounds.x)
  const y = Math.max(area.y, bounds.y)
  const right = Math.min(area.x + area.width, bounds.x + bounds.width)
  const bottom = Math.min(area.y + area.height, bounds.y + bounds.height)
  return right > x && bottom > y ? { x, y, width: right - x, height: bottom - y } : null
}

/**
 * Where a document rectangle shows on the viewport: up to four pieces (above or below the header's
 * bottom, left or right of the frozen edge), each moved by the scroll its region has and clipped to
 * the region and the viewport. Empty when it is scrolled out of view.
 */
export function toScreen(axes: Axes, viewport: Viewport, area: Area): Area[] {
  const out: Area[] = []
  const { width, height, scrollX, scrollY } = viewport
  const sides = [
    { from: 0, to: axes.bodyLeft, dx: 0, screen: { x: 0, width: Math.min(axes.bodyLeft, width) } },
    { from: axes.bodyLeft, to: Number.POSITIVE_INFINITY, dx: scrollX, screen: { x: axes.bodyLeft, width: Math.max(0, width - axes.bodyLeft) } },
  ]
  const bands = [
    { from: 0, to: axes.headerHeight, dy: 0, screen: { y: 0, height: Math.min(axes.headerHeight, height) } },
    { from: axes.headerHeight, to: Number.POSITIVE_INFINITY, dy: scrollY, screen: { y: axes.headerHeight, height: Math.max(0, height - axes.headerHeight) } },
  ]
  for (const band of bands)
    for (const side of sides) {
      const part = clip(area, { x: side.from, y: band.from, width: side.to - side.from, height: band.to - band.from })
      if (!part) continue
      const moved = { x: part.x - side.dx, y: part.y - band.dy, width: part.width, height: part.height }
      const shown = clip(moved, { x: side.screen.x, y: band.screen.y, width: side.screen.width, height: band.screen.height })
      if (shown) out.push(shown)
    }
  return out
}

/** The hit records of `kind` in a row's row head band (the engine's row head controls), left to right. */
export function rowHeadParts(source: HitSource, row: number): Hit[] {
  const { axes } = source
  const list = source.list({ layer: Layer.RowHead, colBand: 0, rowBand: Math.floor(row / axes.bandRows) })
  if (!list) return []
  return list.hits.filter((hit) => hit.row === row && hit.col === -1 && hit.kind !== HitKind.AppendRow).sort((a, b) => a.x - b.x)
}

/** The header's select-all checkbox (the engine's hit record in the corner), or null. */
export function cornerCheckbox(source: HitSource): Hit | null {
  const list = source.list({ layer: Layer.HeaderFrozen, colBand: 0, rowBand: 0 })
  return list?.hits.find((hit) => hit.kind === HitKind.Checkbox && hit.row === -1 && hit.col === -1) ?? null
}
