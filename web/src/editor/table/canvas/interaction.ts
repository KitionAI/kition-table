/**
 * The grid's interaction on the page (grid plan F7, F8, F14–F17, F22, F23; section 3.1): pointer
 * and keyboard input over a `GridView`, the selection and the active cell (editor state, never in
 * the engine), and the overlay that shows them. Everything is hit-tested synchronously with the
 * axes, the linear rows and the bands' hit records (`hitTest.ts`); nothing waits on the worker.
 *
 * - Pointer: a press on a cell makes it active and starts a drag that selects a range (Shift
 *   extends from the active cell); near or past an edge of the scrolling cells the drag scrolls
 *   (`autoscrollVelocity`). A press on a column header selects the column (Shift and dragging extend
 *   it). A row's checkbox toggles the row (Shift toggles the run from the last one clicked), the
 *   header's checkbox every row. The expand control, a column's menu caret, a group's toggle, the
 *   add row and the add-field column act on release, when the press and the release are over the
 *   same part.
 * - Keyboard (`handleKey`): arrows (Shift extends, Cmd/Ctrl jumps to the edge), Tab and Shift+Tab,
 *   Enter and F2 (edit), a printable key (edit, starting with it), Escape, Delete and Backspace,
 *   Space (expand the record), Page Up and Page Down, Cmd/Ctrl+A, Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z
 *   (or Ctrl+Y). Nothing while an editor is open (`editing`): the editor owns the keys.
 * - Hover: the hovered row and column header are tinted and a hovered row shows its controls; the
 *   overlay is drawn again only when what is hovered changes, and a drag redraws it only when the
 *   cell under the pointer changes. The content canvas is never drawn for any of it.
 *
 * Other features plug in without touching this: gestures (`addGesture`: dragging rows and columns,
 * resizing, the freeze handle, the fill handle) are asked before the selection, and painters
 * (`addPainter`) draw on the overlay after it.
 */
import type { Area, Axes } from './axes'
import type { DisplayList, Op } from './displayList'
import type { GridView } from './gridView'
import { cellArea, cellNear, fieldColumnCount, type GridHit, HitKind, hitTest, type HitSource, toScreen } from './hitTest'
import { type OverlayStyle, overlayList, playOverlay, selectionOps } from './overlay'
import type { GridRows } from './rows'
import {
  allRowsSelected,
  boundsOf,
  type CellRef,
  cellSelection,
  type GridSelection,
  isRowSelected,
  moveCell,
  NO_SELECTION,
  sameCell,
  sameSelection,
  selectAllCells,
  selectAllRows,
  selectedRecordIds,
  toggleRow,
} from './selection'

/** A client rectangle (CSS pixels of the page). */
export type ClientRect = { x: number; y: number; width: number; height: number }

/** What the grid asks of the editor that holds it. Every handler is optional. */
export type GridHandlers = {
  /** Open the cell's editor: Enter, F2 or a double click (`text` null), or a typed character. */
  onEdit?(cell: CellRef, text: string | null): void
  /** Delete or Backspace over a selection. */
  onDelete?(selection: GridSelection): void
  onUndo?(): void
  onRedo?(): void
  /** The record context menu: the records it acts on, the cell pressed, where. */
  onContextMenu?(event: { recordIds: number[]; cell: CellRef | null; clientX: number; clientY: number }): void
  /** A column's menu caret: the column, its field, the caret's client rectangle. */
  onColumnMenu?(event: { col: number; fieldId: number; anchor: ClientRect }): void
  onExpandRecord?(recordId: number): void
  /** The add row: the record above it (its group), or null. */
  onAppendRecord?(event: { anchorRecordId: number | null }): void
  onAppendField?(event: { anchor: ClientRect }): void
  /** A group header's toggle. */
  onToggleGroup?(event: { groupId: string; collapsed: boolean }): void
  /** A click on a clickable part of a cell (checkbox, rating symbol, attachment, button, link). */
  onCellPart?(hit: GridHit): void
}

