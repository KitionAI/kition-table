import { describe, expect, it, vi } from 'vitest'
import { engineAIHost } from './aiFields'
import type { EngineTableDataSource } from './dataSource'
import type { HostServices } from './hostRequests'

function setup(prepared: unknown, applied: unknown, answer: unknown) {
  const command = vi.fn(async (method: string) => (method === 'ai.prepare' ? prepared : applied))
  const read = vi.fn(async () => ({ data: new Uint8Array([1, 2, 3]), mimeType: 'image/png', name: 'a.png' }))
  const source = { command, attachments: { read, put: vi.fn() } } as unknown as EngineTableDataSource
  const generate = vi.fn(async () => answer)
  const services: HostServices = { generates: true, acts: () => false, generate, action: vi.fn() } as unknown as HostServices
  return { ai: engineAIHost(source, services), command, read, generate }
}

describe('AI fields over the engine and host/generate', () => {
  it('prepares in the engine, asks the host for the model call with blob bytes, and applies the answer', async () => {
    const record = { id: 7, values: { notes: 'done' } }
    const { ai, command, read, generate } = setup(
      {
        fieldId: 3,
        output: 'text',
        model: 'preferred-model',
        requests: [
          {
            recordId: 7,
            messages: [
              { role: 'system', parts: [{ type: 'text', text: 'Summarize.' }] },
              { role: 'user', parts: [{ type: 'text', text: 'A receipt' }, { type: 'image', hash: 'abc', mime: 'image/png' }, { type: 'image', url: 'https://example.com/a.png' }] },
            ],
          },
        ],
        skipped: [],
      },
      { record, field: { id: 3 } },
      { text: 'A summary' },
    )

    const result = await ai.runCell({ documentId: 1, tableId: 2, recordId: 7, fieldId: 3, action: 'text', config: { type: 'text' } as never, force: true })

    expect(result.record).toBe(record)
    expect(command).toHaveBeenNthCalledWith(1, 'ai.prepare', { tableId: 2, fieldId: 3, record_ids: [7], action: 'text', config: { type: 'text' } })
    expect(read).toHaveBeenCalledWith({ documentId: 0, hash: 'abc' })
    expect(generate).toHaveBeenCalledWith({
      purpose: 'ai-field',
      output: 'text',
      model: 'preferred-model',
      messages: [
        { role: 'system', parts: [{ type: 'text', text: 'Summarize.' }] },
        {
          role: 'user',
          parts: [
            { type: 'text', text: 'A receipt' },
            { type: 'image', mime: 'image/png', bytes: new Uint8Array([1, 2, 3]).buffer },
            { type: 'image', url: 'https://example.com/a.png' },
          ],
        },
      ],
    })
    expect(command).toHaveBeenNthCalledWith(2, 'ai.apply', { tableId: 2, fieldId: 3, action: 'text', results: [{ recordId: 7, text: 'A summary' }] })
  })

  it('applies images in base64, one host/generate per image, and a failed generation as the record error', async () => {
    const { ai, command, generate } = setup(
      {
        fieldId: 3,
        output: 'image',
        requests: [
          { recordId: 1, messages: [{ role: 'user', parts: [{ type: 'text', text: 'A cat' }] }] },
          { recordId: 2, messages: [{ role: 'user', parts: [{ type: 'text', text: 'A dog' }] }] },
        ],
        skipped: [],
      },
      { items: [], field: { id: 3 }, accepted: 2, completed: 1, failed: 1, limit: 100 },
      null,
    )
    generate.mockResolvedValueOnce({ image: { mime: 'image/png', bytes: new Uint8Array([104, 105]).buffer } }).mockRejectedValueOnce(new Error('No credits left'))

    await ai.runBatch({ documentId: 1, tableId: 2, fieldId: 3, record_ids: [1, 2], scope: 'column-menu', limit: 100, action: 'image', config: {} as never })

    expect(command).toHaveBeenNthCalledWith(1, 'ai.prepare', { tableId: 2, fieldId: 3, record_ids: [1, 2], limit: 100, action: 'image', config: {} })
    expect(command).toHaveBeenNthCalledWith(2, 'ai.apply', {
      tableId: 2,
      fieldId: 3,
      action: 'image',
      results: [
        { recordId: 1, images: [{ mime: 'image/png', content: 'aGk=' }] },
        { recordId: 2, error: 'No credits left' },
      ],
    })
  })

  it('asks the host once per image an image field wants (options.count)', async () => {
    const { ai, command, generate } = setup(
      { fieldId: 3, output: 'image', requests: [{ recordId: 1, messages: [{ role: 'user', parts: [{ type: 'text', text: 'A cat' }] }], options: { count: 2 } }], skipped: [] },
      { items: [], field: { id: 3 }, accepted: 1, completed: 1, failed: 0, limit: 100 },
      null,
    )
    generate.mockResolvedValue({ image: { mime: 'image/png', bytes: new Uint8Array([104, 105]).buffer } })

    await ai.runBatch({ documentId: 1, tableId: 2, fieldId: 3, record_ids: [1], action: 'image', config: {} as never })

    expect(generate).toHaveBeenCalledTimes(2)
    expect(command).toHaveBeenNthCalledWith(2, 'ai.apply', expect.objectContaining({ results: [{ recordId: 1, images: [{ mime: 'image/png', content: 'aGk=' }, { mime: 'image/png', content: 'aGk=' }] }] }))
  })

  it("names no model of its own: the host runs the person's", async () => {
    const { ai } = setup(null, null, null)
    await expect(ai.resolveRuntimeModel({} as never)).resolves.toBeUndefined()
    await expect(ai.loadModelOptions('text')).resolves.toEqual([])
  })
})
