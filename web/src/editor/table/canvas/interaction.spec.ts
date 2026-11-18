// The grid's interaction on the page (case table.editor.grid/interaction): the linear rows reader,
// hit testing against the axes and the bands' hit records, where document rectangles show on the
// viewport, the selection model, the keyboard, the overlay's ops, autoscroll speed and when a
// press counts as a click.
import { describe, expect, it } from 'vitest'
import { Axes, type AxesInput } from './axes'
import { type BandKey, type DisplayList, type Hit, Layer, Opcode } from './displayList'
import { cellNear, fieldColumnCount, type GridHit, HitKind, hitTest, type HitSource, toScreen } from './hitTest'
import { autoscrollVelocity, GridInteraction, type GridHandlers, samePart } from './interaction'
import { controlBox, type OverlayStyle, overlayList, selectionOps } from './overlay'
import { GridRows, RowKind } from './rows'
import { allRowsSelected, cellSelection, moveCell, NO_SELECTION, selectAllRows, selectedColumns, selectedRecordIds, toggleRow } from './selection'

// Four fields (the first frozen) and the add-field column; a group of three records, its add row,
// a group of two, its add row.
const INPUT: AxesInput = {
  epoch: 3,
  rowRuns: [
    [48, 1],
    [40, 3],
    [32, 1],
    [48, 1],
    [40, 2],
    [32, 1],
  ],
  columns: [
    { x: 48, width: 200, frozen: true, fieldId: 11 },
    { x: 248, width: 150, frozen: false, fieldId: 12 },
    { x: 398, width: 150, frozen: false, fieldId: 13 },
    { x: 548, width: 150, frozen: false, fieldId: 14 },
    { x: 698, width: 100, frozen: false, fieldId: null },
  ],
  headerHeight: 32,
  rowHeadWidth: 48,
  bandRows: 32,
  bandWidth: 2048,
}

const ROWS = GridRows.of(3, [
  [RowKind.Group, 0],
  [RowKind.Record, 101],
  [RowKind.Record, 102],
  [RowKind.Record, 103],
  [RowKind.Append, 103],
  [RowKind.Group, 1],
  [RowKind.Record, 104],
  [RowKind.Record, 105],
  [RowKind.Append, 105],
])

const STYLE: OverlayStyle = {
  selection: 'rgba(12,131,117,0.1)',
  border: '#0c8375',
  hover: 'rgba(0,0,0,0.04)',
  rowHead: '#ffffff',
  control: '#797772',
  check: '#0c8375',
  checkMark: '#ffffff',
  icons: { drag: 1, expand: 2, check: 3 },
  controlSize: 16,
  checkboxRadius: 4,
  checkboxBorder: 1,
  checkMarkSize: 12,
  iconStroke: 1.5,
  activeWidth: 2,
  rangeWidth: 1,
}

function hit(kind: HitKind, row: number, col: number, x: number, y: number, w: number, h: number, index = 0): Hit {
  return { kind, part: 0, index, col, row, x, y, w, h }
}

function list(key: BandKey, hits: Hit[]): DisplayList {
  return { ...overlayList([], []), header: { flags: 0, epoch: 3, theme: 0, resources: 0, key, x: 0, y: 0, width: 0, height: 0 }, hits }
}