/** A pointer interaction another feature handles (asked before the selection, in order added). */
export type GridGesture = {
  /** Claims the press: the gesture then gets the moves and the release, and nothing is selected. */
  down(hit: GridHit, event: PointerEvent): boolean
  move?(hit: GridHit, event: PointerEvent): void
  up?(hit: GridHit, event: PointerEvent): void
  cancel?(): void
  /** The cursor over `hit` when no button is down, or null for the grid's own. */
  cursor?(hit: GridHit): string | null
}

/** What an overlay painter draws with. */
export type OverlayFrame = {
  ctx: CanvasRenderingContext2D
  dpr: number
  axes: Axes
  rows: GridRows | null
  viewport: { scrollX: number; scrollY: number; width: number; height: number }
  style: OverlayStyle
  interaction: GridInteraction
  /** Plays a list of document-coordinate ops into the viewport's regions, as the content is. */
  play(list: DisplayList): void
}

export type OverlayPainter = (frame: OverlayFrame) => void

export type InteractionState = { selection: GridSelection; active: CellRef | null }

/** How far from an edge of the scrolling cells a drag starts scrolling, and how fast it gets. */
export const AUTOSCROLL_EDGE = 32
export const AUTOSCROLL_MAX = 2400

/**
 * The scroll speed (CSS px per second, x and y) of a drag at viewport point (`x`, `y`): zero inside
 * the scrolling cells, then growing with how deep into the edge zone, or how far past the edge,
 * the pointer is.
 */
export function autoscrollVelocity(axes: Axes, viewport: { width: number; height: number }, x: number, y: number): [number, number] {
  const speed = (depth: number) => (depth <= 0 ? 0 : Math.min(AUTOSCROLL_MAX, depth * 24))
  const vx = speed(x - (viewport.width - AUTOSCROLL_EDGE)) - speed(axes.bodyLeft + AUTOSCROLL_EDGE - x)
  const vy = speed(y - (viewport.height - AUTOSCROLL_EDGE)) - speed(axes.headerHeight + AUTOSCROLL_EDGE - y)
  return [vx, vy]
}

/** Whether a click is released over the part it was pressed on (a press that moved elsewhere is no click). */
export function samePart(down: GridHit, up: GridHit): boolean {
  if (down.region !== up.region || down.row !== up.row || down.col !== up.col) return false
  const a = down.part
  const b = up.part
  return a === b || (a != null && b != null && a.kind === b.kind && a.index === b.index && a.row === b.row && a.col === b.col)
}

const hoverKey = (hit: GridHit | null) => (hit ? `${hit.region}:${hit.row}:${hit.col}:${hit.part?.kind ?? ''}:${hit.part?.index ?? ''}` : '')

type Drag = { kind: 'cells' | 'columns'; pointer: number; x: number; y: number }
type Press = { hit: GridHit; pointer: number; act: (hit: GridHit) => void }

export class GridInteraction {
  private state: InteractionState = { selection: NO_SELECTION, active: null }
  private hovered: GridHit | null = null
  private isEditing = false
  private drag: Drag | null = null
  private press: Press | null = null
  private claimed: { gesture: GridGesture; pointer: number } | null = null
  private lastToggled: number | null = null
  private scrollFrame = 0
  private scrollAt = 0
  private activeIds: { record: number | null; field: number | null } = { record: null, field: null }
  private readonly gestures: GridGesture[] = []
  private readonly painters: Array<{ paint: OverlayPainter; order: number }> = []
  private readonly changeListeners = new Set<(state: InteractionState) => void>()
  private readonly geometryListeners = new Set<() => void>()
  private readonly cleanup: Array<() => void> = []

