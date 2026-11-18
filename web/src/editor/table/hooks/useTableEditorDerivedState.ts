import { useMemo, type CSSProperties } from 'react'
import type { DataField, DataRecord } from '../../types/dataDocument'
import { gridGroupPoints, type GridAxis } from '../lib/gridAxis'
import {
  buildGroupCollection,
  displayValue,
  filterTreeHasActiveCondition,
  gridAddColumnWidth,
  gridFieldColumnWidth,
  gridRecordPageSize,
  gridRowHeight,
  gridRowNumberColumnWidth,
  maxVirtualGridCanvasHeight,
  normalizeChoiceList,
  type DataInlineFilterGroup,
  type DataInlineGroupItem,
  type DataInlineSortItem,
  type SortDirection,
} from '../lib/tableEditorShared'

type UseTableEditorDerivedStateArgs = {
  /** The engine's arrangement of the view (`useTableArrangement`); null until it arrives. */
  axis: GridAxis | null
  fields: DataField[]
  hiddenFieldNames: Set<string>
  coverFieldName: string
  filterFieldName: string
  filterOperator: string
  filterTree?: DataInlineFilterGroup | null
  filterValue: string
  groupFieldName: string
  groupItems?: DataInlineGroupItem[]
  query: string
  recordContextMenuRecordId: number | null
  recordOffset: number
  records: DataRecord[]
  recordTotal: number
  selectedRecordIds: Set<number>
  sortDirection: SortDirection
  sortFieldName: string
  sortItems?: DataInlineSortItem[]
  viewMode: string
}

export function useTableEditorDerivedState({
  axis,
  fields,
  hiddenFieldNames,
  coverFieldName,
  filterFieldName,
  filterOperator,
  filterTree,
  filterValue,
  groupFieldName,
  groupItems,
  query,
  recordContextMenuRecordId,
  recordOffset,
  records,
  recordTotal,
  selectedRecordIds,
  sortDirection,
  sortFieldName,
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
  const effectiveSortItems = useMemo<DataInlineSortItem[]>(() => {
    if (sortItems && sortItems.length > 0) {
      return sortItems.filter((item) => fields.some((field) => field.name === item.field_name))
    }
    if (sortFieldName) {
      return [{ id: `legacy:${sortFieldName}`, field_name: sortFieldName, direction: sortDirection }]
    }
    return []
  }, [fields, sortItems, sortFieldName, sortDirection])
  const effectiveGroupItems = useMemo<DataInlineGroupItem[]>(() => {
    if (groupItems && groupItems.length > 0) {
      return groupItems.filter((item) => fields.some((field) => field.name === item.field_name))
    }
    if (groupFieldName) {
      return [{ id: `legacy:${groupFieldName}`, field_name: groupFieldName, direction: 'asc' }]
    }
    return []
  }, [fields, groupItems, groupFieldName])
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
  const filterTreeActive = useMemo(() => filterTreeHasActiveCondition(filterTree ?? null), [filterTree])
  // The view's records in its order: the engine filters, sorts, groups and searches them
  // (kition-table-arrange); the editor keeps the records' contents.
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
  const searchHits = useMemo(() => {
    if (!axis) return []
    return axis.hits.map(([row, fieldId]) => ({ recordId: axis.ids[row], fieldId }))
  }, [axis])
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
  const groupPoints = useMemo(() => {
    if (!effectiveGroupItems.length) return null
    return axis ? gridGroupPoints(axis) : []
  }, [effectiveGroupItems, axis])

  const groupCollection = useMemo(() => {
    if (!effectiveGroupItems.length) return null
    return buildGroupCollection(effectiveGroupItems, fields)
  }, [effectiveGroupItems, fields])
  const allVisibleSelected = useMemo(() => {
    return sortedAndFilteredRecords.length > 0 && sortedAndFilteredRecords.every((record) => selectedRecordIds.has(record.id))
  }, [selectedRecordIds, sortedAndFilteredRecords])
  const contextMenuRecord = useMemo(() => {
    return recordContextMenuRecordId ? records.find((record) => record.id === recordContextMenuRecordId) || null : null
  }, [recordContextMenuRecordId, records])
  const canReorderRows = viewMode === 'grid' && effectiveSortItems.length === 0 && effectiveGroupItems.length === 0
  const canMoveKanbanCards = Boolean(groupField && !groupField.readonly && groupableFields.some((field) => field.name === groupField.name))
  const isFilterActive = filterTreeActive || Boolean(filterFieldName && (filterOperator === 'empty' || filterOperator === 'not_empty' || filterValue.trim()))
  const canUseVirtualGrid = viewMode === 'grid' && effectiveGroupItems.length === 0 && effectiveSortItems.length === 0 && !query.trim() && !isFilterActive
  const gridTotalRows = canUseVirtualGrid ? recordTotal : sortedAndFilteredRecords.length
  const renderedGridRecords = canUseVirtualGrid ? records : sortedAndFilteredRecords
  const gridVirtualCanvasHeight = canUseVirtualGrid ? Math.min(recordTotal * gridRowHeight, maxVirtualGridCanvasHeight) : 0
  const gridTableMinWidth = gridRowNumberColumnWidth + visibleFields.length * gridFieldColumnWidth + gridAddColumnWidth
  const gridTableStyle = useMemo(() => {
    return { '--data-inline-grid-min-width': `${gridTableMinWidth}px` } as CSSProperties
  }, [gridTableMinWidth])
  const gridVirtualBlockHeight = records.length * gridRowHeight
  const gridVirtualMaxPageOffset = Math.floor(Math.max(0, recordTotal - gridRecordPageSize) / gridRecordPageSize) * gridRecordPageSize
  const gridVirtualBlockTop = canUseVirtualGrid && recordTotal > gridRecordPageSize
    ? Math.round((recordOffset / Math.max(1, gridVirtualMaxPageOffset)) * Math.max(0, gridVirtualCanvasHeight - gridVirtualBlockHeight))
    : 0
  const numericSummaries = useMemo(() => {
    return visibleFields
      .filter((field) => field.type === 'number')
      .map((field) => {
        const values = sortedAndFilteredRecords
          .map((record) => Number(record.values?.[field.name]))
          .filter((value) => Number.isFinite(value))
        const sum = values.reduce((total, value) => total + value, 0)
        const average = values.length ? sum / values.length : 0
        return { field, count: values.length, sum, average }
      })
      .filter((item) => item.count > 0)
      .slice(0, 3)
  }, [sortedAndFilteredRecords, visibleFields])

  const groupFieldNames = useMemo(
    () => effectiveGroupItems.map((item) => item.field_name),
    [effectiveGroupItems],
  )

  return {
    allVisibleSelected,
    canMoveKanbanCards,
    canReorderRows,
    canUseVirtualGrid,
    contextMenuRecord,
    coverField,
    coverFields,
    dateField,
    gridTableStyle,
    gridTotalRows,
    gridVirtualBlockTop,
    gridVirtualCanvasHeight,
    groupCollection,
    groupField,
    groupFieldNames,
    groupPoints,
    groupedRecords,
    groupableFields,
    numericSummaries,
    primaryField,
    renderedGridRecords,
    searchHits,
    sortedAndFilteredRecords,
    visibleFields,
  }
}
