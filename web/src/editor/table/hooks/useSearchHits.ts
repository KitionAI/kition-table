import { useEffect, useState } from 'react'
import type { GridAxis } from '../lib/gridAxis'
import { useTableDataSource } from '../ports/TableDataSource'

/** Rows asked for beyond the shown ones on each side, so that a short scroll finds its hits there. */
const MARGIN = 64
/** The rows taken as shown until the grid says which are. */
const FIRST_ROWS = 64

/** A cell holding the search, as the grid marks it. */
export type SearchHit = { recordId: string; fieldId: string }

/** The rows the grid shows: indexes into the axis's ids, both included. */
export type ShownRows = { from: number; to: number }

const NONE: SearchHit[] = []

/**
 * The cells holding the search among the rows the grid shows and the ones around them, asked of
 * the engine's arrangement (`grid.hits`) whenever the arrangement, the shown rows, the fields or
 * the records (`revision`) change. `axis` is the arrangement while a search is on, else null (no
 * hits).
 */
export function useSearchHits(axis: GridAxis | null, shown: ShownRows | null, fieldIds: number[], revision: unknown): SearchHit[] {
  const grid = useTableDataSource().grid
  const [hits, setHits] = useState<SearchHit[]>(NONE)
  const fieldsKey = fieldIds.join(',')
  const from = Math.max(0, (shown?.from ?? 0) - MARGIN)
  const to = Math.min(axis?.ids.length ?? 0, (shown?.to ?? FIRST_ROWS) + 1 + MARGIN)

  useEffect(() => {
    const fields = fieldsKey ? fieldsKey.split(',').map(Number) : []
    if (!axis || !fields.length || to <= from) {
      setHits(NONE)
      return
    }
    let live = true
    grid.hits({ epoch: axis.epoch, from, count: to - from, fieldIds: fields }).then(
      (found) => {
        if (live) setHits(found.map(([row, fieldId]) => ({ recordId: String(axis.ids[row]), fieldId: String(fieldId) })))
      },
      // A newer arrangement is on its way; its hits follow it.
      () => {},
    )
    return () => {
      live = false
    }
  }, [grid, axis, from, to, fieldsKey, revision])

  return hits
}
