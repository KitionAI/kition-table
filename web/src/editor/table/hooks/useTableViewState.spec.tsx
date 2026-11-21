import { act, createElement, useMemo, useState, type ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TableDataSourceProvider, type TableDataSource } from '../ports/TableDataSource'
import type { DataDocument, DataView } from '../../types/dataDocument'

import { useTableViewState, viewConfigOf, viewConfigUnchanged, viewSettingsOf } from './useTableViewState'

;(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true

function view(config: DataView['config']): DataView {
  return { id: 7, user_id: 1, document_id: 1, table_id: 3, title: 'Grid', type: 'grid', order: 0, locked: false, config, created_at: 0, updated_at: 0 }
}

function documentWith(activeView: DataView): DataDocument {
  return {
    id: 1,
    user_id: 1,
    workspace_root: '',
    path: 'Tasks.ktable',
    title: 'Tasks',
    description: '',
    icon: '',
    color: '',
    tables: [{ id: 3, user_id: 1, document_id: 1, name: 't', title: 'Tasks', description: '', order: 0, fields: [], views: [activeView], created_at: 0, updated_at: 0 }],
    created_at: 0,
    updated_at: 0,
  }
}

type State = ReturnType<typeof useTableViewState>

function Harness({ initial, onState }: { initial: DataDocument; onState: (state: State) => void }): ReactNode {
  const [document, setDocument] = useState<DataDocument | null>(initial)
  const [, setActiveViewId] = useState<number | null>(null)
  const activeTable = document?.tables?.[0] ?? null
  const tableViews = useMemo(() => activeTable?.views ?? [], [activeTable])
  onState(useTableViewState({ document, activeTable, activeView: tableViews[0] ?? null, tableViews, setDocument, setActiveViewId }))
  return null
}

describe('useTableViewState', () => {
  let root: Root
  let container: HTMLDivElement
  let update: ReturnType<typeof vi.fn>
  let state: State

  function mount(config: DataView['config']) {
    const source = { views: { update } } as unknown as TableDataSource
    act(() => {
      root.render(createElement(TableDataSourceProvider, { value: source }, createElement(Harness, { initial: documentWith(view(config)), onState: (next) => (state = next) })))
    })
  }

  async function settle() {
    for (let round = 0; round < 3; round += 1) {
      await act(async () => {
        vi.advanceTimersByTime(1000)
        await Promise.resolve()
      })
    }
  }

  /** Runs the timers due in `ms`, and what their promises do. */
  async function advance(ms: number) {
    await act(async () => {
      vi.advanceTimersByTime(ms)
      await Promise.resolve()
    })
  }

  /** Answers save `index` (its promise resolves with the view it was given) and lets the answer land, no timer run. */
  async function answer(answers: Array<() => void>, index: number) {
    answers[index]()
    for (let round = 0; round < 3; round += 1) await act(async () => await Promise.resolve())
  }

  /** Saves that answer only when the test says so. */
  function heldSaves(): Array<() => void> {
    const answers: Array<() => void> = []
    update.mockImplementation(({ config }: { config: DataView['config'] }) => new Promise<DataView>((resolve) => answers.push(() => resolve(view(config)))))
    return answers
  }

  beforeEach(() => {
    vi.useFakeTimers()
    container = document.createElement('div')
    root = createRoot(container)
    update = vi.fn(async ({ config }: { config: DataView['config'] }) => view(config))
  })

  afterEach(() => {
    act(() => root.unmount())
    vi.useRealTimers()
  })

  it.each([
    ['no config', null],
    ['an empty config', {}],
    ['a config with settings', { hidden_field_names: ['notes'], sort: { field_name: 'name', direction: 'desc' }, row_height: 'tall', frozen_column_count: 2 }],
  ])('opening a view with %s writes nothing', async (_name, config) => {
    mount(config as DataView['config'])
    await settle()
    expect(update).not.toHaveBeenCalled()
  })

  it('persists a setting the user changes, once', async () => {
    mount({})
    await settle()
    act(() => state.setRowHeightKey('tall'))
    await settle()
    expect(update).toHaveBeenCalledTimes(1)
    expect(update.mock.calls[0][0]).toMatchObject({ documentId: 1, tableId: 3, viewId: 7, config: { row_height: 'tall', frozen_column_count: 1 } })
    expect(state.rowHeightKey).toBe('tall')
  })

  it('writes nothing when a change is taken back before it persists', async () => {
    mount({})
    await settle()
    act(() => state.setRowHeightKey('tall'))
    act(() => state.setRowHeightKey('medium'))
    await settle()
    expect(update).not.toHaveBeenCalled()
  })

  // regressions/issue-view-save-race: a sort rule added, saved, and removed while that save was
  // under way came back with the save's answer, and the removal was never saved.
  it('keeps a change made while an earlier one is saving, and saves it after', async () => {
    mount({})
    await settle()
    const answers = heldSaves()
    const rule = { id: 'sort_1', field_name: 'received_at', direction: 'desc' as const }
    act(() => state.setSortItems([rule]))
    // The first save starts half a second after the change, and is under way.
    await advance(500)
    expect(update).toHaveBeenCalledTimes(1)
    expect(update.mock.calls[0][0].config.sort_items).toEqual([rule])
    // The rule is removed before that save answers; then it answers with the view holding the rule.
    act(() => state.setSortItems([]))
    await answer(answers, 0)
    expect(state.sortItems).toEqual([])
    // The removal is saved half a second later, and its answer is the view's.
    await advance(500)
    expect(update).toHaveBeenCalledTimes(2)
    expect(update.mock.calls[1][0].config.sort_items).toBeUndefined()
    await answer(answers, 1)
    await settle()
    expect(state.sortItems).toEqual([])
    expect(update).toHaveBeenCalledTimes(2)
  })

  it('takes the latest save, not an earlier one that answers after it', async () => {
    mount({})
    await settle()
    const answers = heldSaves()
    act(() => state.setRowHeightKey('tall'))
    await advance(500)
    act(() => state.setRowHeightKey('short'))
    await advance(500)
    expect(update).toHaveBeenCalledTimes(2)
    expect(update.mock.calls[1][0].config.row_height).toBe('short')
    // The second save answers first, then the first one.
    await answer(answers, 1)
    await answer(answers, 0)
    await settle()
    expect(state.rowHeightKey).toBe('short')
    expect(update).toHaveBeenCalledTimes(2)
  })
})

describe('viewConfigUnchanged', () => {
  it('takes the defaults the editor spells out for what a stored config leaves out', () => {
    const stored = view({ sort_items: [{ field_name: 'name', direction: 'asc' }] })
    expect(viewConfigUnchanged(stored, viewConfigOf(viewSettingsOf(stored)))).toBe(true)
    expect(viewConfigUnchanged(view(null), { hidden_field_names: [], row_height: 'medium', frozen_column_count: 1 })).toBe(true)
    expect(viewConfigUnchanged(stored, { ...viewConfigOf(viewSettingsOf(stored)), row_height: 'short' })).toBe(false)
  })
})
