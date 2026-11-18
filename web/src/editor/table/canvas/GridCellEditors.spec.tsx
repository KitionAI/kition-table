// The new grid's popup editors (case table.editor.grid/editing; the intents of the old grid's
// TextEditor.spec for dates): a date cell opens the compact calendar, not the native picker, with
// the cell's day and time; a picked day commits once, as the stored instant, and the calendar does
// not come back; Escape closes it without a value; a choice list commits a single choice on the
// pick and keeps a multiple choice's picks as a draft.
import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it } from 'vitest'
import type { DataField, DataRecordValue } from '../../types/dataDocument'
import { GridCellPopup, monthDays, storedDate } from './GridCellEditors'
import type { CellPopup } from './gridEditing'

;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

let root: Root | null = null
let container: HTMLDivElement | null = null

afterEach(async () => {
  await act(async () => root?.unmount())
  root = null
  container?.remove()
  container = null
})

const field = (type: DataField['type'], options: DataField['options'] = null) => ({ id: 7, name: 'f', title: 'F', type, options }) as DataField

function popup(type: DataField['type'], value: DataRecordValue, options: DataField['options'] = null) {
  const calls = { commits: [] as DataRecordValue[], drafts: [] as DataRecordValue[], cancels: 0 }
  const made: CellPopup = {
    kind: type === 'date' || type === 'datetime' ? 'date' : 'select',
    field: field(type, options),
    recordId: 1,
    value,
    anchor: { x: 10, y: 10, width: 180, height: 40 },
    text: null,
    commit: (next) => calls.commits.push(next),
    draft: (next) => calls.drafts.push(next),
    cancel: () => {
      calls.cancels += 1
    },
  }
  return { made, calls }
}

async function render(made: CellPopup) {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  await act(async () => root?.render(createElement(GridCellPopup, { popup: made, locale: 'en-US' })))
}

const local = (iso: string) => {
  const date = new Date(iso)
  const pad = (value: number) => String(value).padStart(2, '0')
  return { day: `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`, time: `${pad(date.getHours())}:${pad(date.getMinutes())}` }
}

describe('the date editor', () => {
  it('opens the compact calendar with the cell\'s day and time, not the native picker', async () => {
    const { made } = popup('datetime', '2026-07-23T04:14:47Z')
    await render(made)
    const editor = document.querySelector('[data-testid="table-date-cell-editor"]')
    const shown = local('2026-07-23T04:14:47Z')
    expect(editor?.textContent).toContain(shown.day)
    expect(editor?.textContent).toContain(shown.time)
    expect(document.querySelector('input[type="datetime-local"]')).toBeNull()
    expect(document.querySelector('[data-testid="table-date-calendar"]')).not.toBeNull()
  })

  it('commits a picked day once, as the stored instant with the time kept, and closes the calendar', async () => {
    const { made, calls } = popup('datetime', '2026-07-23T04:14:47Z')
    await render(made)
    const day = document.querySelector('[data-testid="table-date-calendar"] [data-date]:not([data-date$="-23"])') as HTMLElement
    const picked = day.dataset.date as string
    await act(async () => {
      day.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
    })
    const base = new Date('2026-07-23T04:14:47Z')
    const [y, m, d] = picked.split('-').map(Number)
    expect(calls.commits).toEqual([new Date(y, m - 1, d, base.getHours(), base.getMinutes(), base.getSeconds()).toISOString()])
    expect(document.querySelector('[data-testid="table-date-calendar"]')).toBeNull()
  })

  it('stores a date field as its day, and Escape closes without a value', async () => {
    expect(storedDate('date', new Date(2026, 6, 23, 15, 0))).toBe('2026-07-23')
    const { made, calls } = popup('date', '2026-07-23')
    await render(made)
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    })
    expect(calls.cancels).toBe(1)
    expect(calls.commits).toEqual([])
  })

  it('shows six weeks from the Sunday before the first of the month', () => {
    const days = monthDays(new Date(2026, 6, 15))
    expect(days).toHaveLength(42)
    expect(days[0].getDay()).toBe(0)
    expect(days.some((day) => day.getMonth() === 6 && day.getDate() === 1)).toBe(true)
  })
})

describe('the choice list', () => {
  it('commits a single choice on the pick', async () => {
    const { made, calls } = popup('single_select', 'To do', { choices: ['To do', 'Done'] })
    await render(made)
    const done = document.querySelector('[data-choice="Done"]') as HTMLElement
    await act(async () => {
      done.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
    })
    expect(calls.commits).toEqual(['Done'])
  })

  it("keeps a multiple choice's picks as a draft until it closes", async () => {
    const { made, calls } = popup('multi_select', ['red'], { choices: ['red', 'green', 'blue'] })
    await render(made)
    const blue = document.querySelector('[data-choice="blue"]') as HTMLElement
    await act(async () => {
      blue.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
    })
    const red = document.querySelector('[data-choice="red"]') as HTMLElement
    await act(async () => {
      red.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
    })
    expect(calls.commits).toEqual([])
    expect(calls.drafts).toEqual([['red', 'blue'], ['blue']])
  })
})
