/**
 * Export (CSV, Markdown): the engine answers one table's file as base64 (`table.file.export`) and
 * the host writes it as a new file beside the document (`host/write` mode `export`, Kit UI
 * protocol). The editor runs in the host's page, where it cannot write files itself; only the
 * host's answer says the file was written, and the path it answers is the one the host chose (it
 * never replaces a file).
 */

/** The formats `kit.json` `exports` declares. */
export type TableExportFormat = 'csv' | 'md'

export const EXPORT_COMMAND = 'table.file.export'

/** What `table.file.export` answers. */
export type ExportAnswer = { format: string; extension: string; mime: string; name: string; tableId: number; bytes: number; data: string }

/** What the export needs of the host connection. */
export type ExportHost = {
  init: { document: { path: string; name: string } }
  exportFile: (path: string, bytes: Uint8Array) => Promise<string>
}

/** Runs an engine command and answers its JSON, or throws the engine's error. */
export type RunCommand = (command: string, params: Record<string, unknown>) => Promise<string>

/**
 * The new file's workspace path: in the document's folder, named after the document; one table of
 * a document of several says which.
 */
export function exportPath(documentPath: string, documentName: string, extension: string, table: string | null = null): string {
  const folder = documentPath.includes('/') ? documentPath.slice(0, documentPath.lastIndexOf('/') + 1) : ''
  const own = documentName || documentPath.slice(documentPath.lastIndexOf('/') + 1)
  const clean = (text: string) => text.replace(/[\\/:*?"<>|]/g, '_').trim()
  const stem = clean(own.replace(/\.[^.]+$/, '')) || 'Table'
  const part = table ? clean(table) : ''
  return `${folder}${part ? `${stem} - ${part}` : stem}.${extension}`
}

/** The file's bytes from the engine's base64. */
export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

/** Checks the engine's answer: the format asked for, with its bytes. */
export function parseAnswer(text: string | null, format: TableExportFormat): ExportAnswer {
  const answer = (text ? JSON.parse(text) : null) as Partial<ExportAnswer> | null
  if (!answer || typeof answer.data !== 'string' || answer.extension !== format) {
    throw new Error(`the engine answered no ${format === 'md' ? 'Markdown' : 'CSV'} file`)
  }
  return answer as ExportAnswer
}

/**
 * Exports one table: the engine's bytes (`run`, which throws the engine's error), written by the
 * host (which throws the host's). `tables` is how many tables the document has: with more than
 * one, the file names the table. Answers the path the host wrote.
 */
export async function exportTableThroughHost(
  run: RunCommand,
  host: ExportHost,
  format: TableExportFormat,
  table: { id: number; title: string },
  tables: number,
): Promise<string> {
  const answer = parseAnswer(await run(EXPORT_COMMAND, { format, tableId: table.id }), format)
  const { path, name } = host.init.document
  return host.exportFile(exportPath(path, name, answer.extension, tables > 1 ? table.title : null), decodeBase64(answer.data))
}
