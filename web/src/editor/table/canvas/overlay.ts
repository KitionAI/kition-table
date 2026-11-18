/**
 * The grid's overlay: what follows the pointer and the selection, drawn on the overlay canvas over
 * the content, so neither ever repaints the content (grid plan 3.1, 3.7). Its colors and sizes are
 * the engine's (`gridAxes`' `overlay`, `kition_table_grid::overlay`) and where things are comes
 * from the axes and the bands' hit records: the overlay never lays anything out.
 *
 * The overlay is built as a display list in document coordinates and played by the content's
 * player (`play`) into each of the viewport's four regions (the header and the rows, each left and
 * right of the frozen edge), clipped and scrolled as the content is, so a rectangle across the
 * frozen edge or under the header is cut exactly where the content is.
 */
import type { Area, Axes } from './axes'
import { type DisplayList, type Hit, Layer, type Op, Opcode } from './displayList'
import { cellArea, cornerCheckbox, type GridHit, HitKind, type HitSource, rangeArea, rowHeadParts, fieldColumnCount, type Viewport } from './hitTest'
import { play } from './player'
import type { GridResources } from './resources'
import type { GridRows } from './rows'
import { allRowsSelected, boundsOf, type CellRef, type GridSelection, isRowSelected, normalizeRanges } from './selection'

/** How the overlay draws: the engine's `overlay` in `gridAxes` (CSS colors, CSS pixels). */
export type OverlayStyle = {
  selection: string
  border: string
  hover: string
  rowHead: string
  control: string
  check: string
  checkMark: string
  icons: { drag: number | null; expand: number | null; check: number | null }
  controlSize: number
  checkboxRadius: number
  checkboxBorder: number
  checkMarkSize: number
  iconStroke: number
  activeWidth: number
  rangeWidth: number
}

/** A display list of `ops` over `colors`, to play on the overlay. */
export function overlayList(colors: string[], ops: Op[]): DisplayList {
  return {
    header: { flags: 0, epoch: 0, theme: 0, resources: 0, key: { layer: Layer.View, colBand: 0, rowBand: 0 }, x: 0, y: 0, width: 0, height: 0 },
    colors,
    rgba: new Uint8Array(0),
    ops,
    hits: [],
    bytes: 0,
  }
}

/** The four regions of the viewport and the document point at each one's canvas origin. */
export function overlayRegions(axes: Axes, viewport: Viewport): Array<{ clip: Area; ox: number; oy: number }> {
  const { width: w, height: h, scrollX: sx, scrollY: sy } = viewport
  const { bodyLeft: left, headerHeight: top } = axes
  return [
    { clip: { x: 0, y: top, width: left, height: h - top }, ox: 0, oy: sy },
    { clip: { x: left, y: top, width: w - left, height: h - top }, ox: sx, oy: sy },
    { clip: { x: 0, y: 0, width: left, height: top }, ox: 0, oy: 0 },
    { clip: { x: left, y: 0, width: w - left, height: top }, ox: sx, oy: 0 },
  ]
}

/** Plays `list` (document coordinates) into the overlay's four regions. */
export function playOverlay(ctx: CanvasRenderingContext2D, dpr: number, axes: Axes, viewport: Viewport, list: DisplayList, resources: GridResources) {
  for (const region of overlayRegions(axes, viewport)) {
    const clip = region.clip
    if (clip.width <= 0 || clip.height <= 0) continue
    ctx.save()
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.beginPath()
    const x0 = Math.round(clip.x * dpr)
    const y0 = Math.round(clip.y * dpr)
    ctx.rect(x0, y0, Math.round((clip.x + clip.width) * dpr) - x0, Math.round((clip.y + clip.height) * dpr) - y0)
    ctx.clip()
    play({ ctx, x: region.ox, y: region.oy, dpr }, list, resources, null)
    ctx.restore()
  }
}

/** The box of a control `size` px square in the middle of its slot (a hit record). */
export function controlBox(hit: Pick<Hit, 'x' | 'y' | 'w' | 'h'>, size: number): Area {
  return { x: hit.x + (hit.w - size) / 2, y: hit.y + Math.max(0, (hit.h - size) / 2), width: size, height: size }
}

/** The state the selection overlay draws. */
export type SelectionState = {
  axes: Axes
  rows: GridRows | null
  viewport: Viewport
  style: OverlayStyle
  selection: GridSelection
  active: CellRef | null
  hover: GridHit | null
  /** The bands' hit records (`HitSource.list`). */
  source: HitSource
}

const enum Paint {
  Selection = 0,
  Border,
  Hover,
  RowHead,
  Control,
  Check,
  CheckMark,
}

/** The linear rows the viewport shows. */
export function visibleRows(axes: Axes, viewport: Viewport): [number, number] {
  if (axes.rows === 0) return [0, -1]
  const first = Math.max(0, axes.rowAt(viewport.scrollY + axes.headerHeight))
  const last = Math.min(axes.rows - 1, axes.rowAt(viewport.scrollY + viewport.height - 0.001))
  return [first, last]
}

/**
 * The selection overlay's ops: the row heads' controls (over the hovered and the active row, and
 * over checked rows), the hovered row and column header, the selected cells, rows and columns,
 * a range's outline, the active cell's outline, and the header's checkbox when every row is checked.
 */