  constructor(
    readonly view: GridView,
    private readonly handlers: GridHandlers = {},
  ) {
    const stage = view.scroller
    stage.tabIndex = 0
    stage.style.outline = 'none'
    const on = <K extends keyof HTMLElementEventMap>(type: K, listener: (event: HTMLElementEventMap[K]) => void) => {
      stage.addEventListener(type, listener)
      this.cleanup.push(() => stage.removeEventListener(type, listener))
    }
    on('pointerdown', (event) => this.onDown(event))
    on('pointermove', (event) => this.onMove(event))
    on('pointerup', (event) => this.onUp(event))
    on('pointercancel', () => this.endPointer())
    on('pointerleave', () => {
      if (!this.drag && !this.claimed) this.setHover(null)
    })
    on('dblclick', (event) => this.onDoubleClick(event))
    on('contextmenu', (event) => this.onContextMenu(event))
    on('keydown', (event) => {
      if (this.handleKey(event)) event.preventDefault()
    })
    this.cleanup.push(view.onAxes(() => this.onAxes()))
    this.cleanup.push(
      view.onFrame(() => {
        for (const listener of this.geometryListeners) listener()
      }),
    )
    this.addPainter((frame) => this.paintSelection(frame), 0)
    view.setOverlay((ctx, dpr) => this.paint(ctx, dpr))
  }

  destroy() {
    cancelAnimationFrame(this.scrollFrame)
    for (const undo of this.cleanup) undo()
    this.view.setOverlay(null)
  }

  // ---- state ------------------------------------------------------------------------------

  get selection(): GridSelection {
    return this.state.selection
  }

  get active(): CellRef | null {
    return this.state.active
  }

  get hover(): GridHit | null {
    return this.hovered
  }

  get rows(): GridRows | null {
    return this.view.opened?.rows ?? null
  }

  get style(): OverlayStyle | null {
    return this.view.opened?.overlay ?? null
  }

  get editing(): boolean {
    return this.isEditing
  }

  /** An editor opened or closed: while one is open the grid leaves the keys to it. */
  setEditing(editing: boolean) {
    this.isEditing = editing
  }

  /** How many shown columns hold fields. */
  get columnCount(): number {
    const axes = this.view.currentAxes
    return axes ? fieldColumnCount(axes) : 0
  }

  setSelection(selection: GridSelection, active: CellRef | null = this.state.active) {
    if (sameSelection(selection, this.state.selection) && sameCell(active, this.state.active)) return
    this.state = { selection, active: active ? { ...active } : null }
    this.rememberActive()
    this.view.redrawOverlay()
    for (const listener of this.changeListeners) listener(this.state)
  }

  /** Makes `cell` active (and the selection that cell), or with `extend` moves the range's focus there. */
  setActive(cell: CellRef | null, options: { extend?: boolean; scroll?: boolean } = {}) {
    if (!cell) return this.setSelection(NO_SELECTION, null)
    if (options.extend && this.state.active) {
      const anchor = this.state.selection.kind === 'cells' ? this.state.selection.anchor : this.state.active
      this.setSelection(cellSelection(anchor, cell))
    } else this.setSelection(cellSelection(cell), cell)
    if (options.scroll) this.scrollToCell(cell)
  }

  /** What the arrow keys do: moves the active cell (or with `extend` the range's focus), stepping over rows that are not records. */
  moveActive(dx: number, dy: number, options: { extend?: boolean; jump?: boolean } = {}) {
    const rows = this.rows
    if (!rows) return
    const start = this.state.active ?? { row: rows.firstRecord, col: 0 }
    if (start.row < 0) return
    if (options.extend) {
      const from = this.state.selection.kind === 'cells' ? this.state.selection.focus : start
      const to = moveCell(rows, this.columnCount, from, dx, dy, options.jump)
      this.setActive(to, { extend: true })
      this.scrollToCell(to)
      return
    }
    const to = this.state.active ? moveCell(rows, this.columnCount, start, dx, dy, options.jump) : start
    this.setActive(to, { scroll: true })
  }

  /** Calls `listener` when the selection or the active cell changes. */
  onChange(listener: (state: InteractionState) => void): () => void {
    this.changeListeners.add(listener)
    return () => this.changeListeners.delete(listener)
  }

