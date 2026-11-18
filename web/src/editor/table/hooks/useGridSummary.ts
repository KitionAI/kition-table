import { useEffect, useState } from 'react'
import type { DataField } from '../../types/dataDocument'
import type { GridAxis } from '../lib/gridAxis'
import { useTableDataSource } from '../ports/TableDataSource'

/** A number field's sum and average over the records the view shows, as the footer reads them. */
export type NumericSummary = { field: DataField; count: number; sum: number; average: number }

const NONE: NumericSummary[] = []
/** The number fields the footer sums, at most. */
const SHOWN = 3

/**
 * The footer's sums and averages of the shown number fields, over the records the view shows:
 * the engine reads them (`grid.summary`, `gridSummary`), so the editor never holds the records to
 * sum them. Asked again with each new arrangement and after every change to the document.
 */
export function useGridSummary(axis: GridAxis | null, fields: DataField[]): NumericSummary[] {
  const grid = useTableDataSource().grid
  const [summaries, setSummaries] = useState<NumericSummary[]>(NONE)
  const [changes, setChanges] = useState(0)
  const numbers = fields.filter((field) => field.type === 'number')
  const key = numbers.map((field) => `${field.id}:${field.name}`).join(',')

  useEffect(() => grid.subscribe(() => setChanges((count) => count + 1)), [grid])

  useEffect(() => {
    if (!axis || !numbers.length) {
      setSummaries(NONE)
      return
    }
    let live = true
    grid.summary({ fields: numbers.map((field) => field.name) }).then(
      (answer) => {
        if (!live) return
        const made = numbers
          .map((field, index) => {
            const { count, sum } = answer[index] ?? { count: 0, sum: 0 }
            return { field, count, sum, average: count ? sum / count : 0 }
          })
          .filter((item) => item.count > 0)
          .slice(0, SHOWN)
        setSummaries(made.length ? made : NONE)
      },
      // No arrangement in the engine yet: the next axis asks again.
      () => {},
    )
    return () => {
      live = false
    }
  }, [grid, axis, key, changes])

  return summaries
}
