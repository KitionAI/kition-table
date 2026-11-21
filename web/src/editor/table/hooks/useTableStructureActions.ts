import type { Dispatch, RefObject, SetStateAction } from 'react'
import { useTranslation } from 'react-i18next'
import { useTableDataSource } from '../ports/TableDataSource'
import { useTableHost } from '../ports/TableHost'
import type {
  DataDocument,
  DataField,
  DataFieldOptions,
  DataFieldSeed,
  DataFieldType,
  DataRecord,
  DataTable,
  DataView,
  DataViewSeed,
} from '../../types/dataDocument'
import {
  getViewTypeLabelKey,
  nextViewTitle,
  normalizeLegacyViewTitle,
  type DataInlineViewMode,
} from '../lib/tableEditorShared'
import type { TableAction } from '../lib/tableActions'
import { useConfirm } from '@kition/sdk/editor'

type UseTableStructureActionsArgs = {
  documentPath: string
  document: DataDocument | null
  activeTable: DataTable | null
  tableViews: DataView[]
  fields: DataField[]
  /** Every record of the active table (the grid holds none of them: this reads them when asked). */
  allRecords: () => Promise<DataRecord[]>
  newFieldTitle: string
  newFieldType: DataFieldType
  importInputRef: RefObject<HTMLInputElement | null>
  setBusy: Dispatch<SetStateAction<boolean>>
  setError: Dispatch<SetStateAction<string>>
  setDocument: Dispatch<SetStateAction<DataDocument | null>>
  setRecords: Dispatch<SetStateAction<DataRecord[]>>
  setEditingField: Dispatch<SetStateAction<DataField | null>>
  setNewFieldTitle: Dispatch<SetStateAction<string>>
  setViewCreateOpen: Dispatch<SetStateAction<boolean>>
  setActiveViewId: Dispatch<SetStateAction<number | null>>
  refreshDocument: (preferredTableId?: number | null, preferredViewId?: number | null) => Promise<void>
  loadRecords: () => Promise<void>
  setStatus: (message: string) => void
  copyTextToClipboard: (text: string, successMessage: string) => Promise<void>
  /** Refreshes the undo/redo toolbar state after a server write; optional so existing callers and tests keep working untouched. */
  refreshHistoryState?: () => void
}

