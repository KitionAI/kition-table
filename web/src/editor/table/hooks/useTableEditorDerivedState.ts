import { useMemo } from 'react'
import type { DataField, DataRecord } from '../../types/dataDocument'
import type { GridAxis } from '../lib/gridAxis'
import {
  displayValue,
  normalizeChoiceList,
  type DataInlineGroupItem,
  type DataInlineSortItem,
} from '../lib/tableEditorShared'

type UseTableEditorDerivedStateArgs = {
  /** The engine's arrangement of the view (`useTableArrangement`); null until it arrives. */
  axis: GridAxis | null
  fields: DataField[]
  hiddenFieldNames: Set<string>
  coverFieldName: string
  groupItems?: DataInlineGroupItem[]
  recordContextMenuRecordId: number | null
  records: DataRecord[]
  sortItems?: DataInlineSortItem[]
  viewMode: string
}

export function useTableEditorDerivedState({
  axis,
  fields,
  hiddenFieldNames,
  coverFieldName,
  groupItems,
  recordContextMenuRecordId,
  records,
  sortItems,
  viewMode,
}: UseTableEditorDerivedStateArgs) {
  const visibleFields = useMemo(() => fields.filter((field) => !hiddenFieldNames.has(field.name)), [fields, hiddenFieldNames])
  const primaryField = useMemo(() => fields.find((field) => field.is_primary) || fields[0] || null, [fields])
  const dateField = useMemo(() => fields.find((field) => field.type === 'date' || field.type === 'datetime') || null, [fields])
  const groupableFields = useMemo(() => {
    return fields.filter((field) => field.type === 'single_select' || field.type === 'checkbox' || field.type === 'text')
  }, [fields])
  const coverFields = useMemo(() => {
    return fields.filter((field) => field.type === 'attachment' || field.type === 'url' || field.type === 'text')
  }, [fields])
  const effectiveSortItems = useMemo<DataInlineSortItem[]>(
    () => (sortItems ?? []).filter((item) => fields.some((field) => field.name === item.field_name)),
    [fields, sortItems],
  )
  const effectiveGroupItems = useMemo<DataInlineGroupItem[]>(
    () => (groupItems ?? []).filter((item) => fields.some((field) => field.name === item.field_name)),
    [fields, groupItems],
  )
  const primaryGroupFieldName = effectiveGroupItems[0]?.field_name || ''
  const groupField = useMemo(() => {
    return (
      fields.find((field) => field.name === primaryGroupFieldName) ||
      fields.find((field) => field.type === 'single_select') ||
      primaryField
    )
  }, [fields, primaryGroupFieldName, primaryField])
  const coverField = useMemo(() => {
    return fields.find((field) => field.name === coverFieldName) || fields.find((field) => field.type === 'attachment') || fields.find((field) => field.type === 'url') || null
  }, [coverFieldName, fields])
  // The view's records in its order: the engine filters, sorts, groups and searches them
  // (kition-table-arrange); the editor keeps the records' contents (the kanban, calendar and
  // gallery read them all; the grid draws from the engine and holds only the records it read).
  const recordsById = useMemo(() => new Map(records.map((record) => [record.id, record])), [records])
  const sortedAndFilteredRecords = useMemo(() => {
    if (!axis) return []
    const arranged: DataRecord[] = []
    for (const id of axis.ids) {
      const record = recordsById.get(id)
      if (record) arranged.push(record)
    }
    return arranged
  }, [axis, recordsById])
  // How many records the view shows (the footer's count), from the arrangement.
  const shownCount = axis?.ids.length ?? 0
  const groupedRecords = useMemo(() => {
    const groups = new Map<string, DataRecord[]>()
    if (groupField?.type === 'single_select') {
      normalizeChoiceList(groupField).forEach((choice) => groups.set(choice, []))
    }
    if (groupField?.type === 'checkbox') {
      groups.set('true', [])
      groups.set('false', [])
    }
    sortedAndFilteredRecords.forEach((record) => {
      const key = groupField ? displayValue(record.values?.[groupField.name] ?? null) || 'Empty' : 'All records'
      groups.set(key, [...(groups.get(key) || []), record])
    })
    return Array.from(groups.entries())
  }, [groupField, sortedAndFilteredRecords])
  const contextMenuRecord = useMemo(() => {
    return recordContextMenuRecordId ? records.find((record) => record.id === recordContextMenuRecordId) || null : null
  }, [recordContextMenuRecordId, records])
  const canReorderRows = viewMode === 'grid' && effectiveSortItems.length === 0 && effectiveGroupItems.length === 0
  const canMoveKanbanCards = Boolean(groupField && !groupField.readonly && groupableFields.some((field) => field.name === groupField.name))
  const groupFieldNames = useMemo(
    () => effectiveGroupItems.map((item) => item.field_name),
    [effectiveGroupItems],
  )

  return {
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
  }
}
