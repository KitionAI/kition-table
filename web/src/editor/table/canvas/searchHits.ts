/**
 * The cells holding the editor's search, tinted on the grid's overlay (grid plan F20). The engine
 * knows which cells hold it (`gridHits`, kition-table-arrange): the grid asks for the rows it
 * shows and `MARGIN` rows on each side, and again when it scrolls past them, when the
 * arrangement changes, or when the document does (an edit can make a cell hold the search or stop
 * holding it). The content canvas is never drawn for it.
 */
import type { Op } from './displayList'
import { Opcode } from './displayList'
import { cellArea } from './hitTest'
import type { GridInteraction, OverlayFrame } from './interaction'
import { cssRgba, overlayList } from './overlay'

/** Rows asked for beyond the shown ones on each side, so that a short scroll finds its hits there. */
export const MARGIN = 64
/** How strongly a holding cell is tinted with the brand. */
const TINT = 0.22

/** Which cells of `count` rows of arrangement `epoch` from its `from`th record hold the search, in `fieldIds`: `[index into the arrangement's ids, field id]`. */
export type HitsFetch = (params: { epoch: number; from: number; count: number; fieldIds: number[] }) => Promise<Array<[number, number]>>

/** The arrangement the hits are of: its epoch and its records' ids in order. */
export type SearchedAxis = { epoch: number; ids: number[] }

export class SearchHighlight {
  private axis: SearchedAxis | null = null
  private index = new Map<number, number>()
  /** The rows of the arrangement asked for, and the cells found in them (record id → field ids). */
  private asked: { from: number; to: number; key: string } | null = null
  private cells = new Map<number, Set<number>>()
  private run = 0
  private readonly cleanup: Array<() => void> = []

  constructor(
    private readonly grid: GridInteraction,
    private readonly fetch: HitsFetch,
  ) {
    this.cleanup.push(grid.addPainter((frame) => this.paint(frame), 50))
    this.cleanup.push(grid.view.onFrame(() => this.follow()))
  }

  destroy() {
    this.run += 1
    for (const undo of this.cleanup) undo()
  }

  /** The arrangement while a search is on (null: no search, no tint). */
  setAxis(axis: SearchedAxis | null) {
    if (axis === this.axis) return
    this.axis = axis
    this.index = new Map(axis?.ids.map((id, index) => [id, index]) ?? [])
    this.refresh()
  }

  /** Asks again for the shown rows (the document changed). */
  refresh() {
    this.asked = null
    this.follow()
  }

  /** The arrangement's indexes of the records the view shows, or null when it shows none. */
  private shown(): { from: number; to: number } | null {
    const axes = this.grid.view.currentAxes
    const rows = this.grid.rows
    if (!axes || !rows) return null
    const { scrollY, height } = this.grid.view.viewport
    const first = Math.max(0, axes.rowAt(scrollY + axes.headerHeight))
    const last = Math.min(rows.count - 1, axes.rowAt(scrollY + height - 1))
    let from = Number.POSITIVE_INFINITY
    let to = -1
    for (let row = first; row <= last; row += 1) {
      const id = rows.recordId(row)
      const index = id == null ? undefined : this.index.get(id)
      if (index === undefined) continue
      from = Math.min(from, index)
      to = Math.max(to, index)
    }
    return to < 0 ? null : { from, to }
  }

  private follow() {
    const axis = this.axis
    const columns = this.grid.view.currentAxes?.columns ?? []
    const fieldIds = columns.map((column) => column.fieldId).filter((id): id is number => id != null)
    if (!axis || !fieldIds.length) {
      if (this.cells.size) {
        this.cells = new Map()
        this.grid.invalidateOverlay()
      }
      return
    }
    const shown = this.shown()
    if (!shown) return
    const key = `${axis.epoch}:${fieldIds.join(',')}`
    const asked = this.asked
    if (asked && asked.key === key && shown.from >= asked.from && shown.to < asked.to) return
    const from = Math.max(0, shown.from - MARGIN)
    const to = Math.min(axis.ids.length, shown.to + 1 + MARGIN)
    this.asked = { from, to, key }
    const run = ++this.run
    this.fetch({ epoch: axis.epoch, from, count: to - from, fieldIds }).then(
      (hits) => {
        if (run !== this.run) return
        const cells = new Map<number, Set<number>>()
        for (const [index, fieldId] of hits) {
          const id = axis.ids[index]
          if (id == null) continue
          let fields = cells.get(id)
          if (!fields) cells.set(id, (fields = new Set()))
          fields.add(fieldId)
        }
        this.cells = cells
        this.grid.invalidateOverlay()
      },
      // A newer arrangement is on its way; its hits follow it.
      () => {
        if (run === this.run) this.asked = null
      },
    )
  }

  private paint(frame: OverlayFrame) {
    const rows = frame.rows
    if (!this.cells.size || !rows) return
    const [r, g, b] = cssRgba(frame.style.border)
    const ops: Op[] = []
    frame.axes.columns.forEach((column, col) => {
      if (column.fieldId == null) return
      for (const [recordId, fields] of this.cells) {
        if (!fields.has(column.fieldId)) continue
        const row = rows.rowOf(recordId)
        if (row < 0) continue
        const area = cellArea(frame.axes, { row, col })
        ops.push({ op: Opcode.Rect, x: area.x, y: area.y, w: area.width, h: area.height, paint: 0, snap: true })
      }
    })
    if (ops.length) frame.play(overlayList([`rgba(${r},${g},${b},${TINT})`], ops))
  }

  /** The cells tinted now, as `recordId:fieldId` (the tests read it). */
  get tinted(): string[] {
    return [...this.cells].flatMap(([id, fields]) => [...fields].map((field) => `${id}:${field}`))
  }
}