/** The bands' hit records as the engine lays them out for INPUT (grouped: no drag handle). */
function lists(axes: Axes): Map<string, DisplayList> {
  const head: Hit[] = []
  for (let row = 0; row < axes.rows; row += 1) {
    if (!ROWS.isRecord(row)) continue
    const y = axes.rowTop(row)
    head.push(hit(HitKind.Checkbox, row, -1, 0, y, 24, 40), hit(HitKind.Expand, row, -1, 24, y, 24, 40))
  }
  head.push(hit(HitKind.GroupToggle, 0, -1, 14, 46, 20, 20))
  const corner = [hit(HitKind.Checkbox, -1, -1, 0, 0, 24, 32)]
  const header = [hit(HitKind.ColumnMenu, -1, 1, 378, 10, 12, 12), hit(HitKind.ColumnEdge, -1, 1, 395, 0, 5, 32)]
  const body = [hit(HitKind.Checkbox, 2, 2, 463, 122, 20, 20)]
  const map = new Map<string, DisplayList>()
  map.set(`${Layer.RowHead}:0:0`, list({ layer: Layer.RowHead, colBand: 0, rowBand: 0 }, head))
  map.set(`${Layer.HeaderFrozen}:0:0`, list({ layer: Layer.HeaderFrozen, colBand: 0, rowBand: 0 }, corner))
  map.set(`${Layer.Header}:0:0`, list({ layer: Layer.Header, colBand: 0, rowBand: 0 }, header))
  map.set(`${Layer.Body}:0:0`, list({ layer: Layer.Body, colBand: 0, rowBand: 0 }, body))
  return map
}

function source(viewport = { scrollX: 0, scrollY: 0, width: 600, height: 400 }): HitSource {
  const axes = new Axes(INPUT)
  const bands = lists(axes)
  return { axes, rows: ROWS, viewport, list: (key) => bands.get(`${key.layer}:${key.colBand}:${key.rowBand}`) }
}

describe('grid rows', () => {
  const bytes = (version = 1, count = 3, extra = 0) => {
    const kinds = [RowKind.Group, RowKind.Record, RowKind.Append]
    const buffer = new ArrayBuffer(16 + 8 + count * 8 + extra)
    const view = new DataView(buffer)
    new Uint8Array(buffer).set([...new TextEncoder().encode('KTRW')])
    view.setUint16(4, version, true)
    view.setUint32(8, 7, true)
    view.setUint32(12, count, true)
    kinds.forEach((kind, i) => view.setUint8(16 + i, kind))
    ;[0, 42, 42].forEach((value, i) => view.setFloat64(24 + i * 8, value, true))
    return new Uint8Array(buffer)
  }

  it('reads what each linear row is, as the engine writes it', () => {
    const rows = GridRows.decode(bytes())
    expect([rows.epoch, rows.count]).toEqual([7, 3])
    expect([rows.kind(0), rows.groupIndex(0), rows.recordId(1), rows.appendAnchor(2), rows.rowOf(42)]).toEqual([RowKind.Group, 0, 42, 42, 1])
    expect([rows.firstRecord, rows.lastRecord]).toEqual([1, 1])
  })

  it('refuses another version and bytes that are not whole', () => {
    expect(() => GridRows.decode(bytes(2))).toThrow(/version 2/)
    expect(() => GridRows.decode(bytes(1, 3, 8))).toThrow(/length/)
    expect(() => GridRows.decode(new Uint8Array(8))).toThrow(/not grid rows/)
  })
})

