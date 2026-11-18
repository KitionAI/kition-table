import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DataAttachment, DataField } from '../../types/dataDocument'
import { useIsDark } from '../../lib/useIsDark'
import type { GridCellSelection } from '../lib/tableEditorShared'
import type { GridAxis } from '../lib/gridAxis'
import { useTableDataSource } from '../ports/TableDataSource'
import { resolveTableFileURL, useTableHost, useTableNotify } from '../ports/TableHost'
import { aiCellStore } from '../store/aiCellGenerationStore'
import { arrangedSource, type ArrangedSource } from '../canvas/editorSource'
import { editorData } from '../canvas/editorData'
import { GridCellPopup } from '../canvas/GridCellEditors'
import { type CellPopup, GridEditing } from '../canvas/gridEditing'
import { showsRecords, type Written } from '../canvas/gridSync'
import { GridView as GridCanvas } from '../canvas/gridView'
import { GridInteraction } from '../canvas/interaction'
import { SearchHighlight } from '../canvas/searchHits'
import { timeZoneChanges } from '../canvas/timeZone'
import { NO_SELECTION, selectedRecordIds } from '../canvas/selection'

/** What the editor's grid shows and hands back to the editor. */
export type GridViewProps = {
  documentId: number
  tableId: number
  viewId: number | null
  /** The table's fields, marked read-only for a person who may only view the table. */
  fields: DataField[]
  /** The engine's arrangement of the view (`useTableArrangement`), null until the first one. */
  axis: GridAxis | null
  /** The view's config as the editor holds it (`viewConfigOf`): the grid is laid out with it before it is saved. */
  config: Record<string, unknown>
  /** Whether a search is on: the cells holding it are tinted. */
  searching: boolean
  readOnly: boolean
  collapsedGroupIds: ReadonlySet<string>
  /** How many records the editor counts as checked: when it clears them (a bulk action), the grid unchecks its rows. */
  checkedCount: number
  onCollapsedGroupChanged: (next: Set<string>) => void
  onFrozenColumnCount: (count: number) => void
  onSelectCell: (selection: GridCellSelection) => void
  onCheckRecords: (recordIds: number[]) => void
  onOpenRecord: (recordId: number) => void
  onOpenRecordContextMenu: (recordId: number, at: { clientX: number; clientY: number }) => void
  onOpenColumnHeaderMenu?: (field: DataField, position: { x: number; y: number }) => void
  /** The add row: a new record, with the group values of `anchorRecordId` when the view groups. */
  onAddRecord: (anchorRecordId: number | null) => void
  onOpenFieldCreator: () => void
  onPreviewAttachments: (items: DataAttachment[], index: number) => void
  onRegenerate?: (recordId: number, field: DataField) => void
  onRunButton: (recordId: number, field: DataField) => void
  onOpenLink: (recordId: number, field: DataField) => void
  onUndo: () => void
  onRedo: () => void
}

/** The grid as the page holds it while the editor shows a table view. */
type Mounted = { view: GridCanvas; source: ArrangedSource; interaction: GridInteraction; editing: GridEditing; search: SearchHighlight }

/** The kit's brand as `#rrggbb` when the host keeps its own primary color (`theme.brand: "host"`), else null: the engine's theme has the kit's. */
function hostBrand(): string | null {
  const root = document.documentElement
  if (root.dataset.brand !== 'host') return null
  const hsl = getComputedStyle(root).getPropertyValue('--primary').trim().split(/\s+/).map((part) => Number.parseFloat(part))
  if (hsl.length < 3 || hsl.some((part) => !Number.isFinite(part))) return null
  const [h, s, l] = [hsl[0], hsl[1] / 100, hsl[2] / 100]
  const a = s * Math.min(l, 1 - l)
  const channel = (n: number) => {
    const k = (n + h / 30) % 12
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))))
  }
  return `#${[channel(0), channel(8), channel(4)].map((value) => value.toString(16).padStart(2, '0')).join('')}`
}

/**
 * The table view's grid (grid plan phase 5): the engine lays out the editor's arrangement and
 * draws it in bands (`canvas/`); the page plays them, and holds the selection, the editors and the
 * drags. The records stay in the engine: a write reads only the records it touches, and the
 * editor's menus and drawer read the one record they show.
 */
