import { useRef, useState } from 'react'
import { DownloadIcon as Download, FileUpIcon as FileUp, LayoutDashboardIcon as LayoutDashboard, PlusIcon as Plus } from '@kition/tessera-icons/react'
import { useTranslation } from 'react-i18next'
import { Popover, PopoverContent, PopoverTrigger } from '@kition/kit-editor'
import { DATA_TABLE_ACTION_EVENT, type DataImportResult, type TableAction, type TableActionEventDetail } from './editor'
import { TableFileImportDialog } from './editor/TableFileImportDialog'
import type { EngineTableDataSource } from './dataSource'
import { addTemplateToDocument } from './templates/addTemplate'
import { TemplateLibraryDialog } from './templates/TemplateLibraryDialog'
import type { TableTemplateDefinition } from './templates/tableTemplates'
import type { Shown } from './shown'
import { cn } from './lib/cn'

const IMPORT_ACCEPT = '.csv,.tsv,.xlsx,text/csv,text/tab-separated-values,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'

const TAB_CLASS = 'flex h-7 max-w-48 shrink-0 items-center gap-1.5 truncate rounded-lg px-2.5 text-[13px] font-medium transition-colors'

/**
 * The document's tables and dashboards in the bar, the one shown highlighted; exporting the table
 * shown (CSV, Markdown: the table editor writes it beside the document through the host); and,
 * unless the document is read-only, adding a table (blank or from a template) and importing a file
 * as a new table.
 */
export function TableTabs({
  tables,
  dashboards,
  shown,
  onSelect,
  source,
  readOnly,
  documentPath,
}: {
  tables: Array<{ id: number; title: string }>
  dashboards: Array<{ id: string; title: string }>
  shown: Shown | null
  onSelect: (shown: Shown) => void
  source: EngineTableDataSource
  readOnly: boolean
  documentPath: string
}) {
  const { t } = useTranslation('table')
  const [libraryOpen, setLibraryOpen] = useState(false)
  const [importFile, setImportFile] = useState<File | null>(null)
  const [documentId, setDocumentId] = useState(0)
  const fileInput = useRef<HTMLInputElement | null>(null)
  const [exportOpen, setExportOpen] = useState(false)

  /** Asks the table editor showing the table to export it. */
  function exportShown(action: TableAction) {
    setExportOpen(false)
    window.dispatchEvent(new CustomEvent<TableActionEventDetail>(DATA_TABLE_ACTION_EVENT, { detail: { documentPath, action } }))
  }

  async function addTemplate(template?: TableTemplateDefinition) {
    try {
      const document = await source.documents.get({ documentId: 0 })
      const tableId = await addTemplateToDocument(source, document, template)
      onSelect({ kind: 'table', id: tableId })
      setLibraryOpen(false)
      return true
    } catch {
      return false
    }
  }

  async function chooseImport(file: File | null) {
    if (!file) return
    setDocumentId((await source.documents.get({ documentId: 0 })).id)
    setImportFile(file)
  }

  return (
    <nav className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto px-2" aria-label={t('kit.tables')} data-testid="table-tabs">
      {tables.map((table) => {
        const active = shown?.kind === 'table' && shown.id === table.id
        return (
          <button
            key={`table-${table.id}`}
            type="button"
            className={cn(TAB_CLASS, active ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-muted hover:text-foreground')}
            aria-current={active ? 'page' : undefined}
            onClick={() => onSelect({ kind: 'table', id: table.id })}
            data-testid={`table-tab-${table.id}`}
          >
            <span className="truncate">{table.title}</span>
          </button>
        )
      })}
      {dashboards.map((dashboard) => {
        const active = shown?.kind === 'dashboard' && shown.id === dashboard.id
        return (
          <button
            key={`dashboard-${dashboard.id}`}
            type="button"
            className={cn(TAB_CLASS, active ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-muted hover:text-foreground')}
            aria-current={active ? 'page' : undefined}
            onClick={() => onSelect({ kind: 'dashboard', id: dashboard.id })}
            data-testid={`dashboard-tab-${dashboard.id}`}
          >
            <LayoutDashboard className="size-3.5 shrink-0" aria-hidden />
            <span className="truncate">{dashboard.title}</span>
          </button>
        )
      })}
      {shown?.kind === 'table' ? (
        <Popover open={exportOpen} onOpenChange={setExportOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="grid size-7 shrink-0 place-items-center rounded-lg text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              title={t('kit.export')}
              aria-label={t('kit.export')}
              aria-expanded={exportOpen}
              data-testid="table-export"
            >
              <Download className="size-4" />
            </button>
          </PopoverTrigger>
          <PopoverContent align="start" sideOffset={6} className="data-inline-view-tab-menu" role="menu" onCloseAutoFocus={(event) => event.preventDefault()}>
            <button type="button" role="menuitem" onClick={() => exportShown('export_csv')} data-testid="table-export-csv">
              <span>{t('kit.exportCsv')}</span>
            </button>
            <button type="button" role="menuitem" onClick={() => exportShown('export_markdown')} data-testid="table-export-md">
              <span>{t('kit.exportMarkdown')}</span>
            </button>
          </PopoverContent>
        </Popover>
      ) : null}
      {readOnly ? null : (
        <>
          <button
            type="button"
            className="grid size-7 shrink-0 place-items-center rounded-lg text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            title={t('kit.addTable')}
            aria-label={t('kit.addTable')}
            onClick={() => setLibraryOpen(true)}
            data-testid="table-add"
          >
            <Plus className="size-4" />
          </button>
          <button
            type="button"
            className="grid size-7 shrink-0 place-items-center rounded-lg text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            title={t('kit.importTable')}
            aria-label={t('kit.importTable')}
            onClick={() => fileInput.current?.click()}
            data-testid="table-import"
          >
            <FileUp className="size-4" />
          </button>
          <input
            ref={fileInput}
            type="file"
            accept={IMPORT_ACCEPT}
            className="hidden"
            onChange={(event) => {
              void chooseImport(event.target.files?.[0] || null)
              event.currentTarget.value = ''
            }}
          />
          {/* Mounted while open, so it opens on the catalog each time. */}
          {libraryOpen ? <TemplateLibraryDialog open onOpenChange={setLibraryOpen} onSelect={addTemplate} /> : null}
          <TableFileImportDialog
            open={Boolean(importFile)}
            file={importFile}
            target={{ kind: 'new_table', documentId }}
            onOpenChange={(open) => {
              if (!open) setImportFile(null)
            }}
            onCompleted={(result: DataImportResult) => {
              if (result.table_id != null) onSelect({ kind: 'table', id: result.table_id })
            }}
          />
        </>
      )}
    </nav>
  )
}
