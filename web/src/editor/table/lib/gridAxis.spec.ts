import { describe, expect, it } from 'vitest'

import { decodeGridAxis, decodeGridHits, gridGroupPoints, type GridAxis } from './gridAxis'

/** An axis in format v2, as the engine writes it (kition-table-arrange `Axis::encode`). */
function encode(axis: GridAxis, version = 2): Uint8Array {
  const json = axis.groups.length ? new TextEncoder().encode(JSON.stringify(axis.groups)) : new Uint8Array()
  const bytes = new Uint8Array(32 + axis.ids.length * 8 + json.length)
  const view = new DataView(bytes.buffer)
  bytes.set(new TextEncoder().encode('KTAX'))
  view.setUint16(4, version, true)
  ;[axis.epoch, axis.total, axis.ids.length, 0, axis.groups.length, json.length].forEach((value, index) => view.setUint32(8 + index * 4, value, true))
  axis.ids.forEach((id, index) => view.setFloat64(32 + index * 8, id, true))
  bytes.set(json, 32 + axis.ids.length * 8)
  return bytes
}

/** Search hits in format v1, as the engine writes them (kition-table-arrange `Hits::encode`): a bitmask over `fields` per row. */
function encodeHits(epoch: number, from: number, rows: number[][], fields: number, version = 1): Uint8Array {
  const stride = Math.ceil(fields / 8)
  const bytes = new Uint8Array(24 + rows.length * stride)
  const view = new DataView(bytes.buffer)
  bytes.set(new TextEncoder().encode('KTHT'))
  view.setUint16(4, version, true)
  ;[epoch, from, rows.length, fields].forEach((value, index) => view.setUint32(8 + index * 4, value, true))
  rows.forEach((columns, row) => columns.forEach((column) => (bytes[24 + row * stride + (column >> 3)] |= 1 << (column & 7))))
  return bytes
}

const header = (at: number, count: number, depth: number, value: unknown, id: string, collapsed = false) => ({ at, count, depth, value, id, collapsed })

describe('decodeGridAxis', () => {
  it('reads the ids and group headers', () => {
    const axis: GridAxis = {
      epoch: 3,
      total: 10,
      ids: [5, 2, 2 ** 52 + 1],
      groups: [header(0, 3, 0, 'Done', 'g0:"Done"', true)],
    }
    expect(decodeGridAxis(encode(axis))).toEqual(axis)
  })

  it('reads an axis inside a larger buffer', () => {
    const bytes = encode({ epoch: 1, total: 1, ids: [9], groups: [] })
    const padded = new Uint8Array(bytes.length + 3)
    padded.set(bytes, 3)
    expect(decodeGridAxis(padded.subarray(3)).ids).toEqual([9])
  })

  it('refuses another version and what is no axis', () => {
    expect(() => decodeGridAxis(encode({ epoch: 1, total: 0, ids: [], groups: [] }, 1))).toThrow('version 1')
    expect(() => decodeGridAxis(new Uint8Array(40))).toThrow('not a grid axis')
  })
})

describe('decodeGridHits', () => {
  it('reads the cells holding the search, by row of the axis and field id', () => {
    // Rows 4 and 6 of the axis: the 2nd field in row 4; the 1st and the 10th (past the first byte) in row 6.
    const bytes = encodeHits(3, 4, [[1], [], [0, 9]], 10)
    const fields = [11, 12, 13, 14, 15, 16, 17, 18, 19, 20]
    expect(decodeGridHits(bytes, fields)).toEqual([
      [4, 12],
      [6, 11],
      [6, 20],
    ])
    const padded = new Uint8Array(bytes.length + 5)
    padded.set(bytes, 5)
    expect(decodeGridHits(padded.subarray(5), fields)).toHaveLength(3)
  })

  it('refuses another version, other fields, and what is no hits', () => {
    expect(() => decodeGridHits(encodeHits(1, 0, [[0]], 1, 2), [7])).toThrow('version 2')
    expect(() => decodeGridHits(encodeHits(1, 0, [[0]], 1), [7, 8])).toThrow('other fields')
    expect(() => decodeGridHits(new Uint8Array(40), [7])).toThrow('not grid hits')
  })
})

describe('gridGroupPoints', () => {
  const axis = (count: number, groups: GridAxis['groups']): GridAxis => ({ epoch: 1, total: count, ids: Array.from({ length: count }, (_, index) => index + 1), groups })

  it('has no points without records', () => {
    expect(gridGroupPoints(axis(0, []))).toEqual([])
  })

  it('emits each header, then the run of records up to the next one', () => {
    expect(gridGroupPoints(axis(3, [header(0, 3, 0, 'Done', 'g0:"Done"')]))).toEqual([
      { id: 'g0:"Done"', type: 0, depth: 0, value: 'Done', isCollapsed: false },
      { type: 1, count: 3 },
    ])
  })

  it('nests inner headers under outer ones and keeps collapsed marks', () => {
    const points = gridGroupPoints(axis(3, [
      header(0, 2, 0, 'Done', 'g0:"Done"', true),
      header(0, 1, 1, 'Alice', 'g0:"Done"::g1:"Alice"'),
      header(1, 1, 1, 'Bob', 'g0:"Done"::g1:"Bob"'),
      header(2, 1, 0, 'Todo', 'g0:"Todo"'),
      header(2, 1, 1, 'Alice', 'g0:"Todo"::g1:"Alice"'),
    ]))
    expect(points.map((point) => ('depth' in point ? `H${point.depth}:${String(point.value)}` : 'count' in point ? `R${point.count}` : 'A'))).toEqual([
      'H0:Done',
      'H1:Alice',
      'R1',
      'H1:Bob',
      'R1',
      'H0:Todo',
      'H1:Alice',
      'R1',
    ])
    expect(points[0]).toMatchObject({ isCollapsed: true })
  })
})
