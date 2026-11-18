import { describe, expect, it } from 'vitest'

import { decodeGridAxis, gridGroupPoints, type GridAxis } from './gridAxis'

/** An axis in format v1, as the engine writes it (kition-table-arrange `Axis::encode`). */
function encode(axis: GridAxis, version = 1): Uint8Array {
  const json = axis.groups.length ? new TextEncoder().encode(JSON.stringify(axis.groups)) : new Uint8Array()
  const bytes = new Uint8Array(32 + axis.ids.length * 8 + axis.hits.length * 16 + json.length)
  const view = new DataView(bytes.buffer)
  bytes.set(new TextEncoder().encode('KTAX'))
  view.setUint16(4, version, true)
  ;[axis.epoch, axis.total, axis.ids.length, axis.hits.length, axis.groups.length, json.length].forEach((value, index) => view.setUint32(8 + index * 4, value, true))
  axis.ids.forEach((id, index) => view.setFloat64(32 + index * 8, id, true))
  const hitsAt = 32 + axis.ids.length * 8
  axis.hits.forEach(([row, field], index) => {
    view.setUint32(hitsAt + index * 16, row, true)
    view.setFloat64(hitsAt + index * 16 + 8, field, true)
  })
  bytes.set(json, hitsAt + axis.hits.length * 16)
  return bytes
}

const header = (at: number, count: number, depth: number, value: unknown, id: string, collapsed = false) => ({ at, count, depth, value, id, collapsed })

describe('decodeGridAxis', () => {
  it('reads the ids, hits and group headers', () => {
    const axis: GridAxis = {
      epoch: 3,
      total: 10,
      ids: [5, 2, 2 ** 52 + 1],
      hits: [[1, 7]],
      groups: [header(0, 3, 0, 'Done', 'g0:"Done"', true)],
    }
    expect(decodeGridAxis(encode(axis))).toEqual(axis)
  })

  it('reads an axis inside a larger buffer', () => {
    const bytes = encode({ epoch: 1, total: 1, ids: [9], hits: [], groups: [] })
    const padded = new Uint8Array(bytes.length + 3)
    padded.set(bytes, 3)
    expect(decodeGridAxis(padded.subarray(3)).ids).toEqual([9])
  })

  it('refuses another version and what is no axis', () => {
    expect(() => decodeGridAxis(encode({ epoch: 1, total: 0, ids: [], hits: [], groups: [] }, 2))).toThrow('version 2')
    expect(() => decodeGridAxis(new Uint8Array(40))).toThrow('not a grid axis')
  })
})

describe('gridGroupPoints', () => {
  const axis = (count: number, groups: GridAxis['groups']): GridAxis => ({ epoch: 1, total: count, ids: Array.from({ length: count }, (_, index) => index + 1), hits: [], groups })

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