export function useTableStructureActions({
  documentPath,
  document,
  activeTable,
  tableViews,
  fields,
  allRecords,
  newFieldTitle,
  newFieldType,
  importInputRef,
  setBusy,
  setError,
  setDocument,
  setRecords,
  setEditingField,
  setNewFieldTitle,
  setViewCreateOpen,
  setActiveViewId,
  refreshDocument,
  loadRecords,
  setStatus,
  copyTextToClipboard,
  refreshHistoryState,
}: UseTableStructureActionsArgs) {
  const confirm = useConfirm()
  const dataSource = useTableDataSource()
  const { exportTable, notify } = useTableHost()
  const { t } = useTranslation('table')

  function makeDuplicateFieldSeed(field: DataField): DataFieldSeed {
    return {
      title: field.title,
      name: field.name,
      type: field.type,
      primary: field.is_primary,
      required: false,
      readonly: field.readonly,
      options: field.options,
      formula: field.formula,
    }
  }

  function makeDuplicateViewSeed(view: DataView): DataViewSeed {
    return {
      title: view.title,
      type: view.type,
      config: view.config,
    }
  }

  async function renameActiveTable(nextTitleValue: string) {
    if (!document || !activeTable) return
    const currentTitle = activeTable.title
    const nextTitle = nextTitleValue.trim()
    if (!nextTitle || nextTitle === currentTitle) return
    try {
      const updatedTable = await dataSource.tables.update({ documentId: document.id, tableId: activeTable.id, title: nextTitle })
      setDocument((current) => current ? {
        ...current,
        tables: current.tables?.map((table) => (
          table.id === updatedTable.id ? { ...table, ...updatedTable } : table
        )) || [],
      } : current)
      window.dispatchEvent(new CustomEvent('kition:data-document:table:rename', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, newName: updatedTable.title },
      }))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to save table name')
    }
  }

  async function duplicateActiveTable() {
    if (!document || !activeTable) return
    setBusy(true)
    try {
      const created = await dataSource.tables.create({
        documentId: document.id,
        title: `${activeTable.title} copy`,
        description: activeTable.description,
        fields: fields.map(makeDuplicateFieldSeed),
        views: tableViews.map(makeDuplicateViewSeed),
      })
      const records = await allRecords()
      if (records.length) {
        await Promise.all(records.map((record) => dataSource.records.create({ documentId: document.id, tableId: created.id, values: record.values || {} })))
      }
      window.dispatchEvent(new CustomEvent('kition:data-document:table:create', {
        detail: { vaultPath: documentPath, tableId: created.id },
      }))
      await refreshDocument(created.id)
      setStatus(t('feedback.tableDuplicated'))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to duplicate table')
    } finally {
      setBusy(false)
    }
  }

  /**
   * Writes the table as a new file beside the document through the host: says it exported, naming
   * the file the host wrote, only when the host answers, and shows the host's (or the engine's)
   * error otherwise.
   */
  async function exportActiveTable(format: 'csv' | 'md') {
    if (!activeTable || !exportTable) return
    setBusy(true)
    try {
      const written = await exportTable(format, { id: activeTable.id, title: activeTable.title })
      setStatus(t('feedback.exported', { path: written }))
    } catch (requestError) {
      notify?.error(t('feedback.exportFailed'), { description: requestError instanceof Error ? requestError.message : String(requestError) })
    } finally {
      setBusy(false)
    }
  }

  async function deleteActiveTable() {
    if (!document || !activeTable) return
    if (!(await confirm({ message: `Delete table "${activeTable.title}"?`, variant: 'danger' }))) return
    setBusy(true)
    try {
      await dataSource.tables.delete({ documentId: document.id, tableId: activeTable.id })
      window.dispatchEvent(new CustomEvent('kition:data-document:table:delete', {
        detail: { vaultPath: documentPath, tableId: activeTable.id },
      }))
      await refreshDocument(null)
      setRecords([])
      setStatus(t('feedback.tableDeleted'))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to delete table')
    } finally {
      setBusy(false)
    }
  }

  function runTableAction(action: TableAction) {
    if (action === 'rename') {
      const nextTitle = window.prompt(t('toolbar.renameTablePrompt'), activeTable?.title || '')
      if (nextTitle !== null) void renameActiveTable(nextTitle)
      return
    }
    if (action === 'duplicate') {
      void duplicateActiveTable()
      return
    }
    if (action === 'export_csv' || action === 'export_markdown') {
      void exportActiveTable(action === 'export_csv' ? 'csv' : 'md')
      return
    }
    if (action === 'import_csv') {
      importInputRef.current?.click()
      return
    }
    if (action === 'copy_api') {
      if (document && activeTable) {
        void copyTextToClipboard(`/v1/data-documents/${document.id}/tables/${activeTable.id}`, 'API path copied')
      }
      return
    }
    if (action === 'history') {
      setStatus(t('feedback.historyUnavailable'))
      return
    }
    if (action === 'share') {
      const shareText = document?.path || documentPath
      void copyTextToClipboard(shareText, 'Table link copied')
      return
    }
    if (action === 'delete') {
      void deleteActiveTable()
    }
  }

  async function createView(type: DataInlineViewMode) {
    if (!document || !activeTable) return
    setBusy(true)
    setViewCreateOpen(false)
    try {
      const view = await dataSource.views.create({
        documentId: document.id,
        tableId: activeTable.id,
        title: nextViewTitle(type, tableViews),
        type,
      })
      setDocument((current) => current ? {
        ...current,
        tables: current.tables?.map((table) => table.id === activeTable.id ? {
          ...table,
          views: [...(table.views || []), view].sort((first, second) => first.order - second.order || first.id - second.id),
        } : table),
      } : current)
      setActiveViewId(view.id)
      window.dispatchEvent(new CustomEvent('kition:data-document:view:upsert', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, viewId: view.id },
      }))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to create view')
    } finally {
      setBusy(false)
    }
  }

  async function renameView(viewId: number, nextTitleValue: string) {
    if (!document || !activeTable) return
    const view = tableViews.find((item) => item.id === viewId)
    const nextTitle = nextTitleValue.trim()
    if (!view || !nextTitle || nextTitle === view.title) return
    try {
      const updated = await dataSource.views.update({ documentId: document.id, tableId: activeTable.id, viewId, title: nextTitle })
      setDocument((current) => current ? {
        ...current,
        tables: current.tables?.map((table) => table.id === activeTable.id ? {
          ...table,
          views: table.views?.map((item) => item.id === viewId ? updated : item),
        } : table),
      } : current)
      window.dispatchEvent(new CustomEvent('kition:data-document:view:upsert', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, viewId },
      }))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to rename view')
    }
  }

  async function duplicateView(viewId: number) {
    if (!document || !activeTable) return
    const view = tableViews.find((item) => item.id === viewId)
    if (!view) return
    const sourceTitle = normalizeLegacyViewTitle(view) || t(getViewTypeLabelKey(view.type))
    const baseTitle = t('viewMenu.copyTitle', { title: sourceTitle })
    const existingTitles = new Set(tableViews.map((item) => item.title.trim().toLowerCase()))
    let nextTitle = baseTitle
    let copyIndex = 2
    while (existingTitles.has(nextTitle.toLowerCase())) {
      nextTitle = `${baseTitle} ${copyIndex++}`
    }

    setBusy(true)
    try {
      const sourceFields = await dataSource.views.listFields({ documentId: document.id, tableId: activeTable.id, viewId: view.id })
        .catch(() => ({ items: [] }))
      const created = await dataSource.views.create({
        documentId: document.id,
        tableId: activeTable.id,
        title: nextTitle,
        type: view.type,
        config: view.config ? structuredClone(view.config) : view.config,
      })
      await Promise.allSettled(sourceFields.items.map((field) => dataSource.views.patchField({
        documentId: document.id,
        tableId: activeTable.id,
        viewId: created.id,
        fieldId: field.field_id,
        visible: field.visible,
        width: field.width,
        position: field.position,
        frozen: field.frozen,
      })))
      setDocument((current) => current ? {
        ...current,
        tables: current.tables?.map((table) => table.id === activeTable.id ? {
          ...table,
          views: [...(table.views || []), created]
            .sort((first, second) => first.order - second.order || first.id - second.id),
        } : table),
      } : current)
      setActiveViewId(created.id)
      window.dispatchEvent(new CustomEvent('kition:data-document:view:upsert', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, viewId: created.id },
      }))
      setStatus(t('feedback.viewDuplicated'))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to duplicate view')
    } finally {
      setBusy(false)
    }
  }

  async function deleteView(viewId: number) {
    if (!document || !activeTable || tableViews.length <= 1) return
    const viewIndex = tableViews.findIndex((item) => item.id === viewId)
    const view = tableViews[viewIndex]
    if (!view || view.locked) return
    const viewTitle = normalizeLegacyViewTitle(view) || t(getViewTypeLabelKey(view.type))
    if (!(await confirm({
      message: t('viewMenu.deleteConfirm', { title: viewTitle }),
      variant: 'danger',
    }))) return

    setBusy(true)
    try {
      await dataSource.views.delete({ documentId: document.id, tableId: activeTable.id, viewId: view.id })
      const remainingViews = tableViews.filter((item) => item.id !== view.id)
      const nextView = remainingViews[Math.min(viewIndex, remainingViews.length - 1)] || remainingViews[0]
      setDocument((current) => current ? {
        ...current,
        tables: current.tables?.map((table) => table.id === activeTable.id ? {
          ...table,
          views: table.views?.filter((item) => item.id !== view.id),
        } : table),
      } : current)
      setActiveViewId(nextView?.id ?? null)
      window.dispatchEvent(new CustomEvent('kition:data-document:view:delete', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, viewId: view.id },
      }))
      setStatus(t('feedback.viewDeleted'))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to delete view')
    } finally {
      setBusy(false)
    }
  }

  async function addField() {
    if (!document || !activeTable || !newFieldTitle.trim()) return
    setBusy(true)
    try {
      await dataSource.fields.create({
        documentId: document.id,
        tableId: activeTable.id,
        title: newFieldTitle.trim(),
        type: newFieldType,
        required: false,
        readonly: newFieldType === 'formula',
        options: newFieldType === 'single_select' || newFieldType === 'multi_select'
          ? { choices: ['Not started', 'In progress', 'Done'] }
          : undefined,
      })
      setNewFieldTitle('')
      await refreshDocument(activeTable.id)
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to add property')
    } finally {
      setBusy(false)
    }
  }

  async function addDefaultField() {
    if (!document || !activeTable) return
    setBusy(true)
    try {
      const existingTitles = new Set(fields.map((field) => field.title))
      let title = 'New property'
      let counter = 2
      while (existingTitles.has(title)) {
        title = `New property ${counter++}`
      }
      await dataSource.fields.create({
        documentId: document.id,
        tableId: activeTable.id,
        title,
        type: 'text',
        required: false,
      })
      await refreshDocument(activeTable.id)
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to add property')
    } finally {
      setBusy(false)
    }
  }

  async function saveField(field: DataField, updates: {
    title?: string
    type?: DataFieldType
    readonly?: boolean
    options?: DataFieldOptions | null
    formula?: string
  }) {
    if (!document || !activeTable) return
    try {
      const updated = await dataSource.fields.update({
        documentId: document.id,
        tableId: activeTable.id,
        fieldId: field.id,
        ...updates,
        required: false,
      })
      setDocument((current) => current ? {
        ...current,
        tables: current.tables?.map((table) => table.id === activeTable.id ? {
          ...table,
          fields: table.fields?.map((item) => item.id === field.id ? updated : item),
        } : table),
      } : current)
      setEditingField(updated)
      window.dispatchEvent(new CustomEvent('kition:data-document:field:upsert', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, fieldId: field.id },
      }))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to save field')
    }
  }

  async function removeField(field: DataField) {
    if (!document || !activeTable || field.is_primary) return
    setBusy(true)
    try {
      await dataSource.fields.delete({ documentId: document.id, tableId: activeTable.id, fieldId: field.id })
      setEditingField(null)
      await refreshDocument(activeTable.id)
      void loadRecords()
      window.dispatchEvent(new CustomEvent('kition:data-document:field:delete', {
        detail: { vaultPath: documentPath, tableId: activeTable.id, fieldId: field.id },
      }))
      refreshHistoryState?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to delete field')
    } finally {
      setBusy(false)
    }
  }

  return {
    addField,
    addDefaultField,
    createView,
    deleteView,
    duplicateView,
    renameView,
    removeField,
    runTableAction,
    saveField,
  }
}
