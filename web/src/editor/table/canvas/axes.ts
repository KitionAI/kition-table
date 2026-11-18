/**
 * The grid's axes on the page: where each linear row (record, group header, add row) and each
 * shown column is, in document CSS pixels, as the engine arranged them for one `epoch`. The page
 * hit-tests and scrolls with them without a round trip to the worker; it never computes a row's
 * height or a column's width itself.
 *
 * Document coordinates are the whole grid's, unscrolled: the row heads start at x = 0, the
 * header at y = 0, the first row at y = `headerHeight` and the first column at x = `rowHeadWidth`.
 * Bands split the rows into runs of `bandRows` rows and the scrolling columns into strips
 * `bandWidth` pixels wide, counted from the first scrolling column.
 */
import { type BandKey, Layer } from './displayList'

/** A shown column; the last one is the add-field column, whose `fieldId` is null. */
export type Column = { x: number; width: number; frozen: boolean; fieldId?: number | null }

export type AxesInput = {
  epoch: number
  /** Row heights as runs of (height, count), in display order. */
  rowRuns: Array<[number, number]>
  columns: Column[]
  headerHeight: number
  rowHeadWidth: number
  /** Rows per row band and pixels per column band. */
  bandRows: number
  bandWidth: number
}

/** A rectangle in screen (viewport) CSS pixels. */
export type Area = { x: number; y: number; width: number; height: number }

export class Axes {
  readonly epoch: number
  readonly rows: number
  readonly columns: Column[]
  readonly headerHeight: number
  readonly rowHeadWidth: number
  readonly bandRows: number
  readonly bandWidth: number
  /** The frozen columns' total width, and where the scrolling columns start and end. */
  readonly frozenWidth: number
  readonly bodyLeft: number
  readonly width: number
  readonly height: number
  // Runs: first row, top, height.
  private readonly runRow: Float64Array
  private readonly runTop: Float64Array
  private readonly runHeight: Float64Array

  constructor(input: AxesInput) {
    this.epoch = input.epoch
    this.columns = input.columns
    this.headerHeight = input.headerHeight
    this.rowHeadWidth = input.rowHeadWidth
    this.bandRows = Math.max(1, input.bandRows)
    this.bandWidth = Math.max(1, input.bandWidth)
    const runs = input.rowRuns.filter(([, count]) => count > 0)
    this.runRow = new Float64Array(runs.length + 1)
    this.runTop = new Float64Array(runs.length + 1)
    this.runHeight = new Float64Array(runs.length)
    let row = 0
    let top = input.headerHeight
    runs.forEach(([height, count], index) => {
      this.runRow[index] = row
      this.runTop[index] = top
      this.runHeight[index] = height
      row += count
      top += height * count
    })
    this.runRow[runs.length] = row
    this.runTop[runs.length] = top
    this.rows = row
    this.height = top
    const frozen = input.columns.filter((column) => column.frozen)
    this.frozenWidth = frozen.reduce((sum, column) => sum + column.width, 0)
    this.bodyLeft = input.rowHeadWidth + this.frozenWidth
    const last = input.columns[input.columns.length - 1]
    this.width = last ? Math.max(this.bodyLeft, last.x + last.width) : this.bodyLeft
  }

