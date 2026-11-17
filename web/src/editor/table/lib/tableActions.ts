export const DATA_TABLE_ACTION_EVENT = 'kition:data-table-action'

export type TableAction =
  | 'rename'
  | 'duplicate'
  | 'export_csv'
  | 'export_markdown'
  | 'import_csv'
  | 'copy_api'
  | 'history'
  | 'share'
  | 'delete'

export type TableActionEventDetail = {
  documentPath: string
  action: TableAction
}
