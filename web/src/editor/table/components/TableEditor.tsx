import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { FrameChrome } from '@kition/sdk/editor'
import type {
  DataField,
  DataFieldType,
  DataDocument,
  DataRecord,
  DataRecordValue,
  DataTable,
  DataView,
  LinkRef,
} from '../../types/dataDocument'
import { useTableDataSource } from '../ports/TableDataSource'
import { useTableHost, useTableNotify } from '../ports/TableHost'
import { TableBlobScope, useTableBlobDocument } from '../ports/TableBlobScope'
import {
  DATA_TABLE_ACTION_EVENT,
  type TableActionEventDetail,
} from '../lib/tableActions'
import { buttonConfig } from '../lib/buttonAction'
import { buildTableLiveChangeHandlers } from '../lib/tableLiveChangeDispatch'

import { TableColumnHeaderMenu } from './TableColumnHeaderMenu'
import { TableEditorContent } from './TableEditorContent'
import { TableEditorFooter } from './TableEditorFooter'
import { AttachmentPreviewModal } from './AttachmentPreviewModal'
import { TableEditorToolbar } from './TableEditorToolbar'
import { TableFileImportDialog } from './TableFileImportDialog'
import { FieldConfigPanel } from './TableFieldConfigPanel'
import { TableRecordContextMenu } from './TableRecordContextMenu'
import { RecordDetailDrawer } from './TableRecordDetailDrawer'
import { useApplyAICellPatches } from '../hooks/useApplyAICellPatches'
import { useTableColumnHeaderActions } from '../hooks/useTableColumnHeaderActions'
import { useTableEditorActions } from '../hooks/useTableEditorActions'
import { useGridCollapsedGroup } from '../hooks/useGridCollapsedGroup'
import { useTableEditorData } from '../hooks/useTableEditorData'
import { useTableEditorDerivedState } from '../hooks/useTableEditorDerivedState'
import { useGridSummary } from '../hooks/useGridSummary'
import { useTableEditorUiState } from '../hooks/useTableEditorUiState'
import { useTableHistory } from '../hooks/useTableHistory'
import { useTableLiveChanges } from '../ports/useTableLiveChanges'
import { useTableViewState, viewConfigOf } from '../hooks/useTableViewState'
import { useTableArrangement } from '../hooks/useTableArrangement'

