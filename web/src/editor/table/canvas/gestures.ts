/**
 * The grid's drags (grid plan F3, F11, F12, F13), as gestures of `GridInteraction`: resizing a
 * column at its header's right edge, dragging a selected column to another place, dragging a row
 * by its handle (only when the view neither sorts nor groups: the engine then lays out the
 * handle), dragging the frozen columns' edge, and the fill handle at the corner of a cell range.
 * Each draws its feedback on the overlay (a line, a shadow, the handle) from the engine's axes and
 * style, and hands the result to the editor when the pointer is released; nothing is written
 * while the pointer moves.
 *
 * The geometry is pure and exported for the tests: where a column is dropped, where a row lands,
 * how many columns a freeze drag freezes, which rows a fill covers.
 */
import type { Area, Axes } from './axes'
import { Opcode } from './displayList'
import { cellArea, cellNear, fieldColumnCount, type GridHit, HitKind, rangeArea, toScreen, type Viewport } from './hitTest'
import type { GridInteraction, OverlayFrame } from './interaction'
import { overlayList } from './overlay'
import type { GridRows } from './rows'
import { boundsOf, type CellBounds, type GridSelection } from './selection'

/** The narrowest a column gets (the editor's `MIN_COLUMN_WIDTH`). */
export const MIN_COLUMN_WIDTH = 100
/** How far either side of the frozen edge a press grabs it. */
export const FREEZE_GRAB = 4
/** The fill handle's side, and how far around it a press grabs it. */
export const FILL_HANDLE = 8
export const FILL_GRAB = 4
/** How far the pointer moves before a press on a selected column becomes a drag. */
const DRAG_START = 4

/** A column resized by `dx` from `width`. */
export function resizedWidth(width: number, dx: number): number {
  return Math.max(MIN_COLUMN_WIDTH, Math.round(width + dx))
}

/** Where a column dragged to document `docX` drops: before shown column 0..count (count = after the last). */
export function columnDrop(axes: Axes, docX: number): number {
  const count = fieldColumnCount(axes)
  for (let col = 0; col < count; col += 1) {
    const column = axes.columns[col]
    if (docX < column.x + column.width / 2) return col
  }
  return count
}

/**
 * Where a row dragged from `source` over row `over` lands: in the place of the record row nearest
 * `over`, so before it when it is above the source and after it when below. Null when it would not
 * move.
 */
export function rowDrop(rows: GridRows, source: number, over: number): { target: number; after: boolean } | null {
  let target = rows.isRecord(over) ? over : rows.recordFrom(over, over < source ? 1 : -1)
  if (target < 0) target = rows.recordFrom(over, over < source ? -1 : 1)
  if (target < 0 || target === source) return null
  return { target, after: target > source }
}

/** How many columns a freeze drag at viewport `x` freezes: the leading columns whose middle is left of the pointer. */
export function freezeCount(axes: Axes, viewport: Viewport, x: number): number {
  const count = fieldColumnCount(axes)
  let frozen = 0
  for (let col = 0; col < count; col += 1) {
    const column = axes.columns[col]
    const left = column.frozen ? column.x : column.x - viewport.scrollX
    if (left + column.width / 2 > x) break
    frozen = col + 1
  }
  return frozen
}

/** Where the right edge of the first `count` columns shows on the viewport (the row heads' edge for none). */
export function freezeEdge(axes: Axes, viewport: Viewport, count: number): number {
  const column = axes.columns[count - 1]
  if (!column) return axes.rowHeadWidth
  return column.x + column.width - (column.frozen ? 0 : viewport.scrollX)
}

/** The rows a fill handle dragged to `row` covers beyond the range: down or up, from the range's edge out. */
export function fillSpan(bounds: CellBounds, row: number): { down: boolean; from: number; to: number } | null {
  if (row > bounds.bottom) return { down: true, from: bounds.bottom + 1, to: row }
  if (row < bounds.top) return { down: false, from: bounds.top - 1, to: row }
  return null
}

/** The cell range the fill handle belongs to: a cell range, or the active cell. */
export function fillBounds(selection: GridSelection, active: { row: number; col: number } | null): CellBounds | null {
  if (selection.kind !== 'cells') return null
  if (!active) return null
  return boundsOf(selection.anchor, selection.focus)
}

/** The viewport rectangle a document area shows as (the union of its pieces), or null. */
export function screenUnion(axes: Axes, viewport: Viewport, area: Area): Area | null {
  const parts = toScreen(axes, viewport, area)
  if (!parts.length) return null
  const left = Math.min(...parts.map((part) => part.x))
  const top = Math.min(...parts.map((part) => part.y))
  const right = Math.max(...parts.map((part) => part.x + part.width))
  const bottom = Math.max(...parts.map((part) => part.y + part.height))
  return { x: left, y: top, width: right - left, height: bottom - top }
}

