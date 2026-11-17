import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/** The import contract, a copy of Kition's `contracts/runtime/data-import.schema.json` (commit 7ee7b538). */
const SCHEMA = resolve(dirname(fileURLToPath(import.meta.url)), '../../generated/data-import.schema.json')

describe('data import public contract', () => {
  it('publishes preview, execution, result, and asynchronous job definitions', () => {
    const schema = JSON.parse(readFileSync(SCHEMA, 'utf8'))

    expect(schema.$id).toBe('https://kition.ai/contracts/runtime/data-import.schema.json')
    expect(schema.$defs.preview_request.required).toEqual(['source'])
    expect(schema.$defs.preview_response.required).toContain('fields')
    expect(schema.$defs.execute_request.required).toEqual([
      'import_token',
      'target',
      'write_mode',
      'schema_mode',
    ])
    expect(schema.$defs.job.properties.status.enum).toEqual([
      'queued',
      'running',
      'completed',
      'failed',
      'canceled',
    ])
    expect(schema.$defs.result.required).toContain('rows_created')
  })

  it('requires portable workspace-relative source paths', () => {
    const schema = JSON.parse(readFileSync(SCHEMA, 'utf8'))
    const workspaceSource = schema.$defs.source.oneOf.find((item: any) => item.properties.kind.const === 'workspace')

    expect(workspaceSource.properties.workspace_path.pattern).toContain('(?!/)')
    expect(workspaceSource.properties.workspace_path.pattern).toContain('\\.\\.')
  })
})