describe('hit testing', () => {
  it('names the region, the row, the column and the engine part under a point', () => {
    const s = source()
    const at = (x: number, y: number) => {
      const h = hitTest(s, x, y)
      return [h.region, h.row, h.col, h.part?.kind ?? null]
    }
    expect(at(100, 100)).toEqual(['cell', 1, 0, null])
    expect(at(470, 128)).toEqual(['cell', 2, 2, HitKind.Checkbox])
    expect(at(10, 100)).toEqual(['rowHead', 1, -1, HitKind.Checkbox])
    expect(at(30, 100)).toEqual(['rowHead', 1, -1, HitKind.Expand])
    expect(at(20, 50)).toEqual(['group', 0, -1, HitKind.GroupToggle])
    expect(at(300, 50)).toEqual(['group', 0, 1, null])
    expect(at(300, 210)).toEqual(['appendRow', 4, 1, null])
    expect(at(10, 10)).toEqual(['corner', -1, -1, HitKind.Checkbox])
    expect(at(380, 14)).toEqual(['header', -1, 1, HitKind.ColumnMenu])
    expect(at(397, 14)).toEqual(['header', -1, 1, HitKind.ColumnEdge])
    expect(at(300, 14)).toEqual(['header', -1, 1, null])
    // The add-field column is not a field column: its header adds a field, its cells are nothing.
    expect(at(750, 14)[0]).toBe('none') // past the viewport's 600 px
    expect(hitTest(source({ scrollX: 0, scrollY: 0, width: 900, height: 400 }), 750, 14).region).toBe('appendColumn')
    expect(hitTest(source({ scrollX: 0, scrollY: 0, width: 900, height: 400 }), 750, 100).region).toBe('none')
    expect(at(300, 395)).toEqual(['none', -1, -1, null])
    expect(fieldColumnCount(s.axes)).toBe(4)
  })

  it('scrolls the cells but not the header, the row heads or the frozen column', () => {
    const s = source({ scrollX: 100, scrollY: 80, width: 600, height: 400 })
    expect([hitTest(s, 300, 100).row, hitTest(s, 300, 100).col]).toEqual([3, 2])
    expect([hitTest(s, 100, 100).row, hitTest(s, 100, 100).col]).toEqual([3, 0])
    expect(hitTest(s, 300, 14)).toMatchObject({ region: 'header', col: 2 })
  })

  it('holds a drag inside the record cells', () => {
    const s = source()
    expect(cellNear(s.axes, s.viewport, ROWS, 300, -40)).toEqual({ row: 1, col: 1 })
    expect(cellNear(s.axes, s.viewport, ROWS, 10, 100)).toEqual({ row: 1, col: 0 })
    expect(cellNear(s.axes, s.viewport, ROWS, 300, 50)).toEqual({ row: 1, col: 1 })
    expect(cellNear(s.axes, s.viewport, ROWS, 300, 230)).toEqual({ row: 3, col: 1 })
    expect(cellNear(s.axes, s.viewport, ROWS, 5000, 5000)).toEqual({ row: 7, col: 3 })
  })

  it('shows a document rectangle in up to four clipped, scrolled pieces', () => {
    const axes = new Axes(INPUT)
    const viewport = { scrollX: 100, scrollY: 50, width: 600, height: 400 }
    // A range from the frozen column into the scrolling ones: cut at the frozen edge.
    expect(toScreen(axes, viewport, { x: 48, y: 80, width: 350, height: 80 })).toEqual([
      { x: 48, y: 32, width: 200, height: 78 },
      { x: 248, y: 32, width: 50, height: 78 },
    ])
    // Scrolled out of view.
    expect(toScreen(axes, viewport, { x: 248, y: 80, width: 100, height: 40 })).toEqual([])
    // A whole column with its header: the header piece does not scroll down.
    expect(toScreen(axes, viewport, { x: 398, y: 0, width: 150, height: 400 })).toEqual([
      { x: 298, y: 0, width: 150, height: 32 },
      { x: 298, y: 32, width: 150, height: 318 },
    ])
  })
})