  private run(row: number): number {
    let lo = 0
    let hi = this.runHeight.length - 1
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1
      if (this.runRow[mid] <= row) lo = mid
      else hi = mid - 1
    }
    return lo
  }

  /** The top of `row` (clamped to the rows there are). */
  rowTop(row: number): number {
    if (row <= 0 || this.rows === 0) return this.headerHeight
    if (row >= this.rows) return this.height
    const run = this.run(row)
    return this.runTop[run] + (row - this.runRow[run]) * this.runHeight[run]
  }

  rowHeight(row: number): number {
    if (row < 0 || row >= this.rows) return 0
    return this.runHeight[this.run(row)]
  }

  /** The row at document `y`, or -1 above the rows and `rows` below them. */
  rowAt(y: number): number {
    if (y < this.headerHeight) return -1
    if (y >= this.height) return this.rows
    let lo = 0
    let hi = this.runHeight.length - 1
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1
      if (this.runTop[mid] <= y) lo = mid
      else hi = mid - 1
    }
    return Math.min(this.runRow[lo + 1] - 1, this.runRow[lo] + Math.floor((y - this.runTop[lo]) / this.runHeight[lo]))
  }

  /** The column at document `x` (shown index), or -1. */
  columnAt(x: number): number {
    let lo = 0
    let hi = this.columns.length - 1
    while (lo <= hi) {
      const mid = (lo + hi) >> 1
      const column = this.columns[mid]
      if (x < column.x) hi = mid - 1
      else if (x >= column.x + column.width) lo = mid + 1
      else return mid
    }
    return -1
  }

  get rowBands(): number {
    return Math.ceil(this.rows / this.bandRows)
  }

  get colBands(): number {
    return Math.max(1, Math.ceil((this.width - this.bodyLeft) / this.bandWidth))
  }

  /** The document rectangle a band covers. */
  bandArea(key: BandKey): Area {
    const rows = (): [number, number] => {
      const top = this.rowTop(key.rowBand * this.bandRows)
      return [top, this.rowTop((key.rowBand + 1) * this.bandRows) - top]
    }
    const strip = (): [number, number] => {
      const left = this.bodyLeft + key.colBand * this.bandWidth
      return [left, Math.max(0, Math.min(this.bandWidth, this.width - left))]
    }
    switch (key.layer) {
      case Layer.Body: {
        const [y, height] = rows()
        const [x, width] = strip()
        return { x, y, width, height }
      }
      case Layer.Frozen: {
        const [y, height] = rows()
        return { x: this.rowHeadWidth, y, width: this.frozenWidth, height }
      }
      case Layer.RowHead: {
        const [y, height] = rows()
        return { x: 0, y, width: this.rowHeadWidth, height }
      }
      case Layer.Header: {
        const [x, width] = strip()
        return { x, y: 0, width, height: this.headerHeight }
      }
      case Layer.HeaderFrozen:
        return { x: 0, y: 0, width: this.bodyLeft, height: this.headerHeight }
      case Layer.View:
        return { x: 0, y: 0, width: this.width, height: this.height }
    }
  }
}

/** The bands a viewport needs: `top`..`bottom` document rows range and `left`..`right` body xs. */
export function bandKeys(axes: Axes, top: number, bottom: number, left: number, right: number): BandKey[] {
  const keys: BandKey[] = []
  if (axes.rows > 0) {
    const firstRow = Math.max(0, axes.rowAt(Math.max(axes.headerHeight, top)))
    const lastRow = Math.min(axes.rows - 1, axes.rowAt(Math.min(axes.height - 0.001, bottom)))
    const firstBand = Math.floor(firstRow / axes.bandRows)
    const lastBand = Math.floor(lastRow / axes.bandRows)
    const firstStrip = Math.max(0, Math.floor((left - axes.bodyLeft) / axes.bandWidth))
    const lastStrip = Math.min(axes.colBands - 1, Math.floor((right - axes.bodyLeft) / axes.bandWidth))
    for (let rowBand = firstBand; rowBand <= lastBand; rowBand += 1) {
      for (let colBand = firstStrip; colBand <= lastStrip; colBand += 1) keys.push({ layer: Layer.Body, colBand, rowBand })
      if (axes.frozenWidth > 0) keys.push({ layer: Layer.Frozen, colBand: 0, rowBand })
      keys.push({ layer: Layer.RowHead, colBand: 0, rowBand })
    }
  }
  return keys
}

export function keyId(key: BandKey): string {
  return `${key.layer}:${key.colBand}:${key.rowBand}`
}
