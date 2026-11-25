import { createContext, useContext } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  DataImportField,
  DataImportPreview,
  DataImportResult,
  DataImportWriteMode,
} from '../../types/dataImports'
import type { ModelCapability, RuntimeWritingModel } from '../../types/base'
import type { AnyAIConfig } from '../../types/aiConfig'
import type {
  DataAttachment,
  DataTable,
  RunDataAIFieldBatchResponse,
  RunDataAIFieldCellResponse,
} from '../../types/dataDocument'

type NotifyOptions = { id?: string | number; description?: string }

/** Toasts. */
export type TableNotifier = {
  success(message: string, options?: NotifyOptions): unknown
  error(message: string, options?: NotifyOptions): unknown
  dismiss(id?: string | number): void
}

/** Where an imported file goes: rows into an open table, or a new table in the open document. */
export type TableFileImportTarget =
  | { kind: 'existing_table'; documentId: number; table: DataTable }
  | { kind: 'new_table'; documentId: number; tableTitle: string }

/** A file read and previewed, ready to import. */
export type PreparedTableFileImport = {
  file: File
  preview: DataImportPreview
}

/** CSV, TSV, and XLSX import into a table, in the page. */
type TableFileImporter = {
  prepare(file: File, options?: { sheet?: string }): Promise<PreparedTableFileImport>
  execute(options: {
    fieldTypes: DataImportField['type'][]
    prepared: PreparedTableFileImport
    target: TableFileImportTarget
    writeMode: DataImportWriteMode
  }): Promise<{ result: DataImportResult }>
}

type AIRunParams = {
  action: string
  config: AnyAIConfig
  force?: boolean
  runtime_model?: RuntimeWritingModel
}

/** AI fields: the model to run with, and the runs. */
type TableAIHost = {
  /** The model an AI field's config runs on (its override, or the app's default). */
  resolveRuntimeModel(config: AnyAIConfig): Promise<RuntimeWritingModel | undefined>
  runCell(
    params: { documentId: number; tableId: number; recordId: number; fieldId: number } & AIRunParams,
    options?: { signal?: AbortSignal },
  ): Promise<RunDataAIFieldCellResponse>
  runBatch(
    params: { documentId: number; tableId: number; fieldId: number; record_ids: number[]; scope?: string; limit?: number } & AIRunParams,
    options?: { signal?: AbortSignal },
  ): Promise<RunDataAIFieldBatchResponse>
  /** Models a field may override its runtime model with. */
  loadModelOptions(capability: ModelCapability): Promise<Array<{ value: string; label: string }>>
}

/** What the record context menu's "create design" hands the app. */
export type TableDesignRequest = {
  title: string
  fields: Record<string, unknown>
  record?: { documentId: number; tableId: number; recordId: number; field: string }
}

/** What a button field's "run workflow" action hands the app. */
export type TableWorkflowRequest = { workflowId: number; tableId: number; recordId: number }

/**
 * App services the table editor uses besides its data (`TableDataSource`). Every member is
 * optional: without one, the editor hides what needs it (attachment upload, image copy, CSV
 * export, file import, AI fields, design from a record, workflows) or does without (toasts).
 */
export interface TableHost {
  /**
   * A URL the page can load for an attachment's stored `url` (a workspace path, a bundled
   * asset, or a URL). Without it, only `http(s):`, `data:`, and `blob:` URLs are shown.
   */
  resolveFileURL?(url: string): string
  /**
   * A URL the page can load for the blob `hash` of the document `documentId` (an attachment held
   * in the file, `kition-blob:<sha256>`). Without it, the editor makes object URLs from the bytes
   * the data source reads (`TableDataSource.attachments.read`).
   */
  resolveBlobURL?(params: { documentId: number; hash: string }): string
  /**
   * Stores a file as an attachment of `tableId`; answers the value an attachment cell holds for
   * it. Without it, the data source's `attachment.add`, when it has one.
   */
  uploadAttachment?(params: { documentId: number; tableId: number; file: File }): Promise<DataAttachment>
  copyImageToClipboard?(url: string): Promise<unknown>
  /**
   * Writes one table as a new CSV or Markdown file beside the document, through the host (Kit UI
   * protocol `host/write` mode `export`); answers the path the host wrote, and throws the host's
   * or the engine's error. Without it, the editor offers no export.
   */
  exportTable?(format: 'csv' | 'md', table: { id: number; title: string }): Promise<string>
  notify?: TableNotifier
  fileImport?: TableFileImporter
  ai?: TableAIHost
  requestDesignFromRecord?(request: TableDesignRequest): void
  /**
   * Runs one of the app's workflows on a record (a button field's "run workflow" action).
   * Without it, that action does not show.
   */
  runWorkflow?(request: TableWorkflowRequest): Promise<{ done: boolean; message?: string }>
  /** The BCP 47 locale dates are formatted for; without it, the i18next instance's language. */
  locale?(): string
}

const TableHostContext = createContext<TableHost>({})

export const TableHostProvider = TableHostContext.Provider

/** The host the editor's root provides (`TableHostProvider`); an empty one without a provider. */
export function useTableHost(): TableHost {
  return useContext(TableHostContext)
}

const SELF_CONTAINED_URL = /^(https?:|data:|blob:)/i

/** `host.resolveFileURL`, or the URL itself when it needs no host to load. */
export function resolveTableFileURL(host: TableHost, url: string) {
  if (host.resolveFileURL) return host.resolveFileURL(url)
  return SELF_CONTAINED_URL.test(url) ? url : ''
}

const silentNotifier: TableNotifier = {
  success: () => undefined,
  error: () => undefined,
  dismiss: () => undefined,
}

/** `host.notify`, or a notifier that shows nothing. */
export function useTableNotify(): TableNotifier {
  return useTableHost().notify ?? silentNotifier
}

/** `host.locale()`, or the language of the i18next instance the editor translates with. */
export function useTableLocale(): string {
  const host = useTableHost()
  const { i18n } = useTranslation()
  return host.locale?.() ?? i18n.language
}