describe('selection', () => {
  it('moves over records only, and jumps to the edges', () => {
    expect(moveCell(ROWS, 4, { row: 3, col: 1 }, 0, 1)).toEqual({ row: 6, col: 1 })
    expect(moveCell(ROWS, 4, { row: 6, col: 1 }, 0, -1)).toEqual({ row: 3, col: 1 })
    expect(moveCell(ROWS, 4, { row: 1, col: 1 }, 0, -1)).toEqual({ row: 1, col: 1 })
    expect(moveCell(ROWS, 4, { row: 7, col: 3 }, 1, 1)).toEqual({ row: 7, col: 3 })
    expect(moveCell(ROWS, 4, { row: 2, col: 2 }, 0, 1, true)).toEqual({ row: 7, col: 2 })
    expect(moveCell(ROWS, 4, { row: 6, col: 2 }, -1, 0, true)).toEqual({ row: 6, col: 0 })
  })

  it('checks rows one by one, a run with Shift, and all from the header', () => {
    let selection = toggleRow(NO_SELECTION, 2)
    selection = toggleRow(selection, 6)
    expect(selectedRecordIds(selection, ROWS)).toEqual([102, 104])
    selection = toggleRow(selection, 2)
    expect(selection).toEqual({ kind: 'rows', ranges: [[6, 6]] })
    // Shift from row 1 to 7: the group header and add row in between are not records.
    selection = toggleRow(NO_SELECTION, 7, 1)
    expect(selectedRecordIds(selection, ROWS)).toEqual([101, 102, 103, 104, 105])
    expect(allRowsSelected(selection, ROWS)).toBe(true)
    expect(allRowsSelected(toggleRow(selection, 3), ROWS)).toBe(false)
    expect(selectAllRows(ROWS)).toEqual({ kind: 'rows', ranges: [[1, 7]] })
    expect(toggleRow(toggleRow(NO_SELECTION, 2), 2)).toEqual(NO_SELECTION)
  })

  it('names the records and columns a selection acts on', () => {
    const range = cellSelection({ row: 3, col: 2 }, { row: 1, col: 1 })
    expect(selectedRecordIds(range, ROWS)).toEqual([101, 102, 103])
    expect(selectedColumns(range, 4)).toEqual([1, 2])
    expect(selectedRecordIds({ kind: 'columns', anchor: 3, focus: 2 }, ROWS)).toEqual([101, 102, 103, 104, 105])
    expect(selectedColumns({ kind: 'columns', anchor: 3, focus: 2 }, 4)).toEqual([2, 3])
  })
})

describe('the overlay', () => {
  it('centres the row head controls in their slots at every row height', () => {
    for (const [height, offset] of [
      [32, 8],
      [40, 12],
      [56, 20],
      [88, 36],
    ]) {
      expect(controlBox({ x: 24, y: 100, w: 24, h: height }, 16)).toEqual({ x: 28, y: 100 + offset, width: 16, height: 16 })
    }
    // A control larger than its row keeps to the row's top.
    expect(controlBox({ x: 0, y: 10, w: 24, h: 12 }, 16).y).toBe(10)
  })

  it('draws a checked row with the brand fill and border and the mark on it', () => {
    const s = source()
    const { colors, ops } = selectionOps({ axes: s.axes, rows: ROWS, viewport: s.viewport, style: STYLE, selection: toggleRow(NO_SELECTION, 2), active: null, hover: null, source: s })
    const box = ops.find((op) => op.op === Opcode.RRect)
    const border = ops.find((op) => op.op === Opcode.RRectStroke)
    const mark = ops.find((op) => op.op === Opcode.Icon)
    expect(box && colors[box.paint]).toBe(STYLE.check)
    expect(border && colors[border.paint]).toBe(STYLE.check)
    expect(mark && mark.op === Opcode.Icon && [mark.icon, colors[mark.paint]]).toEqual([STYLE.icons.check, STYLE.checkMark])
    // Only checked: the checkbox alone, no expand control.
    expect(ops.filter((op) => op.op === Opcode.Icon)).toHaveLength(1)
  })

  it('shows every control over the hovered row and outlines the active cell', () => {
    const s = source()
    const hover = hitTest(s, 300, 100)
    const { colors, ops } = selectionOps({ axes: s.axes, rows: ROWS, viewport: s.viewport, style: STYLE, selection: cellSelection({ row: 2, col: 1 }), active: { row: 2, col: 1 }, hover, source: s })
    const icons = ops.filter((op) => op.op === Opcode.Icon)
    expect(icons.map((op) => op.op === Opcode.Icon && op.icon)).toEqual([STYLE.icons.expand, STYLE.icons.expand])
    const outline = ops.find((op) => op.op === Opcode.RRectStroke && op.width === STYLE.activeWidth)
    expect(outline && outline.op === Opcode.RRectStroke && [outline.x, outline.y, outline.w, outline.h, colors[outline.paint]]).toEqual([248, 120, 150, 40, STYLE.border])
    const tint = ops.find((op) => op.op === Opcode.Rect && colors[op.paint] === STYLE.hover)
    expect(tint && tint.op === Opcode.Rect && [tint.x, tint.y, tint.w, tint.h]).toEqual([0, 80, 698, 39])
  })

  it('checks the header box when every row is checked', () => {
    const s = source()
    const { ops } = selectionOps({ axes: s.axes, rows: ROWS, viewport: s.viewport, style: STYLE, selection: selectAllRows(ROWS), active: null, hover: null, source: s })
    const boxes = ops.filter((op) => op.op === Opcode.RRect)
    expect(boxes.some((op) => op.op === Opcode.RRect && op.y === 8 && op.x === 4)).toBe(true)
  })
})

