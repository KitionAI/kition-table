/**
 * The grid brought up to date after a write (grid plan 3.7), without arranging it again: the
 * engine moves only the records the write touched (`gridSync`, kition-table-arrange's
 * relocation). When the arrangement's axis changed (a record moved, appeared or left the view),
 * the view takes the new layout; when it did not, only the cached bands that show a written
 * record go stale: they stay drawn until their new lists arrive, and the rest of the cache stays.
 *
 * A write to the view's columns (a width, the frozen count, the columns' order) lays the view out
 * again (`GridView.reopen`): it changes every band.
 */
import { type BandKey, Layer } from './displayList'
import type { EngineSource } from './engineSource'
import type { GridView } from './gridView'
import type { GridRows } from './rows'

/**
 * What a write changed: the cells of some records (`records`, their ids; null when it does not
 * say which, as for an undo), or the view's columns (`layout`).
 */
export type Written = { kind: 'cells'; records: number[] | null } | { kind: 'layout' }

/** How the view followed a write: laid out again, a new axis, or only stale bands. */
export type Synced = 'layout' | 'axes' | 'bands'

/**
 * Which band keys show one of the records `ids` (every band of rows when `ids` is null): the
 * bands of rows (body, frozen, row heads) whose run of `bandRows` rows holds one of them. The
 * header strips show no record.
 */
export function showsRecords(rows: GridRows, bandRows: number, ids: ReadonlySet<number> | null): (key: BandKey) => boolean {
  const held = new Map<number, boolean>()
  const holds = (rowBand: number) => {
    let answer = held.get(rowBand)
    if (answer === undefined) {
      answer = false
      const last = Math.min(rows.count, (rowBand + 1) * bandRows)
      for (let row = rowBand * bandRows; row < last && !answer; row += 1) {
        const id = rows.recordId(row)
        answer = id != null && (ids?.has(id) ?? false)
      }
      held.set(rowBand, answer)
    }
    return answer
  }
  return (key) => key.layer !== Layer.Header && key.layer !== Layer.HeaderFrozen && (ids === null || holds(key.rowBand))
}

/** Brings `view` (showing `source`) up to date after a write that changed `written`. */
export async function syncAfterWrite(view: GridView, source: EngineSource, written: Written): Promise<Synced> {
  if (written.kind === 'layout') {
    await view.reopen()
    return 'layout'
  }
  const opened = await source.sync()
  if (opened) {
    view.setAxes(opened)
    return 'axes'
  }
  const axes = view.currentAxes
  const rows = view.opened?.rows
  if (!axes || !rows) {
    await view.reopen()
    return 'layout'
  }
  view.cache.markStale(showsRecords(rows, axes.bandRows, written.records ? new Set(written.records) : null))
  view.invalidate()
  return 'bands'
}