/** What the drags hand over when the pointer is released. */
export type DragActions = {
  /** Column `col` (its field `fieldId`) resized to `width`. */
  resize(col: number, fieldId: number, width: number): void
  /** Shown columns `cols` (in order) dropped before shown column `drop`. */
  moveColumns(cols: number[], drop: number): void
  /** The record of row `source` moved to the place of the record of row `target` (after it when `after`). */
  moveRow(source: number, target: number, after: boolean): void
  /** The view freezes `count` columns. */
  freeze(count: number): void
  /** The cell range `bounds` filled over rows `from`..`to` (`down`: below the range). */
  fill(bounds: CellBounds, span: { down: boolean; from: number; to: number }): void
  /** Whether the view lets rows be dragged (no sort, no groups). */
  canMoveRows(): boolean
}

type Paint = (frame: OverlayFrame) => void

/** Installs the drags on `interaction`; answers how to remove them. */
export function installDrags(interaction: GridInteraction, actions: DragActions): () => void {
  const view = interaction.view
  let paint: Paint | null = null
  const set = (next: Paint | null) => {
    paint = next
    interaction.invalidateOverlay()
  }
  const removers: Array<() => void> = []
  removers.push(interaction.addPainter((frame) => paint?.(frame), 200))
  const axes = () => view.currentAxes
  const line = (frame: OverlayFrame, x: number, y: number, width: number, height: number, alpha = false) => {
    const colors = [frame.style.border, frame.style.selection]
    frame.play(overlayList(colors, [{ op: Opcode.Rect, x, y, w: width, h: height, paint: alpha ? 1 : 0, snap: true }]))
  }

  // ---- resize -----------------------------------------------------------------------------
  let resize: { col: number; fieldId: number; width: number; startX: number; now: number } | null = null
  removers.push(
    interaction.addGesture({
      down(hit, event) {
        const a = axes()
        if (hit.region !== 'header' || hit.part?.kind !== HitKind.ColumnEdge || !a) return false
        const column = a.columns[hit.part.col]
        if (!column || column.fieldId == null) return false
        resize = { col: hit.part.col, fieldId: column.fieldId, width: column.width, startX: event.clientX, now: column.width }
        set((frame) => {
          if (!resize) return
          const at = frame.axes.columns[resize.col]
          line(frame, at.x + resize.now - 1, 0, 2, Math.min(frame.axes.height, frame.viewport.scrollY + frame.viewport.height))
        })
        return true
      },
      move(_hit, event) {
        if (!resize) return
        const width = resizedWidth(resize.width, event.clientX - resize.startX)
        if (width !== resize.now) {
          resize.now = width
          interaction.invalidateOverlay()
        }
      },
      up() {
        const done = resize
        resize = null
        set(null)
        if (done && done.now !== done.width) actions.resize(done.col, done.fieldId, done.now)
      },
      cancel() {
        resize = null
        set(null)
      },
      cursor: (hit) => (hit.region === 'header' && hit.part?.kind === HitKind.ColumnEdge ? 'ew-resize' : null),
    }),
  )

  // ---- column drag --------------------------------------------------------------------------
  let columns: { cols: number[]; startX: number; grabX: number; dragging: boolean; docX: number } | null = null
  removers.push(
    interaction.addGesture({
      down(hit, event) {
        const selection = interaction.selection
        if (hit.region !== 'header' || hit.col < 0 || event.shiftKey || selection.kind !== 'columns') return false
        if (hit.part?.kind === HitKind.ColumnMenu || hit.part?.kind === HitKind.ColumnEdge) return false
        const left = Math.min(selection.anchor, selection.focus)
        const right = Math.max(selection.anchor, selection.focus)
        if (hit.col < left || hit.col > right) return false
        const a = axes()
        if (!a) return false
        const cols = Array.from({ length: right - left + 1 }, (_, index) => left + index)
        columns = { cols, startX: event.clientX, grabX: hit.docX - a.columns[left].x, dragging: false, docX: hit.docX }
        set((frame) => {
          if (!columns?.dragging) return
          const first = frame.axes.columns[columns.cols[0]]
          const last = frame.axes.columns[columns.cols[columns.cols.length - 1]]
          const width = last.x + last.width - first.x
          const height = Math.min(frame.axes.height, frame.viewport.scrollY + frame.viewport.height)
          line(frame, columns.docX - columns.grabX, 0, width, height, true)
          const count = fieldColumnCount(frame.axes)
          const drop = columnDrop(frame.axes, columns.docX)
          const end = frame.axes.columns[count - 1]
          const edge = drop < count ? frame.axes.columns[drop].x : end.x + end.width
          line(frame, edge - 1, 0, 2, height)
        })
        return true
      },
      move(hit, event) {
        if (!columns) return
        if (!columns.dragging && Math.abs(event.clientX - columns.startX) < DRAG_START) return
        columns.dragging = true
        columns.docX = hit.docX
        interaction.invalidateOverlay()
      },
      up(hit) {
        const done = columns
        columns = null
        set(null)
        const a = axes()
        if (!done || !a) return
        // A click (no drag) on one column of several selected selects that column alone.
        if (!done.dragging) {
          if (hit.col >= 0) interaction.setSelection({ kind: 'columns', anchor: hit.col, focus: hit.col }, interaction.active)
          return
        }
        actions.moveColumns(done.cols, columnDrop(a, hit.docX))
      },
      cancel() {
        columns = null
        set(null)
      },
    }),
  )

  // ---- row drag -----------------------------------------------------------------------------
  let row: { source: number; over: number; docY: number; grabY: number } | null = null
  removers.push(
    interaction.addGesture({
      down(hit) {
        const a = axes()
        if (hit.region !== 'rowHead' || hit.part?.kind !== HitKind.DragHandle || !a || !actions.canMoveRows()) return false
        row = { source: hit.row, over: hit.row, docY: hit.docY, grabY: hit.docY - a.rowTop(hit.row) }
        set((frame) => {
          const rows = frame.rows
          if (!row || !rows) return
          const fields = fieldColumnCount(frame.axes)
          const last = frame.axes.columns[fields - 1]
          const right = last ? last.x + last.width : frame.axes.bodyLeft
          const height = frame.axes.rowHeight(row.source)
          line(frame, 0, row.docY - row.grabY, right, height, true)
          const drop = rowDrop(rows, row.source, row.over)
          if (drop) {
            const y = drop.after ? frame.axes.rowTop(drop.target + 1) : frame.axes.rowTop(drop.target)
            line(frame, 0, y - 1, right, 2)
          }
        })
        return true
      },
      move(hit) {
        const a = axes()
        if (!row || !a) return
        const near = cellNear(a, view.viewport, interaction.rows, Math.max(hit.x, a.rowHeadWidth), hit.y)
        row.docY = hit.docY
        if (near) row.over = near.row
        interaction.invalidateOverlay()
      },
      up() {
        const done = row
        row = null
        set(null)
        const rows = interaction.rows
        if (!done || !rows) return
        const drop = rowDrop(rows, done.source, done.over)
        if (drop) actions.moveRow(done.source, drop.target, drop.after)
      },
      cancel() {
        row = null
        set(null)
      },
      cursor: (hit) => (hit.region === 'rowHead' && hit.part?.kind === HitKind.DragHandle && actions.canMoveRows() ? 'grab' : null),
    }),
  )

  // ---- freeze -------------------------------------------------------------------------------
  let freeze: { x: number; y: number } | null = null
  const onFreezeEdge = (hit: GridHit) => {
    const a = axes()
    if (!a || hit.y < a.headerHeight || hit.y >= Math.min(view.viewport.height, a.height - view.viewport.scrollY)) return false
    return Math.abs(hit.x - a.bodyLeft) <= FREEZE_GRAB
  }
  /** The freeze handle at viewport `x` (a line down the rows and a grip at the pointer's height). */
  const paintFreeze = (frame: OverlayFrame, x: number, y: number) => {
    const a = frame.axes
    const bottom = Math.min(frame.viewport.height, a.height - frame.viewport.scrollY)
    const ctx = frame.ctx
    ctx.save()
    ctx.setTransform(frame.dpr, 0, 0, frame.dpr, 0, 0)
    ctx.fillStyle = frame.style.border
    ctx.fillRect(x - 1, a.headerHeight, 2, bottom - a.headerHeight)
    ctx.beginPath()
    ctx.roundRect(x - 3, Math.min(Math.max(y - 16, a.headerHeight), bottom - 32), 6, 32, 3)
    ctx.fill()
    ctx.restore()
  }
  let freezeHover = false
  removers.push(
    interaction.addGesture({
      down(hit) {
        if (!onFreezeEdge(hit) || onHandle(hit)) return false
        freeze = { x: hit.x, y: hit.y }
        set((frame) => {
          if (!freeze) return
          const target = freezeEdge(frame.axes, frame.viewport, freezeCount(frame.axes, frame.viewport, freeze.x))
          const ctx = frame.ctx
          ctx.save()
          ctx.setTransform(frame.dpr, 0, 0, frame.dpr, 0, 0)
          ctx.fillStyle = frame.style.selection
          ctx.fillRect(target - 2, 0, 4, frame.viewport.height)
          ctx.restore()
          paintFreeze(frame, freeze.x, freeze.y)
        })
        return true
      },
      move(hit) {
        if (!freeze) return
        freeze.x = hit.x
        freeze.y = hit.y
        interaction.invalidateOverlay()
      },
      up(hit) {
        const done = freeze
        freeze = null
        set(null)
        const a = axes()
        if (!done || !a) return
        const count = freezeCount(a, view.viewport, hit.x)
        const now = a.columns.filter((column) => column.frozen).length
        if (count !== now) actions.freeze(count)
      },
      cancel() {
        freeze = null
        set(null)
      },
      cursor(hit) {
        const over = onFreezeEdge(hit)
        if (over !== freezeHover) {
          freezeHover = over
          set(over ? (frame) => paintFreeze(frame, frame.axes.bodyLeft, hit.y) : null)
        }
        return over ? 'col-resize' : null
      },
    }),
  )

  // ---- fill handle ----------------------------------------------------------------------------
  let fill: { bounds: CellBounds; row: number } | null = null
  /** The fill handle's center on the viewport, or null when there is none or it does not show. */
  const handleAt = (): { x: number; y: number } | null => {
    const a = axes()
    const bounds = fillBounds(interaction.selection, interaction.active)
    if (!a || !bounds || interaction.editing) return null
    const corner = screenUnion(a, view.viewport, cellArea(a, { row: bounds.bottom, col: bounds.right }))
    if (!corner) return null
    return { x: corner.x + corner.width, y: corner.y + corner.height }
  }
  const onHandle = (hit: GridHit) => {
    const at = handleAt()
    return at != null && Math.abs(hit.x - at.x) <= FILL_HANDLE / 2 + FILL_GRAB && Math.abs(hit.y - at.y) <= FILL_HANDLE / 2 + FILL_GRAB
  }
  removers.push(
    interaction.addPainter((frame) => {
      const bounds = fillBounds(interaction.selection, interaction.active)
      if (!bounds || interaction.editing) return
      const a = frame.axes
      if (fill) {
        const span = fillSpan(fill.bounds, fill.row)
        if (span) {
          const top = Math.min(span.from, span.to)
          const bottom = Math.max(span.from, span.to)
          const area = rangeArea(a, top, bottom, fill.bounds.left, fill.bounds.right)
          frame.play(overlayList([frame.style.border, frame.style.selection], [
            { op: Opcode.Rect, x: area.x, y: area.y, w: area.width, h: area.height, paint: 1, snap: true },
            { op: Opcode.RRectStroke, x: area.x, y: area.y, w: area.width, h: area.height, r: 0, width: 1, paint: 0 },
          ]))
        }
      }
      const corner = cellArea(a, { row: bounds.bottom, col: bounds.right })
      const x = corner.x + corner.width - FILL_HANDLE / 2
      const y = corner.y + corner.height - FILL_HANDLE / 2
      frame.play(overlayList([frame.style.border, frame.style.checkMark], [
        { op: Opcode.Rect, x: x - 1, y: y - 1, w: FILL_HANDLE + 2, h: FILL_HANDLE + 2, paint: 1, snap: true },
        { op: Opcode.Rect, x, y, w: FILL_HANDLE, h: FILL_HANDLE, paint: 0, snap: true },
      ]))
    }, 110),
  )
  removers.push(
    interaction.addGesture({
      down(hit) {
        const bounds = fillBounds(interaction.selection, interaction.active)
        if (!bounds || !onHandle(hit)) return false
        fill = { bounds, row: bounds.bottom }
        return true
      },
      move(hit) {
        const a = axes()
        if (!fill || !a) return
        const near = cellNear(a, view.viewport, interaction.rows, hit.x, hit.y)
        if (near && near.row !== fill.row) {
          fill.row = near.row
          interaction.invalidateOverlay()
        }
      },
      up() {
        const done = fill
        fill = null
        interaction.invalidateOverlay()
        const span = done ? fillSpan(done.bounds, done.row) : null
        if (done && span) actions.fill(done.bounds, span)
      },
      cancel() {
        fill = null
        interaction.invalidateOverlay()
      },
      cursor: (hit) => (onHandle(hit) ? 'crosshair' : null),
    }),
  )

  return () => {
    for (const remove of removers) remove()
  }
}
