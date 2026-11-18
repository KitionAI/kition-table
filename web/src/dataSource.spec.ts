// The engine refuses a record `values` key that names no field with a message
// and its errors as JSON after a record separator; the editor shows the message alone.
import { describe, expect, it } from 'vitest'
import { createWorkerTableDataSource, type TableEngineAccess } from './dataSource'

const message = '`values.Primary channel` names no field of table 2; the field titled "Primary channel" is named `primary_channel`'

function refusing(error: Error): TableEngineAccess {
  return {
    run: async () => {
      throw error
    },
    invoke: async <T,>() => '[]' as T,
    revision: () => 0,
  }
}

describe("the engine's refusal of a call's parameters", () => {
  it('reaches the editor as its message, without the errors', async () => {
    const errors = JSON.stringify([{ path: '/values/Primary channel', message, expected: 'a field name of table 2 (not its title): `primary_channel`' }])
    const source = createWorkerTableDataSource(refusing(new Error(`${message}\u001e${errors}`)), 'Contacts.ktable')
    await expect(source.records.create({ documentId: 1, tableId: 2, values: { 'Primary channel': 'Email' } })).rejects.toThrow(new Error(message))
  })

  it('leaves any other error as it is', async () => {
    const source = createWorkerTableDataSource(refusing(new Error('Table does not exist')), 'Contacts.ktable')
    await expect(source.records.create({ documentId: 1, tableId: 9, values: {} })).rejects.toThrow('Table does not exist')
  })
})
