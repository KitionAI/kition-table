import { beforeEach, describe, expect, it, vi } from 'vitest'

const toast = vi.hoisted(() => Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }))
vi.mock('@kition/kit-editor', () => ({ toast }))

import { kitTableHost } from './tableHost'
import type { EngineTableDataSource } from './dataSource'
import type { HostServices } from './hostRequests'

const messages = {
  designFailed: 'The design could not be made',
  workflowFailed: 'The workflow could not run',
  aiUnavailable: 'AI is not set up',
  aiSetUp: 'Set up AI',
  settingsFailed: 'Settings could not open',
}

function refusal(reason: string, message: string) {
  return Object.assign(new Error(message), { code: -32000, data: { reason } })
}

function setup(served: string[]) {
  const prepared = {
    fieldId: 3,
    output: 'text',
    requests: [{ recordId: 7, messages: [{ role: 'user', parts: [{ type: 'text', text: 'Summarize' }] }] }],
    skipped: [],
  }
  const applied = { items: [], field: { id: 3 }, accepted: 1, completed: 0, failed: 1, limit: 100, errors: [] }
  const command = vi.fn(async (method: string) => (method === 'ai.prepare' ? prepared : applied))
  const source = { command, attachments: { read: vi.fn(), put: vi.fn() } } as unknown as EngineTableDataSource
  const generate = vi.fn(async () => {
    throw refusal('unavailable', 'Sign in to Kition before using Kition Cloud models.')
  })
  const action = vi.fn(async () => ({ done: true }))
  const services = { generates: true, acts: (id: string) => served.includes(id), generate, action } as unknown as HostServices
  const host = kitTableHost('en-US', source, services, messages, vi.fn())
  return { host, action }
}

const run = (host: ReturnType<typeof setup>['host']) =>
  host.ai!.runBatch({ documentId: 1, tableId: 2, fieldId: 3, record_ids: [7], scope: 'column-menu', limit: 100, action: 'text', config: {} as never })

describe('an AI field run the host cannot serve', () => {
  beforeEach(() => vi.clearAllMocks())

  it('offers the next step: Set up AI asks the host to open its AI settings', async () => {
    const { host, action } = setup(['settings.open'])
    await run(host)

    expect(toast.error).toHaveBeenCalledTimes(1)
    const [title, options] = toast.error.mock.calls[0]
    expect(title).toBe('AI is not set up')
    expect(options).toMatchObject({ description: 'Sign in to Kition before using Kition Cloud models.', action: { label: 'Set up AI' } })

    options.action.onClick()
    await vi.waitFor(() => expect(action).toHaveBeenCalledWith('settings.open', { section: 'ai' }))
  })

  it('shows the same notice once for a whole run, and without a button when the host has no settings action', async () => {
    const { host, action } = setup([])
    await run(host)
    await run(host)

    expect(toast.error).toHaveBeenCalledTimes(2)
    expect(toast.error.mock.calls[0][1]).toMatchObject({ id: 'table-ai-unavailable' })
    expect(toast.error.mock.calls[0][1].action).toBeUndefined()
    expect(action).not.toHaveBeenCalled()
  })
})