  /** Calls `listener` when cells move on the screen (scroll, resize, new axes): an editor re-places itself. */
  onGeometry(listener: () => void): () => void {
    this.geometryListeners.add(listener)
    return () => this.geometryListeners.delete(listener)
  }

  // ---- geometry ---------------------------------------------------------------------------

  /** Where `cell` shows on the viewport (CSS px from the grid's top left), or null when it does not show. */
  cellScreenArea(cell: CellRef): Area | null {
    const axes = this.view.currentAxes
    if (!axes) return null
    const parts = toScreen(axes, this.view.viewport, cellArea(axes, cell))
    if (!parts.length) return null
    const left = Math.min(...parts.map((part) => part.x))
    const top = Math.min(...parts.map((part) => part.y))
    const right = Math.max(...parts.map((part) => part.x + part.width))
    const bottom = Math.max(...parts.map((part) => part.y + part.height))
    return { x: left, y: top, width: right - left, height: bottom - top }
  }

  /** Where the active cell shows (see `cellScreenArea`). */
  activeScreenArea(): Area | null {
    return this.state.active ? this.cellScreenArea(this.state.active) : null
  }

  /** Scrolls so `cell` shows whole (a frozen column never scrolls sideways). */
  scrollToCell(cell: CellRef) {
    const axes = this.view.currentAxes
    if (!axes) return
    const { scrollX, scrollY, width, height } = this.view.viewport
    const area = cellArea(axes, cell)
    let x = scrollX
    let y = scrollY
    if (area.x >= axes.bodyLeft) {
      if (area.x - x < axes.bodyLeft) x = area.x - axes.bodyLeft
      else if (area.x + area.width - x > width) x = Math.min(area.x - axes.bodyLeft, area.x + area.width - width)
    }
    if (area.y - y < axes.headerHeight) y = area.y - axes.headerHeight
    else if (area.y + area.height - y > height) y = Math.min(area.y - axes.headerHeight, area.y + area.height - height)
    if (x !== scrollX || y !== scrollY) this.view.scrollTo(Math.max(0, x), Math.max(0, y))
  }

  /** What is at client point (`clientX`, `clientY`). */
  hitAt(clientX: number, clientY: number): GridHit {
    const box = this.view.scroller.getBoundingClientRect()
    return this.hitAtViewport(clientX - box.left, clientY - box.top)
  }

  private source(): HitSource | null {
    const axes = this.view.currentAxes
    if (!axes) return null
    return { axes, rows: this.rows, viewport: this.view.viewport, list: (key) => this.view.cache.peek(key) }
  }

  private hitAtViewport(x: number, y: number): GridHit {
    const source = this.source()
    if (!source) return { region: 'none', row: -1, col: -1, part: null, docX: x, docY: y, x, y }
    return hitTest(source, x, y)
  }

  private clientRect(area: Area): ClientRect {
    const box = this.view.scroller.getBoundingClientRect()
    return { x: box.left + area.x, y: box.top + area.y, width: area.width, height: area.height }
  }

  /** A document rectangle's client rectangle (its first visible piece), for menus' anchors. */
  private anchorOf(area: Area): ClientRect {
    const axes = this.view.currentAxes
    const parts = axes ? toScreen(axes, this.view.viewport, area) : []
    return this.clientRect(parts[0] ?? { x: 0, y: 0, width: 0, height: 0 })
  }

  // ---- plug-ins -----------------------------------------------------------------------------

  /** Adds a gesture, asked before the selection on every press; answers how to remove it. */
  addGesture(gesture: GridGesture): () => void {
    this.gestures.push(gesture)
    return () => {
      const at = this.gestures.indexOf(gesture)
      if (at >= 0) this.gestures.splice(at, 1)
    }
  }