describe('pointer rules', () => {
  const at = (region: GridHit['region'], row: number, col: number, part: Hit | null = null): GridHit => ({ region, row, col, part, docX: 0, docY: 0, x: 0, y: 0 })

  it('keeps a click released over the part it was pressed on and drops one that moved', () => {
    const caret = hit(HitKind.ColumnMenu, -1, 1, 0, 0, 12, 12)
    expect(samePart(at('header', -1, 1, caret), at('header', -1, 1, { ...caret }))).toBe(true)
    expect(samePart(at('header', -1, 1, caret), at('header', -1, 1, null))).toBe(false)
    expect(samePart(at('appendRow', 4, 1), at('appendRow', 4, 2))).toBe(false)
    expect(samePart(at('rowHead', 2, -1), at('cell', 2, 0))).toBe(false)
  })

  it('scrolls a drag faster the deeper it is past an edge of the scrolling cells', () => {
    const axes = new Axes(INPUT)
    const viewport = { width: 600, height: 400 }
    expect(autoscrollVelocity(axes, viewport, 400, 200)).toEqual([0, 0])
    const [, slow] = autoscrollVelocity(axes, viewport, 400, 380)
    const [, fast] = autoscrollVelocity(axes, viewport, 400, 460)
    expect(slow).toBeGreaterThan(0)
    expect(fast).toBeGreaterThan(slow)
    expect(autoscrollVelocity(axes, viewport, 400, 10000)[1]).toBe(2400)
    expect(autoscrollVelocity(axes, viewport, 400, 40)[1]).toBeLessThan(0)
    expect(autoscrollVelocity(axes, viewport, 590, 200)[0]).toBeGreaterThan(0)
    expect(autoscrollVelocity(axes, viewport, 260, 200)[0]).toBeLessThan(0)
  })
})

/** A GridView stand-in: the parts GridInteraction reads. */
function fakeView() {
  const s = source()
  const scroller = document.createElement('div')
  document.body.append(scroller)
  const view = {
    scroller,
    cache: { peek: (key: BandKey) => s.list(key) },
    resources: {},
    currentAxes: s.axes,
    viewport: { ...s.viewport, dpr: 1 },
    opened: { axes: INPUT, skeleton: { background: '#fff', line: '#eee' }, rows: ROWS, overlay: STYLE, groups: [] },
    overlayDraws: 0,
    scrolledTo: [] as Array<[number, number]>,
    onAxes: () => () => {},
    onFrame: () => () => {},
    setOverlay: () => {},
    redrawOverlay() {
      view.overlayDraws += 1
    },
    scrollTo(x: number, y: number) {
      view.scrolledTo.push([x, y])
      view.viewport = { ...view.viewport, scrollX: x, scrollY: y }
    },
    scrollBy: () => {},
  }
  return view
}

function keyboard(handlers: GridHandlers = {}) {
  const view = fakeView()
  const grid = new GridInteraction(view as unknown as ConstructorParameters<typeof GridInteraction>[0], handlers)
  const press = (key: string, init: KeyboardEventInit = {}) => grid.handleKey(new KeyboardEvent('keydown', { key, ...init }))
  return { view, grid, press }
}

