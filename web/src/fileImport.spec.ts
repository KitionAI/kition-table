import { describe, expect, it, vi } from 'vitest'
import type { EngineTableDataSource } from './dataSource'
import { engineFileImport } from './fileImport'

const preview = {
  import_token: 'token-1',
  source: { kind: 'upload', upload_name: 'issues.csv' },
  format: 'csv',
  row_count: 2,
  field_count: 2,
  fields: [
    { index: 0, title: 'Title', type: 'text', nullable: false, sample_values: ['A'] },
    { index: 1, title: 'Points', type: 'number', nullable: true, sample_values: [3] },
  ],
  sample_rows: [['A', 3]],
  warnings: [],
  sheets: [],
}

function setup() {
  const result = { table_id: 9, rows_total: 2, rows_created: 2, rows_updated: 0, rows_skipped: 0, fields_created: 2, fields_updated: 0, warnings: [] }
  const command = vi.fn(async (method: string) => (method === 'import.preview' ? preview : result))
  return { importer: engineFileImport({ command } as unknown as EngineTableDataSource), command, result }
}

describe('file import through the engine', () => {
  it('previews with the contract request plus the file, then runs into a new table of the document', async () => {
    const { importer, command, result } = setup()
    const file = new File(['Title,Points\nA,3\n'], 'issues.csv', { type: 'text/csv' })

    const prepared = await importer.prepare(file, { sheet: 'Sheet1' })
    expect(prepared.preview.import_token).toBe('token-1')
    expect(command).toHaveBeenNthCalledWith(1, 'import.preview', {
      source: { kind: 'upload', upload_name: 'issues.csv' },
      sheet: 'Sheet1',
      fileName: 'issues.csv',
      content: btoa('Title,Points\nA,3\n'),
    })

    const done = await importer.execute({ fieldTypes: ['long_text'], prepared, target: { kind: 'new_table', documentId: 4, tableTitle: 'Issues' }, writeMode: 'replace' })
    expect(done.result).toBe(result)
    expect(command).toHaveBeenNthCalledWith(2, 'import.run', {
      import_token: 'token-1',
      target: { kind: 'new_table', document_id: 4, table_title: 'Issues' },
      write_mode: 'append',
      schema_mode: 'auto',
      field_overrides: [
        { index: 0, title: 'Title', type: 'long_text', options: undefined },
        { index: 1, title: 'Points', type: 'number', options: undefined },
      ],
      fileName: 'issues.csv',
      content: btoa('Title,Points\nA,3\n'),
    })
  })

  it('writes into an open table with the chosen write mode', async () => {
    const { importer, command } = setup()
    const file = new File(['a'], 'rows.tsv')
    const prepared = await importer.prepare(file)
    await importer.execute({ fieldTypes: [], prepared, target: { kind: 'existing_table', documentId: 4, table: { id: 2 } as never }, writeMode: 'replace' })
    expect(command).toHaveBeenLastCalledWith('import.run', expect.objectContaining({
      target: { kind: 'existing_table', document_id: 4, table_id: 2 },
      write_mode: 'replace',
    }))
  })

  it('refuses legacy XLS workbooks before asking the engine', async () => {
    const { importer, command } = setup()
    await expect(importer.prepare(new File(['x'], 'old.xls'))).rejects.toThrow('XLSX')
    expect(command).not.toHaveBeenCalled()
  })
})