function TableEditorBody({
  agentOpen: controlledAgentOpen,
  aiCellPatches,
  documentPath,
  markerContent,
  pinnedTableId,
  footerEnd,
  onOpenDocument,
  onAgentContextChange,
  onAgentOpenChange,
  readOnly = false,
}: {
  agentOpen?: boolean
  /**
   * Optional list of AI-fill cell mutations to apply to the local
   * `records` state as new entries arrive. Used by the scenario build
   * flow (see `ScenarioBuildPage`) to stream `cell.ai.filled` events
   * straight into the visible grid without a refetch. The list is
   * append-only: callers should pass the same reference order across
   * renders so we can track an "applied" highwater mark via the patch
   * count. Each entry's `fieldId` is resolved against the active
   * table's fields to find the matching `field.name`, which is the
   * key inside `record.values`. Unknown record/field ids are silently
   * skipped (the orchestrator may emit an event a tick before the
   * frontend has refetched the corresponding row).
   */
  aiCellPatches?: ReadonlyArray<{
    recordId: number
    fieldId: number
    value: DataRecordValue
  }>
  documentPath: string
  markerContent: string
  pinnedTableId?: number
  /** Drawn at the footer's end: the host's document state (unsaved, edited by the agent) when it draws no status bar. */
  footerEnd?: ReactNode
  onOpenDocument?: (path: string) => void
  onAgentContextChange?: (context: {
    documentPath: string
    activeDocument: DataDocument | null
    activeTable: DataTable | null
    onTableChanged?: () => Promise<void> | void
  }) => void
  onAgentOpenChange?: (open: boolean) => void
  /** A co-edited table this person may only view: no cell, record, field, or view changes. */
  readOnly?: boolean
}) {
  const { t } = useTranslation('table')
  const dataSource = useTableDataSource()
  const host = useTableHost()
  const notify = useTableNotify()
  const importInputRef = useRef<HTMLInputElement | null>(null)
  const [importFile, setImportFile] = useState<File | null>(null)
  const {
    clearKanbanDrag,
    closeColumnHeaderMenu,
    columnHeaderMenu,
    editingField,
    imagePreview,
    kanbanDragOverGroup,
    kanbanDragOverRecordId,
    kanbanDraggingRecordId,
    openColumnHeaderMenu,
    openRecordContextMenu,
    recordContextMenu,
    resetSelectedRecordIds,
    selectedGridCell,
    selectedRecord,
    selectedRecordIds,
    setEditingField,
    setImagePreview,
    setKanbanDragOverGroup,
    setKanbanDragOverRecordId,
    setKanbanDraggingRecordId,
    setRecordContextMenu,
    setSelectedGridCell,
    setSelectedRecord,
    setSelectedRecordIds,
    setToolbarPanel,
    toolbarPanel,
  } = useTableEditorUiState()
  const {
    document,
    setDocument,
    activeTableId,
    activeViewId,
    setActiveViewId,
    records,
    setRecords,
    loading,
    error,
    setError,
    refreshDocument,
    loadRecords: loadAllRecords,
  } = useTableEditorData({
    documentPath,
    markerContent,
    pinnedTableId,
    onRecordsLoaded: resetSelectedRecordIds,
  })
  // The editor's settings of the shown view, as it holds them (saved or not).
  const [busy, setBusy] = useState(false)
  const [query, setQuery] = useState('')
  const [newFieldTitle, setNewFieldTitle] = useState('')
  const [newFieldType, setNewFieldType] = useState<DataFieldType>('text')
  const [viewCreateOpen, setViewCreateOpen] = useState(false)
  const [uncontrolledAgentOpen, setUncontrolledAgentOpen] = useState(false)
  const agentOpen = typeof controlledAgentOpen === 'boolean'
    ? controlledAgentOpen
    : uncontrolledAgentOpen

  const activeTable = useMemo<DataTable | null>(() => {
    return (
      document?.tables?.find((table) => table.id === activeTableId) ||
      document?.tables?.[0] ||
      null
    )
  }, [document, activeTableId])
  const tableViews = useMemo(
    () => activeTable?.views || [],
    [activeTable?.views],
  )
  const activeView = useMemo<DataView | null>(() => {
    return (
      tableViews.find((view) => view.id === activeViewId) ||
      tableViews[0] ||
      null
    )
  }, [activeViewId, tableViews])
  const {
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
  } = useTableViewState({
    document,
    activeTable,
    activeView,
    tableViews,
    setDocument,
    setActiveViewId,
  })
  const fields = activeTable?.fields || []
  // Stream `cell.ai.filled` patches (from the scenario build flow) into
  // the local records state as soon as the table fields have loaded.
  // No-op when the scenario page doesn't pass `aiCellPatches`.
  useApplyAICellPatches({
    patches: aiCellPatches,
    fields,
    setRecords,
  })
  const { collapsedGroupIds, setCollapsedGroupIds } = useGridCollapsedGroup(activeView?.id ?? null)
  // The view's settings as the editor holds them, saved or not: the engine arranges with these
  // (only the keys that change which records show and their order, so hiding a field or changing
  // the row height arranges nothing again).
  const viewConfig = useMemo(() => viewConfigOf({
    hiddenFieldNames,
    filterTree,
    sortItems,
    groupItems,
    coverFieldName,
    rowHeightKey,
    frozenColumnCount,
  }) as Record<string, unknown>, [hiddenFieldNames, filterTree, sortItems, groupItems, coverFieldName, rowHeightKey, frozenColumnCount])
  const arrangedConfig = useMemo(() => {
    const config = viewConfigOf({
      hiddenFieldNames,
      filterTree,
      sortItems,
      groupItems,
      coverFieldName,
      rowHeightKey,
      frozenColumnCount,
    })
    return {
      filter_tree: config.filter_tree,
      sorts: config.sorts ?? [],
      groups: config.groups ?? [],
    } as Record<string, unknown>
  }, [hiddenFieldNames, filterTree, sortItems, groupItems, coverFieldName, rowHeightKey, frozenColumnCount])
  const arrangedRevision = useMemo(() => ({}), [records, fields])
  const axis = useTableArrangement({
    tableId: activeTable?.id ?? null,
    viewId: activeView?.id ?? null,
    config: arrangedConfig,
    query,
    collapsedGroupIds,
    revision: arrangedRevision,
  })
  const {
    canMoveKanbanCards,
    canReorderRows,
    contextMenuRecord,
    coverField,
    coverFields,
    dateField,
    groupField,
    groupFieldNames,
    groupedRecords,
    groupableFields,
    primaryField,
    shownCount,
    sortedAndFilteredRecords,
    visibleFields,
  } = useTableEditorDerivedState({
    axis,
    fields,
    hiddenFieldNames,
    coverFieldName,
    groupItems,
    recordContextMenuRecordId: recordContextMenu?.recordId ?? null,
    records,
    sortItems,
    viewMode,
  })
  const numericSummaries = useGridSummary(axis, visibleFields)

  // The grid draws the view's records from the engine, so in it the editor never reads the whole
  // table: it holds only the records a menu, the drawer or an action read (`readRecords`), and
  // reads those again where it would read every record. The kanban, calendar and gallery read
  // every record.
  const gridMode = viewMode === 'grid'
  const held = useRef({ gridMode, records })
  held.current = { gridMode, records }
  const readRecords = useCallback(async (ids: number[]): Promise<DataRecord[]> => {
    const tableId = activeTable?.id
    const documentId = document?.id
    if (tableId == null || documentId == null || !ids.length) return []
    const made = (await dataSource.grid.records({ tableId, ids })).map((item): DataRecord => ({
      ...item,
      table_id: tableId,
      document_id: documentId,
      // Not part of what the grid reads, and nothing the editor shows.
      user_id: 0,
      order: 0,
      created_at: '',
      updated_at: '',
    }))
    setRecords((current) => {
      const byId = new Map(current.map((record) => [record.id, record]))
      for (const record of made) byId.set(record.id, record)
      return [...byId.values()]
    })
    return made
  }, [activeTable?.id, document?.id, dataSource, setRecords])
  const loadRecords = useCallback(async () => {
    if (!held.current.gridMode) return loadAllRecords()
    // The records held, read again (gone when they were deleted).
    const ids = held.current.records.map((record) => record.id)
    if (!ids.length) return
    const made = await readRecords(ids)
    const kept = new Set(made.map((record) => record.id))
    setRecords((current) => current.filter((record) => kept.has(record.id)))
  }, [loadAllRecords, readRecords, setRecords])

  /** Every record of the table: what the editor holds outside the grid, else read now (an action that needs them all). */
  async function allRecords(): Promise<DataRecord[]> {
    if (!gridMode) return records
    if (!document || !activeTable) return []
    return (await dataSource.records.list({ documentId: document.id, tableId: activeTable.id })).items
  }

  function setStatus(message: string) {
    notify.success(message)
  }

  async function refreshAgentChanges() {
    await refreshDocument(
      activeTable?.id ?? activeTableId,
      activeView?.id ?? activeViewId,
    )
    await loadRecords()
  }

  const documentId = document?.id ?? null
  useTableBlobDocument(documentId)
  const onApplyHistoryChange = useCallback(() => refreshAgentChanges(), [
    activeTable?.id,
    activeTableId,
    activeView?.id,
    activeViewId,
    refreshDocument,
    loadRecords,
  ])
  const {
    historyBusy,
    refreshHistoryState,
    undo,
    redo,
  } = useTableHistory({
    documentId,
    onApplyChange: onApplyHistoryChange,
  })

  useTableLiveChanges(documentId, buildTableLiveChangeHandlers({
    activeTableId: activeTable?.id ?? activeTableId,
    activeViewId: activeView?.id ?? activeViewId,
    loadRecords: () => void loadRecords(),
    refreshDocument: () => void refreshDocument(activeTable?.id ?? activeTableId, activeView?.id ?? activeViewId),
    refreshHistoryState,
  }))

  useEffect(() => {
    onAgentContextChange?.({
      documentPath,
      activeDocument: document,
      activeTable,
      onTableChanged:
        document && activeTable
          ? refreshAgentChanges
          : undefined,
    })

    return () => {
      onAgentContextChange?.({
        documentPath,
        activeDocument: null,
        activeTable: null,
      })
    }
  }, [
    activeTable,
    document,
    documentPath,
    onAgentContextChange,
  ])

  useEffect(() => {
    if (gridMode) setRecords([])
    else void loadAllRecords()
  }, [document?.id, activeTable?.id, gridMode])

  useEffect(() => {
    setEditingField(null)
  }, [activeTable?.id, setEditingField])

  useEffect(() => {
    const id = `table-sync:${activeTableId ?? 'unknown'}`
    if (!error || !documentId || !activeTableId) {
      notify.dismiss(id)
      return
    }
    notify.error(error, { id })
  }, [error, documentId, activeTableId, notify])
  const {
    addField,
    addDefaultField,
    addRecord,
    copyTextToClipboard,
    copyRecordURL,
    createView,
    deleteView,
    duplicateRecord,
    duplicateView,
    insertRecordsNear,
    moveKanbanRecord,
    openAttachmentPreview,
    renameView,
    removeField,
    removeRecord,
    removeSelectedRecords,
    duplicateSelectedRecords,
    runAIField,
    runAIFieldBatchForRecords,
    runTableAction,
    saveField,
    updateCell,
  } = useTableEditorActions({
    documentPath,
    document,
    activeTable,
    tableViews,
    fields,
    records,
    allRecords,
    groupField,
    canReorderRows,
    canMoveKanbanCards,
    newFieldTitle,
    newFieldType,
    selectedRecordIds,
    importInputRef,
    setBusy,
    setError,
    setDocument,
    setRecords,
    setSelectedRecord,
    setSelectedRecordIds,
    setRecordContextMenu,
    setEditingField,
    setNewFieldTitle,
    setViewCreateOpen,
    setActiveViewId,
    setImagePreview,
    refreshDocument,
    loadRecords,
    refreshHistoryState,
  })

  const columnActions = useTableColumnHeaderActions({
    document,
    activeTable,
    allRecords,
    visibleFields,
    sortItems,
    setBusy,
    setError,
    setEditingField,
    setFilterTree,
    setSortItems,
    setGroupItems,
    setFrozenColumnCount,
    setHiddenFieldNames,
    setToolbarPanel,
    refreshDocument,
    copyTextToClipboard,
    setStatus,
    refreshHistoryState,
  })

  function handleColumnAIBatch(field: DataField) {
    void runAIFieldBatchForRecords(field, axis?.ids ?? [], 'column-menu')
  }

  /** A record the grid names, as the editor holds it or read now. */
  async function gridRecord(recordId: number): Promise<DataRecord | null> {
    return records.find((record) => record.id === recordId) ?? (await readRecords([recordId]))[0] ?? null
  }

  /** A button cell: its link opens, or its workflow runs in the host (without one, nothing happens). */
  function runButton(recordId: number, field: DataField) {
    const { action } = buttonConfig(field, '')
    if (action.kind === 'open_url' && action.url) window.open(action.url, '_blank', 'noopener,noreferrer')
    else if (action.kind === 'run_workflow' && action.workflow_id != null) void host.runWorkflow?.({ workflowId: action.workflow_id, tableId: field.table_id, recordId })
  }

  /** A document link cell opens its document in the host. */
  async function openLink(recordId: number, field: DataField) {
    if (field.type !== 'document_link' || !onOpenDocument) return
    const record = await gridRecord(recordId)
    const path = record?.values?.[field.name]
    if (typeof path === 'string' && path) onOpenDocument(path)
  }

  /** The add row: a record with the group values of the record above it, when the view groups. */
  async function addGridRecord(anchorRecordId: number | null) {
    const anchor = anchorRecordId != null && groupFieldNames.length ? await gridRecord(anchorRecordId) : null
    if (!anchor) return void addRecord()
    const initialValues: Record<string, DataRecordValue> = {}
    for (const name of groupFieldNames) initialValues[name] = (anchor.values?.[name] ?? null) as DataRecordValue
    void addRecord(initialValues)
  }

  useEffect(() => {
    function handleDataTableAction(event: Event) {
      const detail = (event as CustomEvent<TableActionEventDetail>).detail
      if (!detail || detail.documentPath !== documentPath) return
      runTableAction(detail.action)
    }

    window.addEventListener(DATA_TABLE_ACTION_EVENT, handleDataTableAction)
    return () =>
      window.removeEventListener(DATA_TABLE_ACTION_EVENT, handleDataTableAction)
  }, [documentPath, runTableAction])

  function closeRecordContextMenu() {
    setRecordContextMenu(null)
  }

  function setAgentOpen(nextOpen: boolean | ((open: boolean) => boolean)) {
    const resolvedNextOpen = typeof nextOpen === 'function'
      ? nextOpen(agentOpen)
      : nextOpen
    if (typeof controlledAgentOpen !== 'boolean') {
      setUncontrolledAgentOpen(resolvedNextOpen)
    }
    onAgentOpenChange?.(resolvedNextOpen)
  }

  async function copyRecordValuesForChat(record: DataRecord) {
    await copyTextToClipboard(
      JSON.stringify(record.values || {}, null, 2),
      t('editor.recordCopiedForChat'),
    )
  }

  /** `run`, or nothing at all for a table this person may only view. */
  const viewOnly = <F extends (...args: never[]) => unknown>(run: F): F => (
    readOnly ? ((() => undefined) as unknown as F) : run
  )
  const shownFields = readOnly ? fields.map((field) => ({ ...field, readonly: true })) : fields
  const shownVisibleFields = readOnly ? visibleFields.map((field) => ({ ...field, readonly: true })) : visibleFields

  if (loading) {
    return (
      <div
        data-testid="table-editor-skeleton"
        aria-busy="true"
        aria-label={t('editor.loadingTable')}
        className="flex h-full w-full flex-col gap-3 p-4"
      >
        <div className="flex items-center gap-2">
          <span className="skeleton h-7 w-32" />
          <span className="skeleton h-7 w-20" />
          <span className="skeleton ml-auto h-7 w-24" />
        </div>
        <div className="skeleton h-9 w-full" />
        {Array.from({ length: 8 }).map((_, idx) => (
          <div key={idx} className="flex items-center gap-3">
            <span className="skeleton h-5 w-6" />
            <span className="skeleton h-5 flex-[2]" />
            <span className="skeleton h-5 flex-1" />
            <span className="skeleton h-5 flex-1" />
            <span className="skeleton h-5 flex-[1.5]" />
          </div>
        ))}
      </div>
    )
  }

  if (!document || !activeTable) {
    return (
      <div className="data-inline-state">{error || t('editor.tableNotFound')}</div>
    )
  }

  return (
    <div className="data-inline-shell" data-testid="ktable-editor" data-read-only={readOnly ? 'true' : undefined}>
      {host.fileImport ? (
        <input
          ref={importInputRef}
          type="file"
          accept=".csv,.tsv,.xlsx,text/csv,text/tab-separated-values,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
          className="hidden"
          onChange={(event) => {
            setImportFile(event.target.files?.[0] || null)
            event.currentTarget.value = ''
          }}
        />
      ) : null}
      <TableFileImportDialog
        open={Boolean(importFile)}
        file={importFile}
        target={{ kind: 'existing_table', documentId: document.id, table: activeTable }}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setImportFile(null)
        }}
        onCompleted={async (result) => {
          await refreshDocument(activeTable.id)
          await loadRecords()
          notify.success(t('fileImport.completedSummary', {
            fields: result.fields_created + result.fields_updated,
            rows: result.rows_created + result.rows_updated,
          }))
        }}
      />
      <div className="data-inline-editor">
        {/* The views bar and the toolbar, and the footer, are the editor's own bars: Focus (@kition/sdk/editor's display modes) shows the records alone. */}
        <FrameChrome>
        <TableEditorToolbar
          tableViews={tableViews}
          activeViewId={activeView?.id ?? null}
          onSelectView={(viewId) => {
            setActiveViewId(viewId)
            setViewCreateOpen(false)
          }}
          viewCreateOpen={viewCreateOpen}
          onToggleViewCreate={() => setViewCreateOpen((open) => !open)}
          onCloseViewCreate={() => setViewCreateOpen(false)}
          onCreateView={viewOnly((type) => void createView(type))}
          onRenameView={viewOnly((viewId, nextTitle) => void renameView(viewId, nextTitle))}
          onDuplicateView={viewOnly((viewId) => void duplicateView(viewId))}
          onDeleteView={viewOnly((viewId) => void deleteView(viewId))}
          busy={busy || historyBusy || readOnly}
          fields={fields}
          hiddenFieldNames={hiddenFieldNames}
          onResetHiddenFields={viewOnly(() => setHiddenFieldNames(new Set()))}
          onToggleFieldVisibility={viewOnly((fieldName, checked) => {
            setHiddenFieldNames((current) => {
              const next = new Set(current)
              if (checked) next.delete(fieldName)
              else next.add(fieldName)
              return next
            })
          })}
          onEditField={viewOnly(setEditingField)}
          onDuplicateField={viewOnly(columnActions.duplicateField)}
          onDeleteField={viewOnly(removeField)}
          onAddDefaultField={viewOnly(() => void addDefaultField())}
          filterTree={filterTree}
          onFilterTreeChange={viewOnly(setFilterTree)}
          sortItems={sortItems}
          onSortItemsChange={viewOnly(setSortItems)}
          groupableFields={groupableFields}
          groupItems={groupItems}
          onGroupItemsChange={viewOnly(setGroupItems)}
          viewMode={viewMode}
          coverFieldName={coverFieldName}
          coverField={coverField}
          coverFields={coverFields}
          onCoverFieldNameChange={viewOnly(setCoverFieldName)}
          onResetCoverField={viewOnly(() => setCoverFieldName(''))}
          rowHeightKey={rowHeightKey}
          onRowHeightKeyChange={viewOnly(setRowHeightKey)}
          frozenColumnCount={frozenColumnCount}
          onFrozenColumnCountChange={viewOnly(setFrozenColumnCount)}
          toolbarPanel={toolbarPanel}
          onToolbarPanelChange={setToolbarPanel}
          newFieldTitle={newFieldTitle}
          newFieldType={newFieldType}
          onNewFieldTitleChange={setNewFieldTitle}
          onNewFieldTypeChange={setNewFieldType}
          onAddField={viewOnly(() => {
            void addField()
            setToolbarPanel(null)
          })}
          selectedRecordCount={selectedRecordIds.size}
          onRemoveSelectedRecords={viewOnly(() => void removeSelectedRecords())}
          onDuplicateSelectedRecords={viewOnly(() => void duplicateSelectedRecords())}
          agentOpen={agentOpen}
          onToggleAgent={() => setAgentOpen((open) => !open)}
          agentDisabled={!document || !activeTable}
          query={query}
          onQueryChange={setQuery}
          onAddRecord={viewOnly(() => void addRecord())}
        />
        </FrameChrome>

        <TableEditorContent
          tableId={activeTable.id}
          viewMode={viewMode}
          visibleFields={shownVisibleFields}
          groupField={groupField}
          groupedRecords={groupedRecords}
          primaryField={primaryField}
          dateField={dateField}
          coverField={coverField}
          sortedAndFilteredRecords={sortedAndFilteredRecords}
          canMoveKanbanCards={canMoveKanbanCards}
          draggingRecordId={kanbanDraggingRecordId}
          dragOverRecordId={kanbanDragOverRecordId}
          dragOverGroup={kanbanDragOverGroup}
          onDragStart={setKanbanDraggingRecordId}
          onDragEnd={clearKanbanDrag}
          onDragOverRecord={setKanbanDragOverRecordId}
          onDragOverGroup={setKanbanDragOverGroup}
          onMoveKanbanRecord={viewOnly((recordId, group, targetRecordId) =>
            void moveKanbanRecord(recordId, group, targetRecordId)
          )}
          onAddRecord={viewOnly((initialValues) => void addRecord(initialValues))}
          onOpenRecord={setSelectedRecord}
          onOpenDocument={onOpenDocument}
          onPreviewAttachments={openAttachmentPreview}
          onRegenerate={host.ai ? viewOnly((record, field) => void runAIField(record, field)) : undefined}
          grid={{
            documentId: document.id,
            tableId: activeTable.id,
            viewId: activeView?.id ?? null,
            fields: shownFields,
            axis,
            config: viewConfig,
            searching: Boolean(query.trim()),
            readOnly,
            collapsedGroupIds,
            checkedCount: selectedRecordIds.size,
            onCollapsedGroupChanged: setCollapsedGroupIds,
            onFrozenColumnCount: viewOnly(setFrozenColumnCount),
            onSelectCell: setSelectedGridCell,
            onCheckRecords: (ids) => setSelectedRecordIds((current) => (ids.length === current.size && ids.every((id) => current.has(id)) ? current : new Set(ids))),
            onOpenRecord: (recordId) => void gridRecord(recordId).then((record) => record && setSelectedRecord(record)),
            onOpenRecordContextMenu: (recordId, at) =>
              void gridRecord(recordId).then((record) => record && openRecordContextMenu(record, { ...at, preventDefault: () => undefined, stopPropagation: () => undefined })),
            onOpenColumnHeaderMenu: openColumnHeaderMenu,
            onAddRecord: viewOnly((anchorRecordId) => void addGridRecord(anchorRecordId)),
            onOpenFieldCreator: viewOnly(() => void addDefaultField()),
            onPreviewAttachments: openAttachmentPreview,
            onRegenerate: host.ai && !readOnly ? (recordId, field) => void gridRecord(recordId).then((record) => record && runAIField(record, field)) : undefined,
            onRunButton: viewOnly(runButton),
            onOpenLink: (recordId, field) => void openLink(recordId, field),
            onUndo: viewOnly(() => void undo()),
            onRedo: viewOnly(() => void redo()),
          }}
        />
        <TableRecordContextMenu
          menu={
            recordContextMenu
              ? { x: recordContextMenu.x, y: recordContextMenu.y }
              : null
          }
          record={contextMenuRecord}
          busy={busy}
          canInsert={canReorderRows}
          fields={fields}
          selectedFieldName={
            selectedGridCell && contextMenuRecord
            && selectedGridCell.recordId === contextMenuRecord.id
              ? selectedGridCell.fieldName
              : null
          }
          onClose={closeRecordContextMenu}
          onInsertRecords={viewOnly((record, count, position) =>
            insertRecordsNear(record, count, position)
          )}
          onDuplicateRecord={viewOnly(duplicateRecord)}
          onCopyRecordURL={copyRecordURL}
          onOpenRecord={setSelectedRecord}
          onCopyRecordForChat={copyRecordValuesForChat}
          onDeleteRecord={viewOnly(removeRecord)}
          onRegenerateAIField={viewOnly((record, field) => void runAIField(record, field))}
          onStatus={setStatus}
        />
        <TableColumnHeaderMenu
          menu={
            columnHeaderMenu
              ? { x: columnHeaderMenu.x, y: columnHeaderMenu.y }
              : null
          }
          field={
            columnHeaderMenu
              ? fields.find((item) => item.id === columnHeaderMenu.fieldId) ?? null
              : null
          }
          busy={busy}
          onClose={closeColumnHeaderMenu}
          onEditField={viewOnly(setEditingField)}
          onDuplicateField={viewOnly(columnActions.duplicateField)}
          onRunAIBatch={host.ai ? viewOnly(handleColumnAIBatch) : undefined}
          onDownloadAllFiles={columnActions.downloadAllFiles}
          onAddColumnToChat={columnActions.copyColumnValuesForChat}
          onInsertField={viewOnly(columnActions.insertField)}
          onAddFilterForField={viewOnly(columnActions.addFilterForField)}
          onAddSortForField={viewOnly(columnActions.addSortForField)}
          onSetGroupField={viewOnly(columnActions.setGroupField)}
          onFreezeUpTo={viewOnly(columnActions.freezeUpToField)}
          onHideField={viewOnly(columnActions.hideField)}
          onDeleteField={viewOnly(removeField)}
        />
        <FrameChrome>
          <TableEditorFooter
            shownCount={shownCount}
            numericSummaries={numericSummaries}
            selectedRecordCount={selectedRecordIds.size}
            end={footerEnd}
          />
        </FrameChrome>
      </div>

      {editingField ? (
        <FieldConfigPanel
          field={editingField}
          fields={fields}
          busy={busy}
          onClose={() => setEditingField(null)}
          onSave={viewOnly((updates) => saveField(editingField, updates))}
        />
      ) : null}

      {selectedRecord ? (() => {
        // The view's order is the engine's arrangement; a neighbour is read when the drawer moves to it.
        const navList = axis?.ids ?? []
        const navIndex = navList.indexOf(selectedRecord.id)
        const canPrev = navIndex > 0
        const canNext = navIndex >= 0 && navIndex < navList.length - 1
        const moveTo = (recordId: number) => void gridRecord(recordId).then((record) => record && setSelectedRecord(record))
        return (
          <RecordDetailDrawer
            record={
              records.find((record) => record.id === selectedRecord.id) ||
              selectedRecord
            }
            fields={shownFields}
            primaryField={primaryField}
            documentId={document.id}
            tableId={activeTable.id}
            navIndex={navIndex >= 0 ? navIndex : undefined}
            navTotal={navList.length || undefined}
            canNavigatePrev={canPrev}
            canNavigateNext={canNext}
            onNavigatePrev={canPrev ? () => moveTo(navList[navIndex - 1]) : undefined}
            onNavigateNext={canNext ? () => moveTo(navList[navIndex + 1]) : undefined}
            onPreviewAttachments={openAttachmentPreview}
            onOpenDocument={onOpenDocument}
            onClose={() => setSelectedRecord(null)}
            onDelete={viewOnly(() => {
              void removeRecord(selectedRecord.id)
              setSelectedRecord(null)
            })}
            onUpdate={viewOnly((field, value) =>
              void updateCell(selectedRecord, field, value)
            )}
            onReplaceLinks={viewOnly(async (field: DataField, rowKeys: string[]): Promise<LinkRef[] | null> => {
              try {
                const refs = await dataSource.links.replace({
                  documentId: document.id,
                  tableId: activeTable.id,
                  recordId: selectedRecord.id,
                  field_id: field.id,
                  target_row_keys: rowKeys,
                })
                setRecords((items) =>
                  items.map((record) =>
                    record.id === selectedRecord.id
                      ? { ...record, values: { ...(record.values || {}), [field.name]: refs } }
                      : record,
                  ),
                )
                return refs
              } catch (error) {
                console.error('replaceRecordLinks failed', error)
                notify.error(t('editor.replaceLinksFailed'), {
                  description: error instanceof Error ? error.message : String(error),
                })
                return null
              }
            })}
          />
        )
      })() : null}

      <AttachmentPreviewModal
        open={Boolean(imagePreview)}
        items={imagePreview?.items ?? []}
        index={imagePreview?.index ?? 0}
        onClose={() => setImagePreview(null)}
        onIndexChange={(nextIndex) =>
          setImagePreview((current) =>
            current ? { ...current, index: nextIndex } : current,
          )
        }
      />
    </div>
  )
}

type TableEditorProps = Parameters<typeof TableEditorBody>[0]

/**
 * The table editor: the document at `documentPath`, from the data source and host its root
 * provides. Attachments held in the document as blobs resolve and upload through
 * `TableBlobScope`.
 */
export function TableEditor(props: TableEditorProps) {
  return (
    <TableBlobScope>
      <TableEditorBody {...props} />
    </TableBlobScope>
  )
}
