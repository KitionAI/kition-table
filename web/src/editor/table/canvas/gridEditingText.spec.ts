// The text editor of an Enter, F2 or double click edit reads the cell's value after it shows
// (regressions/issue-cell-editor-early-typing): keys typed before the value arrives are never lost.
// The value goes before them, as typing adds to the value the editor shows with the caret at its
// end; an edit committed before the value arrives writes the value with what was typed after it.
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { DataField } from '../../types/dataDocument'
import type { GridData, GridRecord } from './gridData'
import { GridEditing, withValueBefore } from './gridEditing'
import type { GridInteraction } from './interaction'
import { GridRows, RowKind } from './rows'
import type { CellRef } from './selection'

const TITLE = { id: 1, name: 'title', title: 'Title', type: 'text', readonly: false, is_primary: true, order: 0, required: false, unique: false, user_id: 0, document_id: 0, table_id: 1, created_at: '', updated_at: '' } as DataField
const CELL: CellRef = { row: 0, col: 0 }

/** An editing over one record (id 12) whose title is `Record 2`; its reads answer when `answer` is called. */
function setup() {
  const held: Array<() => void> = []
  const records = vi.fn(
    (ids: number[]) => new Promise<GridRecord[]>((resolve) => held.push(() => resolve(ids.map((id) => ({ id, row_key: `r${id}`, values: { title: 'Record 2' } }))))),
  )
  const update = vi.fn(async () => {})
  const data = { tableId: 1, viewId: 1, field: (id: number) => (id === TITLE.id ? TITLE : undefined), records, update } as unknown as GridData
  const editing = new GridEditing(data, { onWritten: () => {} })
  const grid = {
    active: CELL,
    rows: GridRows.of(1, [[RowKind.Record, 12]]),
    selection: { kind: 'none' },
    columnCount: 1,
    style: null,
    view: { currentAxes: { columns: [{ fieldId: TITLE.id }], epoch: 1 } },
    setEditing: () => {},
    cellScreenArea: () => ({ x: 0, y: 32, width: 160, height: 40 }),
  }
  ;(editing as unknown as { grid: GridInteraction }).grid = grid as unknown as GridInteraction
  document.body.append(editing.input)
  /** Answers the reads made so far. */
  const answer = async () => {
    for (const resolve of held.splice(0)) resolve()
    await new Promise((done) => setTimeout(done, 0))
  }
  /** Types `text` at the caret, as a key press does. */
  const type = (text: string) => {
    const input = editing.input
    input.setRangeText(text, input.selectionStart, input.selectionEnd, 'end')
    input.dispatchEvent(new Event('input'))
  }
  return { editing, update, answer, type }
}

describe('the text editor of an Enter, F2 or double click edit', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('puts the value before what was typed before it arrived, the caret after the typing', async () => {
    const { editing, answer, type } = setup()
    editing.edit(CELL, null)
    expect(editing.editing).toBe(true)
    type(' later')
    await answer()
    expect(editing.input.value).toBe('Record 2 later')
    expect([editing.input.selectionStart, editing.input.selectionEnd]).toEqual([14, 14])
  })

  it('writes the value and what was typed after it when committed before the value arrives', async () => {
    const { editing, update, answer, type } = setup()
    editing.edit(CELL, null)
    type(' x')
    editing.commit()
    expect(editing.editing).toBe(false)
    expect(update).not.toHaveBeenCalled()
    await answer()
    expect(update).toHaveBeenCalledTimes(1)
    expect(update).toHaveBeenCalledWith(12, { title: 'Record 2 x' })
  })

  it('shows the value with the caret at its end when nothing was typed, and writes nothing unchanged', async () => {
    const { editing, update, answer } = setup()
    editing.edit(CELL, null)
    await answer()
    expect(editing.input.value).toBe('Record 2')
    expect([editing.input.selectionStart, editing.input.selectionEnd]).toEqual([8, 8])
    editing.commit()
    await answer()
    expect(update).not.toHaveBeenCalled()
  })

  it('adds what is typed once the value is there', async () => {
    const { editing, update, answer, type } = setup()
    editing.edit(CELL, null)
    await answer()
    type('!')
    editing.commit()
    await answer()
    expect(update).toHaveBeenCalledWith(12, { title: 'Record 2!' })
  })

  it('keeps the caret and selection in what was typed', () => {
    expect(withValueBefore('abc', 'xy', { start: 1, end: 2 })).toEqual({ text: 'abcxy', start: 4, end: 5 })
    expect(withValueBefore('abc', '', { start: 0, end: 0 })).toEqual({ text: 'abc', start: 3, end: 3 })
  })
})
