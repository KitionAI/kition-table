import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { lightGridTheme } from '../../configs'
import { SpriteManager } from '../../managers'
import { buildCellForField } from '../../useGridAdapter'
import type { DataField, DataRecord } from '../../../../types/dataDocument'
import { CellType } from './interface'
import type { ICellRenderProps, IRatingCell } from './interface'
import { ratingCellRenderer } from './ratingCellRenderer'

function ratingField(options: Record<string, unknown>): DataField {
  return {
    id: 1,
    user_id: 1,
    document_id: 1,
    table_id: 1,
    name: 'confidence',
    title: 'Confidence',
    type: 'rating',
    required: false,
    unique: false,
    readonly: false,
    is_primary: false,
    order: 1,
    options,
    created_at: '2026-11-15T00:00:00Z',
    updated_at: '2026-11-15T00:00:00Z',
  } as DataField
}

function record(value: number): DataRecord {
  return {
    id: 1,
    user_id: 1,
    document_id: 1,
    table_id: 1,
    row_key: 'r1',
    order: 1,
    values: { confidence: value },
    created_at: '2026-11-15T00:00:00Z',
    updated_at: '2026-11-15T00:00:00Z',
  } as DataRecord
}

function fakeContext() {
  return {
    save: vi.fn(),
    restore: vi.fn(),
    beginPath: vi.fn(),
    rect: vi.fn(),
    clip: vi.fn(),
    drawImage: vi.fn(),
    globalAlpha: 1,
  }
}

describe('ratingCellRenderer', () => {
  beforeEach(() => {
    // jsdom has no canvas and no image decoding: stubs let the sprite manager cache its sprites.
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
      () => ({ drawImage: vi.fn() }) as unknown as CanvasRenderingContext2D,
    )
    Object.defineProperty(HTMLImageElement.prototype, 'decode', {
      configurable: true,
      value: () => Promise.resolve(),
    })
  })

  afterEach(() => {
    vi.restoreAllMocks()
    delete (HTMLImageElement.prototype as { decode?: unknown }).decode
  })

  // Every icon a rating field can name (none, star, heart, thumbs) draws one sprite per step.
  it.each([
    [{ max: 5 }],
    [{ max: 5, icon: 'star' }],
    [{ max: 4, icon: 'heart' }],
    [{ max: 3, icon: 'thumbs' }],
  ])('draws a sprite for each step of a rated cell (%o)', (options) => {
    const cell = buildCellForField(ratingField(options), record(2)) as IRatingCell
    expect(cell.type).toBe(CellType.Rating)
    expect(cell.data).toBe(2)

    const spriteManager = new SpriteManager()
    const props = {
      theme: lightGridTheme,
      rect: { x: 0, y: 0, width: 160, height: 32 },
      spriteManager,
      hoverCellPosition: null,
    }

    // The first frame makes the sprites; the next frame draws them.
    ratingCellRenderer.draw(cell, { ...props, ctx: fakeContext() } as unknown as ICellRenderProps)
    const ctx = fakeContext()
    ratingCellRenderer.draw(cell, { ...props, ctx } as unknown as ICellRenderProps)

    expect(ctx.drawImage).toHaveBeenCalledTimes(cell.max)
  })
})
