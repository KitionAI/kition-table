import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import { useTableDataSource } from '../ports/TableDataSource'
import type { DataDocument, DataTable, DataView } from '../../types/dataDocument'
import { parseFilterTree } from '../filter'
import {
  readViewConfig,
  type DataInlineFilterGroup,
  type DataInlineGroupItem,
  type DataInlineSortItem,
  type DataInlineViewConfig,
  type DataInlineViewMode,
  type GridRowHeightKey,
} from '../lib/tableEditorShared'

/** The view settings the editor holds as state, and persists as the view's config. */
export type TableViewSettings = {
  hiddenFieldNames: ReadonlySet<string>
  filterTree: DataInlineFilterGroup | null
  sortItems: DataInlineSortItem[]
  groupItems: DataInlineGroupItem[]
  coverFieldName: string
  rowHeightKey: GridRowHeightKey
  frozenColumnCount: number
}

/**
 * A view's sorts or groups as the editor lists them: each with an id, which the ones the engine
 * wrote (`tableView.setSorts`, `tableView.setGroups`) do not carry; theirs is made from its place.
 */
function keyed<T extends { id?: string; field_name: string }>(items: T[] | undefined, kind: string): Array<T & { id: string }> {
  return Array.isArray(items) ? items.map((item, index) => ({ ...item, id: item.id || `${kind}:${index}:${item.field_name}` })) : []
}

/** The settings a view's stored config gives, with the editor's defaults for what it leaves out. */
export function viewSettingsOf(view: DataView | null | undefined): TableViewSettings {
  const config = readViewConfig(view)
  return {
    hiddenFieldNames: new Set(config.hidden_field_names || []),
    filterTree: parseFilterTree(config.filter_tree),
    sortItems: keyed(config.sorts, 'sort'),
    groupItems: keyed(config.groups, 'group'),
    coverFieldName: config.cover_field_name || '',
    rowHeightKey: config.row_height || 'medium',
    frozenColumnCount: typeof config.frozen_column_count === 'number' ? config.frozen_column_count : 1,
  }
}

/** The config the editor persists for `settings`. */
export function viewConfigOf(settings: TableViewSettings): DataInlineViewConfig {
  return {
    hidden_field_names: Array.from(settings.hiddenFieldNames),
    filter_tree: settings.filterTree && settings.filterTree.children.length > 0 ? settings.filterTree : undefined,
    sorts: settings.sortItems.length ? settings.sortItems : undefined,
    groups: settings.groupItems.length ? settings.groupItems : undefined,
    cover_field_name: settings.coverFieldName || undefined,
    row_height: settings.rowHeightKey,
    frozen_column_count: settings.frozenColumnCount,
  }
}

/**
 * Whether `config` says what `view` already holds. Opening a view only reads it: its settings
 * pass through state (as new objects) and come back as a config that differs from the stored one
 * only in the defaults it spells out, which is no change to write. Writing it would make a
 * document that was only opened unsaved, and add an undo step nobody took.
 */
export function viewConfigUnchanged(view: DataView, config: DataInlineViewConfig): boolean {
  return JSON.stringify(viewConfigOf(viewSettingsOf(view))) === JSON.stringify(config)
}

type UseTableViewStateArgs = {
  document: DataDocument | null
  activeTable: DataTable | null
  activeView: DataView | null
  tableViews: DataView[]
  setDocument: Dispatch<SetStateAction<DataDocument | null>>
  setActiveViewId: Dispatch<SetStateAction<number | null>>
}