export function selectionOps(state: SelectionState): { colors: string[]; ops: Op[] } {
  const { axes, rows, viewport, style, selection, active, hover } = state
  const colors = [style.selection, style.border, style.hover, style.rowHead, style.control, style.check, style.checkMark]
  const ops: Op[] = []
  const fields = fieldColumnCount(axes)
  const lastField = axes.columns[fields - 1]
  const fieldsRight = lastField ? lastField.x + lastField.width : axes.bodyLeft
  const rect = (area: Area, paint: Paint) => {
    if (area.width > 0 && area.height > 0) ops.push({ op: Opcode.Rect, x: area.x, y: area.y, w: area.width, h: area.height, paint, snap: true })
  }
  const outline = (area: Area, width: number) => {
    if (area.width > 0 && area.height > 0) ops.push({ op: Opcode.RRectStroke, x: area.x, y: area.y, w: area.width, h: area.height, r: 0, width, paint: Paint.Border })
  }
  const icon = (id: number | null, box: Area, paint: Paint) => {
    if (id != null) ops.push({ op: Opcode.Icon, icon: id, x: box.x, y: box.y, size: box.width, stroke: style.iconStroke, paint, solid: false })
  }
  const checkbox = (slot: Pick<Hit, 'x' | 'y' | 'w' | 'h'>, checked: boolean) => {
    const box = controlBox(slot, style.controlSize)
    if (checked) {
      ops.push({ op: Opcode.RRect, x: box.x, y: box.y, w: box.width, h: box.height, r: style.checkboxRadius, paint: Paint.Check })
      ops.push({ op: Opcode.RRectStroke, x: box.x, y: box.y, w: box.width, h: box.height, r: style.checkboxRadius, width: style.checkboxBorder, paint: Paint.Check })
      icon(style.icons.check, controlBox(slot, style.checkMarkSize), Paint.CheckMark)
    } else ops.push({ op: Opcode.RRectStroke, x: box.x, y: box.y, w: box.width, h: box.height, r: style.checkboxRadius, width: style.checkboxBorder, paint: Paint.Control })
  }
  // A row's area without its bottom line, from `left` to `right`.
  const rowBand = (row: number, left: number, right: number): Area => ({ x: left, y: axes.rowTop(row), width: right - left, height: axes.rowHeight(row) - 1 })

  const hoverRow = hover && (hover.region === 'cell' || hover.region === 'rowHead') ? hover.row : -1
  const activeRow = active && selection.kind === 'cells' ? active.row : -1
  const [first, last] = visibleRows(axes, viewport)
  // Rows whose row head shows controls: all of them when hovered or active, the checkbox alone when only checked.
  const controls: Array<{ row: number; all: boolean }> = []
  for (let row = first; row <= last; row += 1) {
    if (rows && !rows.isRecord(row)) continue
    const all = row === hoverRow || row === activeRow
    if (all || isRowSelected(selection, row)) controls.push({ row, all })
  }
  for (const { row } of controls) rect(rowBand(row, 0, axes.rowHeadWidth - 1), Paint.RowHead)

  // Hover: the row across its cells and row head, or a column's header.
  if (hoverRow >= 0 && (!rows || rows.isRecord(hoverRow))) rect(rowBand(hoverRow, 0, fieldsRight), Paint.Hover)
  if (hover?.region === 'header' && hover.col >= 0) {
    const column = axes.columns[hover.col]
    rect({ x: column.x, y: 0, width: column.width - 1, height: axes.headerHeight - 1 }, Paint.Hover)
  }

  // The selection.
  switch (selection.kind) {
    case 'cells': {
      const b = boundsOf(selection.anchor, selection.focus)
      if (b.top !== b.bottom || b.left !== b.right) {
        const area = rangeArea(axes, b.top, b.bottom, b.left, b.right)
        rect(area, Paint.Selection)
        outline(area, style.rangeWidth)
      }
      break
    }
    case 'rows':
      for (const [from, to] of normalizeRanges(selection.ranges)) {
        const a = Math.max(from, first)
        const b = Math.min(to, last)
        if (a > b) continue
        const top = axes.rowTop(a)
        rect({ x: axes.rowHeadWidth, y: top, width: fieldsRight - axes.rowHeadWidth, height: axes.rowTop(b + 1) - top }, Paint.Selection)
      }
      break
    case 'columns': {
      const left = Math.min(selection.anchor, selection.focus)
      const right = Math.max(selection.anchor, selection.focus)
      const area = rangeArea(axes, 0, axes.rows - 1, left, right)
      rect({ x: area.x, y: 0, width: area.width, height: axes.height }, Paint.Selection)
      break
    }
    case 'none':
      break
  }
  if (active && selection.kind === 'cells' && active.col >= 0 && active.col < fields) outline(cellArea(axes, active), style.activeWidth)

  // The row heads' controls, centred in the engine's slots.
  for (const { row, all } of controls) {
    for (const part of rowHeadParts(state.source, row)) {
      if (part.kind === HitKind.Checkbox) checkbox(part, isRowSelected(selection, row))
      else if (!all) continue
      else if (part.kind === HitKind.DragHandle) icon(style.icons.drag, controlBox(part, style.controlSize), Paint.Control)
      else if (part.kind === HitKind.Expand) icon(style.icons.expand, controlBox(part, style.controlSize), Paint.Control)
    }
  }
  if (rows && allRowsSelected(selection, rows)) {
    const corner = cornerCheckbox(state.source)
    if (corner) checkbox(corner, true)
  }
  return { colors, ops }
}
