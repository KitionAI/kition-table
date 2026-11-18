// Export: the engine's bytes go to the host, which writes the new file; success only when the
// host answers, and the host's error otherwise.
import { describe, expect, it } from 'vitest'
import { decodeBase64, exportPath, exportTableThroughHost, parseAnswer } from './exportFile'

const CSV = btoa('Name,Score\r\nAda,10\r\n')

const engine = (calls: Array<[string, Record<string, unknown>]>) => async (command: string, params: Record<string, unknown>) => {
  calls.push([command, params])
  return JSON.stringify({ format: params.format, extension: params.format, mime: 'x', name: 'People', tableId: params.tableId, bytes: 20, data: CSV })
}

describe('exportFile', () => {
  it('names the new file after the document, in its folder', () => {
    expect(exportPath('Team/People.ktable', 'People.ktable', 'csv')).toBe('Team/People.csv')
    expect(exportPath('People.ktable', 'People.ktable', 'md', 'Q3/Q4')).toBe('People - Q3_Q4.md')
    expect(exportPath('a/Plan.ktable', '', 'csv')).toBe('a/Plan.csv')
    expect(exportPath('', '', 'csv')).toBe('Table.csv')
  })

  it('decodes the base64 the engine answers', () => {
    expect(new TextDecoder().decode(decodeBase64(CSV))).toBe('Name,Score\r\nAda,10\r\n')
  })

  it('refuses an answer without the file asked for', () => {
    expect(() => parseAnswer('null', 'csv')).toThrow(/no CSV file/)
    expect(() => parseAnswer(null, 'md')).toThrow(/no Markdown file/)
    expect(() => parseAnswer(JSON.stringify({ extension: 'md', data: CSV }), 'csv')).toThrow(/no CSV file/)
    expect(parseAnswer(JSON.stringify({ extension: 'csv', data: CSV }), 'csv').data).toBe(CSV)
  })

  it('hands the bytes to the host and answers the path the host wrote', async () => {
    const calls: Array<[string, Record<string, unknown>]> = []
    const writes: Array<[string, Uint8Array]> = []
    const host = {
      init: { document: { path: 'Team/People.ktable', name: 'People.ktable' } },
      exportFile: async (path: string, bytes: Uint8Array) => (writes.push([path, bytes]), `${path} (host)`),
    }
    expect(await exportTableThroughHost(engine(calls), host, 'csv', { id: 3, title: 'Staff' }, 2)).toBe('Team/People - Staff.csv (host)')
    expect(calls[0]).toEqual(['table.file.export', { format: 'csv', tableId: 3 }])
    expect(new TextDecoder().decode(writes[0][1])).toBe('Name,Score\r\nAda,10\r\n')
    await exportTableThroughHost(engine(calls), host, 'md', { id: 1, title: 'Staff' }, 1)
    expect(calls[1]).toEqual(['table.file.export', { format: 'md', tableId: 1 }])
    expect(writes[1][0]).toBe('Team/People.md')
  })

  it("the host's refusal and the engine's error reach the caller", async () => {
    const host = {
      init: { document: { path: 'P.ktable', name: 'P.ktable' } },
      exportFile: async (): Promise<string> => {
        throw new Error('the disk is full')
      },
    }
    await expect(exportTableThroughHost(engine([]), host, 'csv', { id: 1, title: 'T' }, 1)).rejects.toThrow(/the disk is full/)
    let wrote = false
    const failing = async (): Promise<string> => {
      throw new Error('Table not found')
    }
    const writing = { ...host, exportFile: async () => ((wrote = true), '') }
    await expect(exportTableThroughHost(failing, writing, 'md', { id: 9, title: 'T' }, 1)).rejects.toThrow(/Table not found/)
    expect(wrote).toBe(false)
  })
})