export function useTableViewState({
  document,
  activeTable,
  activeView,
  tableViews,
  setDocument,
  setActiveViewId,
}: UseTableViewStateArgs) {
  const dataSource = useTableDataSource()
  const [hiddenFieldNames, setHiddenFieldNames] = useState<Set<string>>(new Set())
  const [sortItems, setSortItems] = useState<DataInlineSortItem[]>([])
  const [groupItems, setGroupItems] = useState<DataInlineGroupItem[]>([])
  const [coverFieldName, setCoverFieldName] = useState('')
  const [filterTree, setFilterTree] = useState<DataInlineFilterGroup | null>(null)
  const [rowHeightKey, setRowHeightKey] = useState<GridRowHeightKey>('medium')
  const [frozenColumnCount, setFrozenColumnCount] = useState(1)
  const skipNextViewConfigPersistRef = useRef<number | null>(null)
  // A view change made while an earlier one is saving is the latest: the earlier save's answer
  // must not take it back (regressions/issue-view-save-race). The saves are numbered; only the
  // latest one's answer is taken, and when the settings changed after it was sent, the answer only
  // updates the document (the stored view) and the settings stay, to be saved next.
  const latestSaveRef = useRef(0)
  /** The view the settings are for, and the config they hold now (as JSON). */
  const settingsRef = useRef<{ viewId: number | null; config: string }>({ viewId: null, config: '' })
  /** Views a save answered with while the settings had moved on: they do not reset the settings. */
  const keepSettingsForRef = useRef(new WeakSet<DataView>())

  const viewMode: DataInlineViewMode = activeView?.type || 'grid'

  useEffect(() => {
    setActiveViewId((current) => current && tableViews.some((view) => view.id === current) ? current : tableViews[0]?.id ?? null)
  }, [activeTable?.id, setActiveViewId, tableViews])

  useEffect(() => {
    // A save's answer that came after a later change: the settings are newer than the view (once:
    // shown again later, after another view, the view gives the settings as usual).
    if (activeView && keepSettingsForRef.current.delete(activeView)) return
    if (activeView) skipNextViewConfigPersistRef.current = activeView.id
    const settings = viewSettingsOf(activeView)
    settingsRef.current = { viewId: activeView?.id ?? null, config: JSON.stringify(viewConfigOf(settings)) }
    setHiddenFieldNames(new Set(settings.hiddenFieldNames))
    setFilterTree(settings.filterTree)
    setSortItems(settings.sortItems)
    setGroupItems(settings.groupItems)
    setCoverFieldName(settings.coverFieldName)
    setRowHeightKey(settings.rowHeightKey)
    setFrozenColumnCount(settings.frozenColumnCount)
    // Depends on the `activeView` object itself, not just its id: a live
    // `table/changed` event (another window editing the same view) or an
    // undo/redo replaces the document with a freshly fetched `activeView`
    // that keeps the same id but carries a different `config`, and the
    // local view state below needs to pick that up.
  }, [activeTable?.id, activeView])

  useEffect(() => {
    if (!document || !activeTable || !activeView) return
    if (skipNextViewConfigPersistRef.current === activeView.id) {
      skipNextViewConfigPersistRef.current = null
      return
    }
    const config = viewConfigOf({
      hiddenFieldNames,
      filterTree,
      sortItems,
      groupItems,
      coverFieldName,
      rowHeightKey,
      frozenColumnCount,
    })
    const sent = JSON.stringify(config)
    settingsRef.current = { viewId: activeView.id, config: sent }
    if (viewConfigUnchanged(activeView, config)) return
    const documentId = document.id
    const tableId = activeTable.id
    const viewId = activeView.id
    const timer = window.setTimeout(() => {
      const save = ++latestSaveRef.current
      // The sorts and groups are named even when there are none: a config without them keeps the
      // stored ones (tableView.set), so an emptied list must be sent as one.
      const written = { ...config, sorts: config.sorts ?? [], groups: config.groups ?? [] }
      void dataSource.views.update({ documentId, tableId, viewId, config: written }).then((updated) => {
        // A later save was sent: its answer is the one to take.
        if (save !== latestSaveRef.current) return
        const settings = settingsRef.current
        if (settings.viewId === updated.id && settings.config !== sent) keepSettingsForRef.current.add(updated)
        setDocument((current) => current ? {
          ...current,
          tables: current.tables?.map((table) => table.id === tableId ? {
            ...table,
            views: table.views?.map((view) => view.id === updated.id ? updated : view),
          } : table),
        } : current)
      }).catch(() => {
        // View state remains usable locally; persistence will retry on the next change.
      })
    }, 500)
    return () => window.clearTimeout(timer)
  }, [
    activeTable?.id,
    activeView,
    coverFieldName,
    document?.id,
    filterTree,
    frozenColumnCount,
    groupItems,
    hiddenFieldNames,
    rowHeightKey,
    setDocument,
    sortItems,
  ])

  return {
    coverFieldName,
    filterTree,
    frozenColumnCount,
    groupItems,
    hiddenFieldNames,
    rowHeightKey,
    setCoverFieldName,
    setFilterTree,
    setFrozenColumnCount,
    setGroupItems,
    setHiddenFieldNames,
    setRowHeightKey,
    setSortItems,
    sortItems,
    viewMode,
  }
}
