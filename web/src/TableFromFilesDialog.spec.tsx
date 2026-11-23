// The "New table from files" dialog: it says what choosing files left out (past the most files at
// once, and a file chosen again), says that the files go to the app's AI model, shows a failure
// in the person's language, and Escape, Cancel and the dialog going away stop a running run.
import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const toast = vi.hoisted(() => Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() }))
vi.mock('@kition/sdk/editor', async (importOriginal) => ({ ...(await importOriginal<Record<string, unknown>>()), toast }))

import * as fromFiles from './fromFiles'
import { MAX_FILES, planFromAnswer } from './fromFiles'
import { TableFromFilesDialog, type FromFilesRun } from './TableFromFilesDialog'
;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

let container: HTMLDivElement
let root: Root | null = null

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
})

afterEach(() => {
  act(() => root?.unmount())
  root = null
  container.remove()
  vi.clearAllMocks()
})

const byTestId = (id: string) => document.querySelector(`[data-testid="${id}"]`) as HTMLElement | null

async function mount(run: FromFilesRun, onOpenChange = vi.fn()) {
  await act(async () => {
    root = createRoot(container)
    root.render(createElement(TableFromFilesDialog, { open: true, onOpenChange, run, onCompleted: vi.fn() }))
    await Promise.resolve()
  })
  return onOpenChange
}

async function choose(files: File[]) {
  const input = byTestId('table-from-files-input') as HTMLInputElement
  Object.defineProperty(input, 'files', { value: files, configurable: true })
  await act(async () => {
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await Promise.resolve()
  })
}

async function start() {
  await act(async () => {
    byTestId('table-from-files-start')!.click()
    await Promise.resolve()
  })
}

/** A run that waits until it is canceled, and tells whether it was. */
function waitingRun() {
  let signal: AbortSignal | undefined
  const run = vi.fn<FromFilesRun>(
    (_files, _request, options) =>
      new Promise((_resolve, reject) => {
        signal = options.signal
        options.signal.addEventListener('abort', () => reject(new DOMException('Canceled', 'AbortError')))
      }),
  )
  return { run, aborted: () => signal?.aborted === true }
}

const file = (name: string, lastModified = 1) => new File([`bytes of ${name}`], name, { type: 'image/png', lastModified })

describe('the New table from files dialog', () => {
  it('says that the files are sent to the AI model the app uses', async () => {
    await mount(vi.fn())
    expect(document.body.textContent).toContain('The names and contents of the files you choose are sent to the AI model the app uses.')
  })

  it('keeps a file chosen again once and leaves out files past the most at once, and says both', async () => {
    await mount(vi.fn())
    await choose([file('a.png'), file('b.png')])
    await choose([file('a.png'), ...Array.from({ length: MAX_FILES + 1 }, (_, index) => file(`r${index}.png`))])
    expect(byTestId('table-from-files-choose')!.textContent).toContain(`${MAX_FILES} files chosen`)
    expect(byTestId('table-from-files-left-out')?.textContent).toBe(
      `3 more files were left out: at most ${MAX_FILES} files at once. 1 file was already chosen and is kept once`,
    )
  })

  it('stops a running run on Escape, as Cancel does', async () => {
    const { run, aborted } = waitingRun()
    const onOpenChange = await mount(run)
    await choose([file('a.png')])
    await start()
    expect(run).toHaveBeenCalledTimes(1)
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      ;(document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      await Promise.resolve()
    })
    expect(aborted()).toBe(true)
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })

  it('stops a running run when the dialog goes away', async () => {
    const { run, aborted } = waitingRun()
    await mount(run)
    await choose([file('a.png')])
    await start()
    act(() => root?.unmount())
    root = null
    expect(aborted()).toBe(true)
  })

  it("shows a failure in the person's language, by its code", async () => {
    let refused: unknown
    try {
      planFromAnswer('{"fields":[{"name":"Scan","type":"attachment","isAIField":true,"aiPrompt":"Read it"}]}')
    } catch (error) {
      refused = error
    }
    const run = vi.fn<FromFilesRun>(async () => {
      throw refused ?? new Error('accepted')
    })
    await mount(run)
    await choose([file('a.png')])
    await start()
    expect(byTestId('table-from-files-error')?.textContent).toBe('The table could not be made: The design makes the attachment field “Scan” an AI field.')
  })

  it('tells the person when the plain design was used and which AI fields left cells empty', async () => {
    const run = vi.fn<FromFilesRun>(async () => ({ tableId: 4, fallback: 'not-allowed', unfilled: [{ field: 'Merchant', records: 3 }, { field: 'Total', records: 1 }] }) as never)
    await mount(run)
    await choose([file('a.png')])
    await start()
    expect(toast.warning).toHaveBeenCalledWith('The AI model did not design the table, so the plain design was used')
    expect(toast.warning).toHaveBeenCalledWith('AI could not fill 4 cells', { description: 'In: Merchant, Total. Run these AI fields again from their column menu.' })
    expect(fromFiles.MAX_TOTAL_BYTES).toBe(100 << 20)
  })
})
