import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const drawGrid = vi.hoisted(() => vi.fn())
vi.mock('./renderers', () => ({ drawGrid }))

import { RegionType, type IMouseState } from './interface'
import { RenderLayer, type IRenderLayerProps } from './RenderLayer'

;(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true

let container: HTMLDivElement | null = null
let root: Root | null = null

beforeEach(() => {
  drawGrid.mockClear()
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
})

afterEach(async () => {
  await act(async () => root?.unmount())
  root = null
  container?.remove()
  container = null
})

const fixed = {
  theme: {},
  width: 400,
  height: 300,
  columns: [],
  coordInstance: { containerWidth: 400 },
  visibleRegion: {},
  scrollState: { scrollTop: 0, scrollLeft: 0, isScrolling: false },
  selection: {},
  activeCell: [0, 0],
  activeCellBound: null,
  hoverCellPosition: null,
  hoveredColumnResizeIndex: -1,
  forceRenderFlag: 'flag',
  getCellContent: () => ({}),
  real2RowIndex: (index: number) => index,
  getLinearRow: () => ({}),
}
const idle = {
  dragState: { isDragging: false },
  columnFreezeState: { isFreezing: false },
  columnResizeState: { columnIndex: -1 },
}

const draw = async (mouseState: IMouseState, state: object) => {
  await act(async () => {
    root?.render(
      createElement(RenderLayer, { ...fixed, ...idle, ...state, mouseState } as unknown as IRenderLayerProps),
    )
  })
}

const cell = (x: number, y: number, columnIndex = 1, rowIndex = 1): IMouseState => ({
  type: RegionType.Cell,
  x,
  y,
  columnIndex,
  rowIndex,
  isOutOfBounds: false,
})

describe('RenderLayer redraws during a drag', () => {
  it('a drag that selects cells redraws only when the hovered cell changes', async () => {
    const selecting = { isSelecting: true, isInteracting: true }
    await draw(cell(10, 10), selecting)
    const first = drawGrid.mock.calls.length
    await draw(cell(12, 11), selecting)
    await draw(cell(15, 13), selecting)
    expect(drawGrid.mock.calls.length).toBe(first)
    await draw(cell(220, 13, 2, 1), selecting)
    expect(drawGrid.mock.calls.length).toBe(first + 1)
  })

  it('a dragged column still follows the pointer', async () => {
    const dragging = { isSelecting: false, isInteracting: true, dragState: { isDragging: true } }
    await draw(cell(10, 10), dragging)
    const first = drawGrid.mock.calls.length
    await draw(cell(12, 10), dragging)
    expect(drawGrid.mock.calls.length).toBe(first + 1)
    expect(drawGrid.mock.calls.at(-1)?.[2].mouseState).toMatchObject({ x: 12, y: 10 })
  })
})
