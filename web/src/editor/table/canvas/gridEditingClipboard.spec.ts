// The grid's copy keys (regressions/issue-grid-copy-keys): Cmd+C and Cmd+X on a Mac, Ctrl+C and
// Ctrl+X elsewhere, write the selection's text through navigator.clipboard from the key press
// itself (WebKit starts no copy event of its own on the grid's empty textarea), once: a copy event
// the same key press still makes writes nothing more. A cell being edited keeps its own text copy.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DataField } from '../../types/dataDocument'
import type { GridData, GridRecord } from './gridData'
import { GridEditing } from './gridEditing'
import type { GridInteraction } from './interaction'
import { GridRows, RowKind } from './rows'
import { cellSelection, type CellRef, type GridSelection } from './selection'

const TITLE = { id: 1, name: 'title', title: 'Title', type: 'text', readonly: false, is_primary: true, order: 0, required: false, unique: false, user_id: 0, document_id: 0, table_id: 1, created_at: '', updated_at: '' } as DataField
const CELL: CellRef = { row: 0, col: 0 }

let writeText: ReturnType<typeof vi.fn>

beforeEach(() => {
  writeText = vi.fn(async () => {})
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
})

afterEach(() => {
  Reflect.deleteProperty(navigator, 'clipboard')
  document.body.innerHTML = ''
})

/** An editing over one record (id 12, title `Record 2`), its one cell selected, on a Mac or not. */
function setup(mac: boolean, selection: GridSelection = cellSelection(CELL)) {
  const records = vi.fn(async (ids: number[]): Promise<GridRecord[]> => ids.map((id) => ({ id, row_key: `r${id}`, values: { title: 'Record 2' } })))
  const transact = vi.fn(async (_ops: unknown[]) => {})
  const data = { tableId: 1, viewId: 1, field: (id: number) => (id === TITLE.id ? TITLE : undefined), records, transact, update: vi.fn(async () => {}) } as unknown as GridData
  const editing = new GridEditing(data, { onWritten: () => {} }, { mac })
  const grid = {
    active: CELL,
    rows: GridRows.of(1, [[RowKind.Record, 12]]),
    selection,
    columnCount: 1,
    style: null,
    view: { currentAxes: { columns: [{ fieldId: TITLE.id }], epoch: 1 } },
    setEditing: () => {},
    cellScreenArea: () => ({ x: 0, y: 32, width: 160, height: 40 }),
    handleKey: vi.fn(() => false),
  }
  ;(editing as unknown as { grid: GridInteraction }).grid = grid as unknown as GridInteraction
  document.body.append(editing.input)
  const internals = editing as unknown as { onKey(event: KeyboardEvent): void; onCopy(event: ClipboardEvent, cut: boolean): void }
  /** Presses `key` with the modifiers on the grid; answers whether the grid took it (prevented its default). */
  const press = (key: string, modifiers: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean } = {}) => {
    const event = new KeyboardEvent('keydown', { key, code: `Key${key.toUpperCase()}`, cancelable: true, ...modifiers })
    internals.onKey(event)
    return event.defaultPrevented
  }
  /** A copy event, as the browser makes one; answers whether the grid took it. */
  const copyEvent = () => {
    const event = new Event('copy', { cancelable: true }) as ClipboardEvent
    internals.onCopy(event, false)
    return event.defaultPrevented
  }
  const settle = () => new Promise((done) => setTimeout(done, 0))
  return { editing, grid, transact, press, copyEvent, settle }
}

describe("the grid's copy keys", () => {
  it('Ctrl+C off a Mac writes the selection through navigator.clipboard, once', async () => {
    const { press, copyEvent, settle } = setup(false)
    expect(press('c', { ctrlKey: true })).toBe(true)
    // A copy event the same key press makes is taken, and writes nothing more.
    expect(copyEvent()).toBe(true)
    await settle()
    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith('Record 2')
  })

  it('Cmd+C on a Mac writes it; Ctrl+C there, and Cmd (Meta) elsewhere, are not copy keys', async () => {
    const mac = setup(true)
    expect(mac.press('c', { ctrlKey: true })).toBe(false)
    expect(mac.press('c', { metaKey: true })).toBe(true)
    await mac.settle()
    expect(writeText).toHaveBeenCalledTimes(1)
    document.body.innerHTML = ''
    const other = setup(false)
    expect(other.press('c', { metaKey: true })).toBe(false)
    expect(other.press('c', { ctrlKey: true, shiftKey: true })).toBe(false)
    await other.settle()
    expect(writeText).toHaveBeenCalledTimes(1)
  })

  it('Ctrl+X writes the selection, then clears it in one transaction', async () => {
    const { press, transact, settle } = setup(false)
    expect(press('x', { ctrlKey: true })).toBe(true)
    await settle()
    await settle()
    expect(writeText).toHaveBeenCalledWith('Record 2')
    expect(transact).toHaveBeenCalledTimes(1)
    expect(transact.mock.calls[0][0]).toEqual([{ op: 'cells.set', tableId: 1, records: [{ rowKey: 'r12', values: { title: null } }] }])
  })

  it('a copy event from elsewhere (a menu) still copies when no key made it', async () => {
    const { copyEvent, settle } = setup(false)
    expect(copyEvent()).toBe(true)
    await settle()
    expect(writeText).toHaveBeenCalledWith('Record 2')
  })

  it('leaves the key to the browser with nothing selected, and to the cell editor while a cell is edited', async () => {
    const none = setup(false, { kind: 'none' })
    expect(none.press('c', { ctrlKey: true })).toBe(false)
    document.body.innerHTML = ''
    const { editing, press, settle } = setup(false)
    editing.edit(CELL, null)
    expect(editing.editing).toBe(true)
    expect(press('c', { ctrlKey: true })).toBe(false)
    await settle()
    expect(writeText).not.toHaveBeenCalled()
  })
})
