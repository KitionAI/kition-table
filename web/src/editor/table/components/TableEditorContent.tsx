import type {
  DataAttachment,
  DataField,
  DataRecord,
  DataRecordValue,
  DataViewType,
} from '../../types/dataDocument'
import { kanbanGroupToValue } from '../lib/tableEditorShared'

import { GridView, type GridViewProps } from './TableEditorGridView'
import {
  CalendarView,
  GalleryView,
  KanbanView,
} from './TableEditorViews'

type TableEditorContentProps = {
  tableId: number
  viewMode: DataViewType
  visibleFields: DataField[]
  groupField: DataField | null
  groupedRecords: Array<[string, DataRecord[]]>
  primaryField: DataField | null
  dateField: DataField | null
  coverField: DataField | null
  sortedAndFilteredRecords: DataRecord[]
  canMoveKanbanCards: boolean
  draggingRecordId: number | null
  dragOverRecordId: number | null
  dragOverGroup: string
  onDragStart: (recordId: number) => void
  onDragEnd: () => void
  onDragOverRecord: (recordId: number | null) => void
  onDragOverGroup: (group: string) => void
  onMoveKanbanRecord: (
    recordId: number,
    group: string,
    targetRecordId?: number,
  ) => void
  onAddRecord: (
    initialValues?: Record<string, DataRecordValue>,
  ) => void | Promise<void>
  onOpenRecord: (record: DataRecord) => void
  onOpenDocument?: (path: string) => void
  onPreviewAttachments: (items: DataAttachment[], index: number) => void
  onRegenerate?: (record: DataRecord, field: DataField) => void
  /** The grid view's props (the table view), when it shows. */
  grid: GridViewProps
}

/** The view the editor shows: the grid, or the kanban, calendar or gallery of the records it holds. */
export function TableEditorContent({
  tableId,
  viewMode,
  visibleFields,
  groupField,
  groupedRecords,
  primaryField,
  dateField,
  coverField,
  sortedAndFilteredRecords,
  canMoveKanbanCards,
  draggingRecordId,
  dragOverRecordId,
  dragOverGroup,
  onDragStart,
  onDragEnd,
  onDragOverRecord,
  onDragOverGroup,
  onMoveKanbanRecord,
  onAddRecord,
  onOpenRecord,
  onOpenDocument,
  onPreviewAttachments,
  onRegenerate,
  grid,
}: TableEditorContentProps) {
  if (viewMode === 'kanban') {
    return (
      <KanbanView
        fields={visibleFields}
        groupField={groupField}
        groups={groupedRecords}
        primaryField={primaryField}
        canMoveCards={canMoveKanbanCards}
        draggingRecordId={draggingRecordId}
        dragOverRecordId={dragOverRecordId}
        dragOverGroup={dragOverGroup}
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
        onDragOverRecord={onDragOverRecord}
        onDragOverGroup={onDragOverGroup}
        onMoveRecord={onMoveKanbanRecord}
        onAddRecord={(group) =>
          void onAddRecord(
            groupField
              ? { [groupField.name]: kanbanGroupToValue(groupField, group) }
              : undefined,
          )
        }
        onOpenRecord={onOpenRecord}
        onOpenDocument={onOpenDocument}
        onPreviewAttachments={onPreviewAttachments}
        tableId={tableId}
        onRegenerate={onRegenerate}
      />
    )
  }

  if (viewMode === 'calendar') {
    return (
      <CalendarView
        records={sortedAndFilteredRecords}
        fields={visibleFields}
        primaryField={primaryField}
        dateField={dateField}
        onAddRecord={() => void onAddRecord()}
        onOpenRecord={onOpenRecord}
        onOpenDocument={onOpenDocument}
        onPreviewAttachments={onPreviewAttachments}
        tableId={tableId}
        onRegenerate={onRegenerate}
      />
    )
  }

  if (viewMode === 'gallery') {
    return (
      <GalleryView
        documentId={grid.documentId}
        records={sortedAndFilteredRecords}
        fields={visibleFields}
        primaryField={primaryField}
        coverField={coverField}
        onOpenRecord={onOpenRecord}
        onOpenDocument={onOpenDocument}
        onPreviewAttachments={onPreviewAttachments}
        tableId={tableId}
        onRegenerate={onRegenerate}
      />
    )
  }

  return <GridView {...grid} />
}
