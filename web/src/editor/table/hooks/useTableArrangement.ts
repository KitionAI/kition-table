import { useEffect, useRef, useState } from 'react'
import { useTableDataSource, type GridOpenParams } from '../ports/TableDataSource'
import type { GridAxis } from '../lib/gridAxis'

type UseTableArrangementArgs = {
  tableId: number | null
  viewId: number | null
  /** The view's config as the editor holds it (`viewConfigOf` of its settings). */
  config: Record<string, unknown>
  query: string
  collapsedGroupIds: ReadonlySet<string>
  /** Changes when the editor's records or fields change: the arrangement is brought up to date then too. */
  revision: unknown
}

/**
 * The engine's arrangement of the view the editor shows (`TableDataSource.grid`): arranged again
 * when the table, the view, its settings, the search or the collapsed groups change, and brought
 * up to date after every change to the document. Answers the latest axis, or null until the first
 * one for this table and view arrives.
 */
export function useTableArrangement({ tableId, viewId, config, query, collapsedGroupIds, revision }: UseTableArrangementArgs): GridAxis | null {
  const dataSource = useTableDataSource()
  const grid = dataSource.grid
  const [axis, setAxis] = useState<GridAxis | null>(null)
  const opened = useRef<GridOpenParams | null>(null)
  const configKey = JSON.stringify(config)
  const collapsedKey = JSON.stringify(Array.from(collapsedGroupIds).sort())

  useEffect(() => {
    if (tableId == null) {
      opened.current = null
      setAxis(null)
      return
    }
    const params: GridOpenParams = {
      tableId,
      viewId,
      config: JSON.parse(configKey) as Record<string, unknown>,
      query,
      collapsed: JSON.parse(collapsedKey) as string[],
    }
    const previous = opened.current
    opened.current = params
    // Another table or view shows nothing of the previous one's order.
    if (previous && (previous.tableId !== tableId || previous.viewId !== viewId)) setAxis(null)
    let live = true
    void grid.open(params).then(
      (next) => {
        if (live) accept(next)
      },
      () => {},
    )
    return () => {
      live = false
    }
  }, [grid, tableId, viewId, configKey, query, collapsedKey])

  // The engine's worker answers in the order it is asked, so the last answer is the latest axis.
  function accept(next: GridAxis) {
    setAxis(next)
  }

  function sync() {
    if (!opened.current) return
    const params = opened.current
    grid.sync().then(
      (next) => {
        if (next && opened.current === params) accept(next)
      },
      // The engine holds no arrangement (it was opened again): arrange the view anew.
      () => {
        if (opened.current !== params) return
        void grid.open(params).then((next) => {
          if (opened.current === params) accept(next)
        }, () => {})
      },
    )
  }

  const syncRef = useRef(sync)
  syncRef.current = sync
  useEffect(() => grid.subscribe(() => syncRef.current()), [grid])
  useEffect(() => {
    syncRef.current()
  }, [revision])

  return axis
}