  /** Adds an overlay painter; lower `order` draws first (the selection is 0). */
  addPainter(paint: OverlayPainter, order: number): () => void {
    const entry = { paint, order }
    this.painters.push(entry)
    this.painters.sort((a, b) => a.order - b.order)
    this.view.redrawOverlay()
    return () => {
      const at = this.painters.indexOf(entry)
      if (at >= 0) this.painters.splice(at, 1)
      this.view.redrawOverlay()
    }
  }

  invalidateOverlay() {
    this.view.redrawOverlay()
  }

  private paint(ctx: CanvasRenderingContext2D, dpr: number) {
    const axes = this.view.currentAxes
    const style = this.style
    if (!axes || !style) return
    const viewport = this.view.viewport
    const frame: OverlayFrame = {
      ctx,
      dpr,
      axes,
      rows: this.rows,
      viewport,
      style,
      interaction: this,
      play: (list) => playOverlay(ctx, dpr, axes, viewport, list, this.view.resources),
    }
    for (const { paint } of this.painters) paint(frame)
  }

  private paintSelection(frame: OverlayFrame) {
    const source = this.source()
    if (!source) return
    const { colors, ops } = selectionOps({
      axes: frame.axes,
      rows: frame.rows,
      viewport: frame.viewport,
      style: frame.style,
      selection: this.state.selection,
      active: this.state.active,
      hover: this.drag ? null : this.hovered,
      source,
    })
    if (ops.length) frame.play(overlayList(colors, ops as Op[]))
  }

  // ---- new axes ---------------------------------------------------------------------------

  private rememberActive() {
    const active = this.state.active
    const axes = this.view.currentAxes
    this.activeIds = {
      record: active ? (this.rows?.recordId(active.row) ?? null) : null,
      field: active ? (axes?.columns[active.col]?.fieldId ?? null) : null,
    }
  }

  /** A new arrangement: the active cell follows its record and field; checked rows follow their records. */
  private onAxes() {
    const rows = this.rows
    const axes = this.view.currentAxes
    this.hovered = null
    if (!rows || !axes) return
    const columns = fieldColumnCount(axes)
    const { record, field } = this.activeIds
    let active: CellRef | null = null
    if (record != null) {
      const row = rows.rowOf(record)
      const col = axes.columns.findIndex((column) => column.fieldId === field)
      if (row >= 0 && col >= 0 && col < columns) active = { row, col }
    }
    let selection: GridSelection = active ? cellSelection(active) : NO_SELECTION
    if (this.state.selection.kind === 'columns') selection = { kind: 'columns', anchor: Math.min(this.state.selection.anchor, columns - 1), focus: Math.min(this.state.selection.focus, columns - 1) }
    this.state = { selection, active }
    this.rememberActive()
    for (const listener of this.changeListeners) listener(this.state)
    for (const listener of this.geometryListeners) listener()
  }

  /** Keeps the checked records checked across a new arrangement (call with the ids from before). */
  checkRecords(ids: number[]) {
    const rows = this.rows
    if (!rows) return
    const ranges: Array<[number, number]> = ids.map((id) => rows.rowOf(id)).filter((row) => row >= 0).map((row) => [row, row])
    this.setSelection(ranges.length ? { kind: 'rows', ranges } : NO_SELECTION, this.state.active)
  }

  // ---- pointer ----------------------------------------------------------------------------

  private viewportPoint(event: MouseEvent): { x: number; y: number } {
    const box = this.view.scroller.getBoundingClientRect()
    return { x: event.clientX - box.left, y: event.clientY - box.top }
  }