describe('the keyboard', () => {
  it('moves the active cell with arrows and Tab, extends with Shift, jumps with Cmd/Ctrl', () => {
    const { grid, press } = keyboard()
    grid.setActive({ row: 1, col: 0 })
    press('ArrowRight')
    press('ArrowDown')
    expect(grid.active).toEqual({ row: 2, col: 1 })
    press('Tab')
    expect(grid.active).toEqual({ row: 2, col: 2 })
    press('Tab', { shiftKey: true })
    expect(grid.active).toEqual({ row: 2, col: 1 })
    press('ArrowDown', { shiftKey: true })
    press('ArrowDown', { shiftKey: true })
    expect(grid.selection).toEqual({ kind: 'cells', anchor: { row: 2, col: 1 }, focus: { row: 6, col: 1 } })
    expect(grid.active).toEqual({ row: 2, col: 1 })
    press('ArrowDown', { metaKey: true })
    expect(grid.active).toEqual({ row: 7, col: 1 })
    press('ArrowLeft', { ctrlKey: true })
    expect(grid.active).toEqual({ row: 7, col: 0 })
  })

  it('asks to edit on Enter, F2 and a typed character, and leaves keys to an open editor', () => {
    const edits: Array<[number, number, string | null]> = []
    const { grid, press } = keyboard({ onEdit: (cell, text) => edits.push([cell.row, cell.col, text]) })
    grid.setActive({ row: 3, col: 2 })
    expect(press('Enter')).toBe(true)
    expect(press('F2')).toBe(true)
    expect(press('q')).toBe(true)
    expect(edits).toEqual([
      [3, 2, null],
      [3, 2, null],
      [3, 2, 'q'],
    ])
    grid.setEditing(true)
    expect(press('ArrowDown')).toBe(false)
    expect(press('z', { metaKey: true })).toBe(false)
    expect(grid.active).toEqual({ row: 3, col: 2 })
  })

  it('collapses a range to the active cell on Escape, then selects all with Cmd/Ctrl+A', () => {
    const { grid, press } = keyboard()
    grid.setActive({ row: 1, col: 0 })
    grid.setActive({ row: 3, col: 2 }, { extend: true })
    expect(press('Escape')).toBe(true)
    expect(grid.selection).toEqual(cellSelection({ row: 1, col: 0 }))
    expect(press('Escape')).toBe(false)
    press('a', { metaKey: true })
    expect(grid.selection).toEqual(cellSelection({ row: 1, col: 0 }, { row: 7, col: 3 }))
  })

  it('runs undo and redo, deletes the selection, expands the active record on Space', () => {
    const calls: string[] = []
    const { grid, press } = keyboard({
      onUndo: () => calls.push('undo'),
      onRedo: () => calls.push('redo'),
      onDelete: (selection) => calls.push(`delete ${selection.kind}`),
      onExpandRecord: (id) => calls.push(`expand ${id}`),
    })
    press('z', { metaKey: true })
    press('Z', { metaKey: true, shiftKey: true })
    press('y', { ctrlKey: true })
    grid.setActive({ row: 6, col: 1 })
    press('Backspace')
    press(' ')
    expect(calls).toEqual(['undo', 'redo', 'redo', 'delete cells', 'expand 104'])
  })

  it('scrolls a moved cell into view and redraws the overlay once per change', () => {
    const { view, grid, press } = keyboard()
    grid.setActive({ row: 1, col: 1 })
    const draws = view.overlayDraws
    press('ArrowRight')
    press('ArrowRight')
    // Column 3 (548..698) is past the viewport's right edge (600): scrolled until it shows whole.
    expect(view.scrolledTo.at(-1)).toEqual([98, 0])
    expect(view.overlayDraws).toBe(draws + 2)
    grid.setActive({ row: 1, col: 3 })
    expect(view.overlayDraws).toBe(draws + 2)
    expect(grid.activeScreenArea()).toEqual({ x: 450, y: 80, width: 150, height: 40 })
  })
})