export function GridView(props: GridViewProps) {
  const { i18n } = useTranslation('table')
  const dataSource = useTableDataSource()
  const host = useTableHost()
  const notify = useTableNotify()
  const isDark = useIsDark()
  const theme: 'light' | 'dark' = isDark ? 'dark' : 'light'
  const latest = useRef(props)
  latest.current = props
  const services = useRef({ host, notify })
  services.current = { host, notify }
  const element = useRef<HTMLDivElement | null>(null)
  const mounted = useRef<Mounted | null>(null)
  const [popup, setPopup] = useState<CellPopup | null>(null)
  const configKey = JSON.stringify(props.config)
  const fieldsKey = useMemo(() => JSON.stringify(props.fields.map((field) => [field.id, field.name, field.type, field.order, field.readonly, field.options ?? null, field.ai_config ?? null])), [props.fields])
  const { documentId, tableId, viewId, readOnly } = props

  // The grid of this view: made once per table and view, destroyed with them.
  useEffect(() => {
    const host = element.current
    if (!host) return
    const at = () => latest.current
    let ownWrites = 0
    const source = arrangedSource(dataSource, documentId, { axis: null, tableId, viewId, config: {}, zone: timeZoneChanges(), theme: 'light', brand: null })
    const view = new GridCanvas(host, source, { fontsBase: new URL('fonts/', document.baseURI).href })
    const data = editorData({
      dataSource,
      documentId,
      tableId,
      viewId,
      fields: () => at().fields,
      setFrozen: (count) => at().onFrozenColumnCount(count),
      upload: services.current.host.uploadAttachment ? (file) => services.current.host.uploadAttachment!({ documentId, tableId, file }) : undefined,
      writing: async (write) => {
        ownWrites += 1
        try {
          return await write()
        } finally {
          ownWrites -= 1
        }
      },
    })
    const field = (fieldId: number) => at().fields.find((item) => item.id === fieldId)
    const mount: { current: Mounted | null } = { current: null }
    const editing = new GridEditing(
      data,
      {
        onPopup: setPopup,
        onPreviewAttachments: ({ items, index }) => at().onPreviewAttachments(items, index),
        onButton: ({ recordId, fieldId }) => {
          const target = field(fieldId)
          if (target) at().onRunButton(recordId, target)
        },
        onOpenLink: ({ recordId, fieldId }) => {
          const target = field(fieldId)
          if (target) at().onOpenLink(recordId, target)
        },
        onRegenerate: at().onRegenerate
          ? ({ recordId, fieldId }) => {
              const target = field(fieldId)
              if (target) at().onRegenerate?.(recordId, target)
            }
          : undefined,
        onWritten: (written) => followWrite(written),
        onError: (error) => services.current.notify.error(error instanceof Error ? error.message : String(error)),
        resolveFileURL: (url) => resolveTableFileURL(services.current.host, url),
      },
      { readOnly },
    )
    const interaction = new GridInteraction(view, {
      ...editing.gridHandlers,
      onUndo: () => at().onUndo(),
      onRedo: () => at().onRedo(),
      onContextMenu: ({ recordIds, cell, clientX, clientY }) => {
        const recordId = (cell ? interaction.rows?.recordId(cell.row) : null) ?? recordIds[0]
        if (recordId != null) at().onOpenRecordContextMenu(recordId, { clientX, clientY })
      },
      onColumnMenu: ({ fieldId, anchor }) => {
        const target = field(fieldId)
        if (target) at().onOpenColumnHeaderMenu?.(target, { x: anchor.x + anchor.width, y: anchor.y + anchor.height })
      },
      onExpandRecord: (recordId) => at().onOpenRecord(recordId),
      onAppendRecord: ({ anchorRecordId }) => at().onAddRecord(anchorRecordId),
      onAppendField: () => at().onOpenFieldCreator(),
      onToggleGroup: ({ groupId, collapsed }) => {
        const next = new Set(at().collapsedGroupIds)
        if (collapsed) next.add(groupId)
        else next.delete(groupId)
        at().onCollapsedGroupChanged(next)
      },
    })
    editing.attach(interaction)
    const search = new SearchHighlight(interaction, (params) => dataSource.grid.hits(params))
    mount.current = { view, source, interaction, editing, search }
    mounted.current = mount.current

    // A write of the grid's own: its records' bands are fetched again (a new axis, if the write
    // moved a record, comes through the editor's arrangement); a write to the columns lays out again.
    async function followWrite(written: Written) {
      if (written.kind === 'layout') return relayout(mount.current)
      const axes = view.currentAxes
      const rows = view.opened?.rows
      if (!axes || !rows) return
      view.cache.markStale(showsRecords(rows, axes.bandRows, written.records ? new Set(written.records) : null))
      view.invalidate()
      search.refresh()
    }
    // Any other change to the document (the editor's own writes elsewhere, undo, an agent, a
    // co-editor): every band is fetched again, drawn as it was until then.
    const unsubscribe = dataSource.grid.subscribe(() => {
      if (ownWrites > 0 || !view.currentAxes) return
      view.cache.markStale(() => true)
      view.invalidate()
      search.refresh()
    })
    // The selection: the active cell and the checked records go to the editor.
    const unwatch = interaction.onChange((state) => {
      const rows = interaction.rows
      const axes = view.currentAxes
      const active = state.active
      const recordId = active && rows ? rows.recordId(active.row) : null
      const fieldId = active ? axes?.columns[active.col]?.fieldId : null
      const name = fieldId != null ? field(fieldId)?.name : undefined
      at().onSelectCell(recordId != null && name ? { recordId, fieldName: name } : null)
      at().onCheckRecords(rows && state.selection.kind === 'rows' ? selectedRecordIds(state.selection, rows) : [])
    })
    // Cells being generated by an AI field turn a spinner.
    const generating = () => {
      const cells = aiCellStore.pendingKeys().flatMap((key) => {
        const [table, recordId, fieldId] = key.split(':').map(Number)
        return table === tableId ? [{ recordId, fieldId }] : []
      })
      editing.setGenerating(cells)
    }
    const unsubscribeAI = aiCellStore.subscribeAll(generating)
    generating()
    return () => {
      unsubscribe()
      unwatch()
      unsubscribeAI()
      search.destroy()
      editing.destroy()
      interaction.destroy()
      view.destroy()
      view.element.remove()
      mounted.current = null
      setPopup(null)
    }
  }, [dataSource, documentId, tableId, viewId, readOnly])

  // Lays the editor's latest axis out with its settings, theme and fields.
  const axisBytes = props.axis?.bytes ?? null
  useEffect(() => {
    const mount = mounted.current
    if (!mount || !axisBytes) return
    mount.source.update({ axis: axisBytes, config: JSON.parse(configKey) as Record<string, unknown>, theme, brand: hostBrand() })
    mount.search.setAxis(latest.current.searching && latest.current.axis ? latest.current.axis : null)
    void relayout(mount)
  }, [axisBytes, configKey, theme, fieldsKey, props.searching, dataSource, documentId, tableId, viewId, readOnly])

  // The editor cleared its checked records (a bulk action ran): the grid's checked rows go too.
  useEffect(() => {
    const mount = mounted.current
    if (mount && props.checkedCount === 0 && mount.interaction.selection.kind === 'rows') mount.interaction.setSelection(NO_SELECTION, mount.interaction.active)
  }, [props.checkedCount])

  return (
    <div className="data-inline-grid-canvas relative flex-1 min-h-0">
      <div ref={element} className="absolute inset-0" data-testid="table-grid" />
      <GridCellPopup popup={popup} locale={i18n.language} />
    </div>
  )
}

/** Each grid's layouts, one after another (a later one starts from the source's latest target). */
const layouts = new WeakMap<Mounted, Promise<void>>()

/** Lays the source's target out (the first time, loading Inter with it); bands of an arrangement laid out again go. */
function relayout(mount: Mounted | null): Promise<void> {
  if (!mount) return Promise.resolve()
  const next = (layouts.get(mount) ?? Promise.resolve())
    .then(() => layOut(mount))
    .catch((error: unknown) => console.error('grid: the layout failed', error))
  layouts.set(mount, next)
  return next
}

async function layOut({ view, source }: Mounted) {
  if (!view.element.isConnected) return
  if (!view.opened) return view.open()
  const opened = await source.open()
  if (!view.element.isConnected) return
  if (opened.axes.epoch === view.currentAxes?.epoch) view.cache.clear()
  view.setAxes(opened)
}