  private onDown(event: PointerEvent) {
    if (event.button !== 0) return
    const { x, y } = this.viewportPoint(event)
    const hit = this.hitAtViewport(x, y)
    // The scroll bars, or outside the grid.
    if (hit.region === 'none' && (x >= this.view.viewport.width || y >= this.view.viewport.height)) return
    this.view.scroller.focus({ preventScroll: true })
    for (const gesture of this.gestures) {
      if (gesture.down(hit, event)) {
        this.claimed = { gesture, pointer: event.pointerId }
        this.capture(event)
        this.setHover(null)
        return
      }
    }
    const rows = this.rows
    const part = hit.part
    const click = (act: (up: GridHit) => void) => {
      this.press = { hit, pointer: event.pointerId, act }
      this.capture(event)
    }
    switch (hit.region) {
      case 'cell': {
        const cell = { row: hit.row, col: hit.col }
        if (event.shiftKey && this.state.active) this.setActive(cell, { extend: true })
        else this.setActive(cell)
        if (part && part.col >= 0 && part.kind <= HitKind.Link) {
          click((up) => this.handlers.onCellPart?.(up))
          return
        }
        this.drag = { kind: 'cells', pointer: event.pointerId, x, y }
        this.capture(event)
        event.preventDefault()
        return
      }
      case 'rowHead': {
        if (!part) return
        if (part.kind === HitKind.Checkbox) {
          const extend = event.shiftKey && this.lastToggled != null ? this.lastToggled : null
          this.setSelection(toggleRow(this.state.selection, hit.row, extend), this.state.active)
          this.lastToggled = hit.row
          event.preventDefault()
        } else if (part.kind === HitKind.Expand) {
          click(() => {
            const id = rows?.recordId(hit.row)
            if (id != null) this.handlers.onExpandRecord?.(id)
          })
        }
        return
      }
      case 'corner':
        if (part?.kind === HitKind.Checkbox && rows) {
          this.setSelection(allRowsSelected(this.state.selection, rows) ? NO_SELECTION : selectAllRows(rows), this.state.active)
          event.preventDefault()
        }
        return
      case 'header': {
        if (part?.kind === HitKind.ColumnMenu) {
          click(() => this.columnMenu(hit.col))
          return
        }
        if (part?.kind === HitKind.ColumnEdge) return
        const anchor = event.shiftKey && this.state.selection.kind === 'columns' ? this.state.selection.anchor : hit.col
        this.setSelection({ kind: 'columns', anchor, focus: hit.col }, this.state.active)
        this.drag = { kind: 'columns', pointer: event.pointerId, x, y }
        this.capture(event)
        event.preventDefault()
        return
      }
      case 'group':
        if (part?.kind === HitKind.GroupToggle) click(() => this.toggleGroup(hit.row))
        return
      case 'appendRow':
        click(() => this.handlers.onAppendRecord?.({ anchorRecordId: rows?.appendAnchor(hit.row) ?? null }))
        return
      case 'appendColumn': {
        const axes = this.view.currentAxes
        const column = axes?.columns[axes.columns.length - 1]
        click(() => this.handlers.onAppendField?.({ anchor: this.anchorOf({ x: column?.x ?? 0, y: 0, width: column?.width ?? 0, height: axes?.headerHeight ?? 0 }) }))
        return
      }
      case 'none':
        return
    }
  }

  private capture(event: PointerEvent) {
    try {
      this.view.scroller.setPointerCapture(event.pointerId)
    } catch {
      // A synthetic or finished pointer: the moves still come while it is over the grid.
    }
  }

  private onMove(event: PointerEvent) {
    const { x, y } = this.viewportPoint(event)
    if (this.claimed) {
      this.claimed.gesture.move?.(this.hitAtViewport(x, y), event)
      return
    }
    if (this.drag) {
      this.drag.x = x
      this.drag.y = y
      this.extendDrag()
      this.autoscroll()
      return
    }
    if (this.press) return
    const hit = this.hitAtViewport(x, y)
    this.setHover(hit.region === 'none' ? null : hit)
    this.view.scroller.style.cursor = this.cursorFor(hit)
  }

  private cursorFor(hit: GridHit): string {
    for (const gesture of this.gestures) {
      const cursor = gesture.cursor?.(hit)
      if (cursor) return cursor
    }
    const part = hit.part
    if (hit.region === 'appendRow' || hit.region === 'appendColumn') return 'pointer'
    if (part && part.kind !== HitKind.ColumnEdge && part.kind !== HitKind.DragHandle) return 'pointer'
    return 'default'
  }

