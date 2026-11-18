import { describe, expect, it } from 'vitest'
import { BandCache } from './bandCache'
import { type BandKey, type DisplayList, Layer } from './displayList'
import { addButtonArea } from './gridEditing'
import { showsRecords } from './gridSync'
import { GridRows, RowKind } from './rows'

function list(key: BandKey, bytes = 10): DisplayList {
  return { header: { flags: 0, epoch: 1, theme: 0, resources: 0, key, x: 0, y: 0, width: 0, height: 0 }, colors: [], rgba: new Uint8Array(), ops: [], hits: [], bytes }
}

const body = (rowBand: number): BandKey => ({ layer: Layer.Body, colBand: 0, rowBand })

// Bands of two rows: a group header and record 11; records 12 and 13; the add row and record 14.
const ROWS = GridRows.of(1, [
  [RowKind.Group, 0],
  [RowKind.Record, 11],
  [RowKind.Record, 12],
  [RowKind.Record, 13],
  [RowKind.Append, 13],
  [RowKind.Record, 14],
])

describe('stale bands after a write', () => {
  it('keeps drawing a stale band until its new list comes, but asks for it again', () => {
    const cache = new BandCache()
    const old = list(body(0))
    cache.put(old)
    cache.put(list(body(1)))
    expect(cache.markStale((key) => key.rowBand === 0)).toBe(1)
    // Not here for the view's planning: it is fetched again.
    expect(cache.has(body(0))).toBe(false)
    expect(cache.peek(body(0))).toBeUndefined()
    // Still drawn, and still hit tested, meanwhile.
    expect(cache.get(body(0))).toBe(old)
    expect(cache.drawn(body(0))).toBe(old)
    expect(cache.has(body(1))).toBe(true)
    // Marking it again counts nothing; its new list makes it current.
    expect(cache.markStale((key) => key.rowBand === 0)).toBe(0)
    const fresh = list(body(0))
    cache.put(fresh)
    expect(cache.has(body(0))).toBe(true)
    expect(cache.peek(body(0))).toBe(fresh)
  })

  it('forgets that a band was stale when it leaves the cache', () => {
    const cache = new BandCache()
    cache.put(list(body(0)))
    cache.markStale(() => true)
    cache.delete(body(0))
    cache.put(list(body(0)))
    expect(cache.has(body(0))).toBe(true)
    cache.markStale(() => true)
    cache.clear()
    cache.put(list(body(0)))
    expect(cache.has(body(0))).toBe(true)
  })

  it('names the bands of rows that show a written record, in every layer but the header strips', () => {
    const shows = showsRecords(ROWS, 2, new Set([12]))
    expect(shows(body(1))).toBe(true)
    expect(shows({ layer: Layer.Frozen, colBand: 0, rowBand: 1 })).toBe(true)
    expect(shows({ layer: Layer.RowHead, colBand: 0, rowBand: 1 })).toBe(true)
    expect(shows(body(0))).toBe(false)
    expect(shows(body(2))).toBe(false)
    expect(shows({ layer: Layer.Header, colBand: 0, rowBand: 0 })).toBe(false)
    expect(shows({ layer: Layer.HeaderFrozen, colBand: 0, rowBand: 0 })).toBe(false)
    // The add row is not record 13's row: only record 14 is in the last band.
    expect(showsRecords(ROWS, 2, new Set([13]))(body(2))).toBe(false)
    expect(showsRecords(ROWS, 2, new Set([14]))(body(2))).toBe(true)
    // A write that does not say which records: every band of rows.
    const all = showsRecords(ROWS, 2, null)
    expect([0, 1, 2].every((band) => all(body(band)))).toBe(true)
    expect(all({ layer: Layer.Header, colBand: 0, rowBand: 0 })).toBe(false)
  })
})

describe('the attachment add button', () => {
  it('sits at the active cell left, centred on its height, as the old grid drew it', () => {
    expect(addButtonArea({ x: 100, y: 40, width: 200, height: 32 })).toEqual({ x: 106, y: 46, width: 20, height: 20 })
    expect(addButtonArea({ x: 0, y: 0, width: 200, height: 88 })).toEqual({ x: 6, y: 34, width: 20, height: 20 })
  })
})
