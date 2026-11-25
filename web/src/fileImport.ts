import type {
  DataImportPreview,
  DataImportResult,
  ExecuteDataImportPayload,
  TableHost,
} from './editor'
import type { DataImportPreviewRequest } from './editor/generated/data-import'
import { toBase64, type EngineTableDataSource } from './dataSource'

/** What every import command carries besides its request: the file's name and its bytes in base64. */
type FileParams = { fileName: string; content: string }

const LEGACY_WORKBOOK = /\.xls$/i

async function fileParams(file: File): Promise<FileParams> {
  return { fileName: file.name, content: toBase64(new Uint8Array(await file.arrayBuffer())) }
}

/**
 * CSV, TSV, and XLSX import in the page, through the engine: `file.previewImport` reads the file
 * and infers its fields (the data-import contract's preview request, plus the file), and
 * `file.runImport` writes it into a table of the open document, or a new one, in one engine
 * command (an Undo step), and answers the import's result. The file goes with both requests, so
 * the engine keeps nothing between them.
 */
export function engineFileImport(source: EngineTableDataSource): NonNullable<TableHost['fileImport']> {
  // The bytes are read once per prepared file.
  const read = new WeakMap<File, Promise<FileParams>>()
  const bytesOf = (file: File) => {
    let params = read.get(file)
    if (!params) read.set(file, (params = fileParams(file)))
    return params
  }

  return {
    async prepare(file, options) {
      if (LEGACY_WORKBOOK.test(file.name)) {
        throw new Error('Legacy XLS files are not supported. Save the workbook as XLSX and try again.')
      }
      const request: DataImportPreviewRequest = {
        source: { kind: 'upload', upload_name: file.name },
        ...(options?.sheet ? { sheet: options.sheet } : {}),
      }
      const preview = await source.command<DataImportPreview>('import.preview', { ...request, ...(await bytesOf(file)) })
      return { file, preview }
    },
    async execute({ fieldTypes, prepared, target, writeMode }) {
      const { preview } = prepared
      const request: ExecuteDataImportPayload = {
        import_token: preview.import_token,
        target:
          target.kind === 'existing_table'
            ? { kind: 'existing_table', document_id: target.documentId, table_id: target.table.id }
            : { kind: 'new_table', document_id: target.documentId, table_title: target.tableTitle },
        write_mode: target.kind === 'new_table' ? 'append' : writeMode,
        schema_mode: 'auto',
        field_overrides: preview.fields.map((field, index) => ({
          index: field.index,
          title: field.title,
          type: fieldTypes[index] || field.type,
          options: field.options,
        })),
      }
      const result = await source.command<DataImportResult>('import.run', { ...request, ...(await bytesOf(prepared.file)) })
      return { result }
    },
  }
}