  private onUp(event: PointerEvent) {
    const { x, y } = this.viewportPoint(event)
    const hit = this.hitAtViewport(x, y)
    if (this.claimed) {
      const { gesture } = this.claimed
      this.claimed = null
      gesture.up?.(hit, event)
      return
    }
    const press = this.press
    this.endPointer()
    if (press && samePart(press.hit, hit)) press.act(hit)
  }

  private endPointer() {
    if (this.claimed) {
      this.claimed.gesture.cancel?.()
      this.claimed = null
    }
    this.press = null
    if (this.drag) {
      this.drag = null
      cancelAnimationFrame(this.scrollFrame)
      this.scrollFrame = 0
      this.view.redrawOverlay()
    }
  }

  /** A drag's focus follows the pointer, held inside the cells; the overlay redraws only when the cell changes. */
  private extendDrag() {
    const drag = this.drag
    const axes = this.view.currentAxes
    if (!drag || !axes) return
    const viewport = this.view.viewport
    if (drag.kind === 'cells') {
      const cell = cellNear(axes, viewport, this.rows, drag.x, drag.y)
      if (cell && this.state.active) this.setActive(cell, { extend: true })
    } else if (this.state.selection.kind === 'columns') {
      const near = cellNear(axes, viewport, null, drag.x, Math.max(drag.y, axes.headerHeight))
      if (near) this.setSelection({ kind: 'columns', anchor: this.state.selection.anchor, focus: near.col }, this.state.active)
    }
  }

  /** While a drag is near or past an edge of the scrolling cells, scrolls every frame and extends the drag. */
  private autoscroll() {
    if (this.scrollFrame || !this.drag) return
    this.scrollAt = performance.now()
    const tick = (now: number) => {
      this.scrollFrame = 0
      const drag = this.drag
      const axes = this.view.currentAxes
      if (!drag || !axes) return
      const [vx, vy] = autoscrollVelocity(axes, this.view.viewport, drag.x, drag.kind === 'columns' ? axes.headerHeight + AUTOSCROLL_EDGE : drag.y)
      if (vx === 0 && vy === 0) return
      const dt = Math.min(64, now - this.scrollAt) / 1000
      this.scrollAt = now
      this.view.scrollBy(vx * dt, vy * dt)
      this.extendDrag()
      this.scrollFrame = requestAnimationFrame(tick)
    }
    this.scrollFrame = requestAnimationFrame(tick)
  }

  private setHover(hit: GridHit | null) {
    if (hoverKey(hit) === hoverKey(this.hovered)) {
      this.hovered = hit
      return
    }
    this.hovered = hit
    this.view.redrawOverlay()
  }

  private onDoubleClick(event: MouseEvent) {
    const { x, y } = this.viewportPoint(event)
    const hit = this.hitAtViewport(x, y)
    if (hit.region === 'cell' && !(hit.part && hit.part.col >= 0)) this.handlers.onEdit?.({ row: hit.row, col: hit.col }, null)
  }

  private onContextMenu(event: MouseEvent) {
    const { x, y } = this.viewportPoint(event)
    const hit = this.hitAtViewport(x, y)
    const rows = this.rows
    if ((hit.region !== 'cell' && hit.region !== 'rowHead') || !rows) return
    event.preventDefault()
    const cell = hit.region === 'cell' ? { row: hit.row, col: hit.col } : null
    const selection = this.state.selection
    const inSelection =
      isRowSelected(selection, hit.row) ||
      (selection.kind === 'cells' && cell != null && (() => {
        const b = boundsOf(selection.anchor, selection.focus)
        return cell.row >= b.top && cell.row <= b.bottom && cell.col >= b.left && cell.col <= b.right
      })())
    if (!inSelection && cell) this.setActive(cell)
    const id = rows.recordId(hit.row)
    const recordIds = inSelection ? selectedRecordIds(this.state.selection, rows) : id != null ? [id] : []
    this.handlers.onContextMenu?.({ recordIds, cell, clientX: event.clientX, clientY: event.clientY })
  }

