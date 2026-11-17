/**
 * The table editor's entry: its data and host seams (`TableDataSource`, `TableHost`), the table
 * document types, and the helpers its host shares with it. The editors themselves load from their
 * own modules (`./TableEditor`, `./DashboardEditor`, `./TableFileImportDialog`), so importing this
 * one stays light. The editor was Kition's `packages/table-editor` (commit 7ee7b538).
 */
export * from './types/base'
export * from './types/dataDocument'
export * from './types/aiConfig'
export * from './types/tableFilter'
export * from './types/dataImports'
export * from './lib/timestamps'
export * from './table/ports/TableDataSource'
export * from './table/ports/TableHost'
export * from './table/lib/csvImport'
export * from './table/lib/tableActions'
export * from './dashboard/lib/dashboardCreation'
export * from './dashboard/lib/dashboardMetadata'