  private columnMenu(col: number) {
    const axes = this.view.currentAxes
    const column = axes?.columns[col]
    if (!axes || !column || column.fieldId == null) return
    this.handlers.onColumnMenu?.({ col, fieldId: column.fieldId, anchor: this.anchorOf({ x: column.x, y: 0, width: column.width, height: axes.headerHeight }) })
  }

  private toggleGroup(row: number) {
    const index = this.rows?.groupIndex(row)
    const group = index != null ? this.view.opened?.groups?.[index] : undefined
    if (group) this.handlers.onToggleGroup?.({ groupId: group.id, collapsed: !group.collapsed })
  }

  // ---- keyboard ---------------------------------------------------------------------------

  /** The grid's keys; answers whether it handled `event` (the caller then prevents its default). */
  handleKey(event: KeyboardEvent): boolean {
    if (this.isEditing || event.isComposing) return false
    const mod = event.metaKey || event.ctrlKey
    const key = event.key
    const lower = key.length === 1 ? key.toLowerCase() : key
    if (mod && lower === 'z') {
      if (event.shiftKey) this.handlers.onRedo?.()
      else this.handlers.onUndo?.()
      return true
    }
    if (event.ctrlKey && !event.metaKey && lower === 'y') {
      this.handlers.onRedo?.()
      return true
    }
    const rows = this.rows
    if (!rows) return false
    if (mod && lower === 'a') {
      const all = selectAllCells(rows, this.columnCount)
      if (all.kind === 'cells') this.setSelection(all, this.state.active ?? all.anchor)
      return true
    }
    const active = this.state.active
    switch (key) {
      case 'ArrowUp':
      case 'ArrowDown':
      case 'ArrowLeft':
      case 'ArrowRight': {
        const dx = key === 'ArrowLeft' ? -1 : key === 'ArrowRight' ? 1 : 0
        const dy = key === 'ArrowUp' ? -1 : key === 'ArrowDown' ? 1 : 0
        this.moveActive(dx, dy, { extend: event.shiftKey, jump: mod })
        return true
      }
      case 'Tab':
        if (!active) return false
        this.moveActive(event.shiftKey ? -1 : 1, 0)
        return true
      case 'PageUp':
      case 'PageDown': {
        if (!active) return false
        const axes = this.view.currentAxes
        const height = this.view.viewport.height - (axes?.headerHeight ?? 0)
        const step = Math.max(1, Math.floor(height / Math.max(1, axes?.rowHeight(active.row) ?? 1)))
        this.view.scrollBy(0, key === 'PageUp' ? -height : height)
        this.moveActive(0, key === 'PageUp' ? -step : step)
        return true
      }
      case 'Escape': {
        const selection = this.state.selection
        if (selection.kind === 'cells' && active && !(sameCell(selection.anchor, active) && sameCell(selection.focus, active))) {
          this.setSelection(cellSelection(active), active)
          return true
        }
        if (selection.kind === 'rows' || selection.kind === 'columns') {
          this.setSelection(active ? cellSelection(active) : NO_SELECTION, active)
          return true
        }
        return false
      }
      case 'Delete':
      case 'Backspace':
        if (this.state.selection.kind === 'none') return false
        this.handlers.onDelete?.(this.state.selection)
        return true
      case 'Enter':
      case 'F2':
        if (!active || this.state.selection.kind !== 'cells') return false
        this.handlers.onEdit?.(active, null)
        return true
      case ' ': {
        const id = active ? rows.recordId(active.row) : null
        if (id == null || event.shiftKey) break
        this.handlers.onExpandRecord?.(id)
        return true
      }
    }
    // A printable character starts editing the active cell with it.
    if (active && this.state.selection.kind === 'cells' && key.length === 1 && !mod && !event.altKey) {
      this.handlers.onEdit?.(active, key)
      return true
    }
    return false
  }
}
