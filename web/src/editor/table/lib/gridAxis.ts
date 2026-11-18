import { LinearRowType, type IGroupPoint } from '../grid/interface'

/** A group header of the engine's arrangement: it sits before the record at `at` and holds `count` records. */
export type GridAxisGroup = {
  at: number
  count: number
  depth: number
  collapsed: boolean
  /** `g0:<value>::g1:<value>…`, the id the editor keeps collapsed groups by. */
  id: string
  value: unknown
}

/**
 * The engine's arrangement of a view (`gridOpen`, `gridSync`; kition-table-arrange's axis, format
 * v1): the shown records' ids in the view's order, its group headers, and the cells holding the
 * search. The engine is the only place a view's filter, sorts, groups and search are applied.
 */
export type GridAxis = {
  /** Which arrangement this is: a later one replaces an earlier one. */
  epoch: number
  /** The table's records, shown or not. */
  total: number
  ids: number[]
  groups: GridAxisGroup[]
  /** `[index into ids, field id]` of each cell whose text holds the search. */
  hits: Array<[number, number]>
}

const MAGIC = 'KTAX'
const VERSION = 1
const HEADER = 32

/** Reads an axis; anything but format v1 is an error (the page and the engine ship together). */
export function decodeGridAxis(bytes: Uint8Array): GridAxis {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < HEADER || String.fromCharCode(...bytes.subarray(0, 4)) !== MAGIC) throw new Error('not a grid axis')
  const version = view.getUint16(4, true)
  if (version !== VERSION) throw new Error(`grid axis version ${version}, not ${VERSION}`)
  const field = (index: number) => view.getUint32(8 + index * 4, true)
  const [epoch, total, count, hitCount, groupCount, jsonBytes] = [0, 1, 2, 3, 4, 5].map(field)
  const ids = new Array<number>(count)
  for (let index = 0; index < count; index += 1) ids[index] = view.getFloat64(HEADER + index * 8, true)
  const hitsAt = HEADER + count * 8
  const hits = new Array<[number, number]>(hitCount)
  for (let index = 0; index < hitCount; index += 1) {
    hits[index] = [view.getUint32(hitsAt + index * 16, true), view.getFloat64(hitsAt + index * 16 + 8, true)]
  }
  const jsonAt = hitsAt + hitCount * 16
  const groups = jsonBytes ? (JSON.parse(new TextDecoder().decode(bytes.subarray(jsonAt, jsonAt + jsonBytes))) as GridAxisGroup[]) : []
  if (groups.length !== groupCount) throw new Error('the grid axis group headers do not match their count')
  return { epoch, total, ids, groups, hits }
}

/** The grid's group points for an axis: each header, then the run of records up to the next header. */
export function gridGroupPoints(axis: GridAxis): IGroupPoint[] {
  const points: IGroupPoint[] = []
  let next = 0
  for (let index = 0; index <= axis.groups.length; index += 1) {
    const group = axis.groups[index]
    const end = group ? group.at : axis.ids.length
    if (end > next) {
      points.push({ type: LinearRowType.Row, count: end - next })
      next = end
    }
    if (group) points.push({ id: group.id, type: LinearRowType.Group, depth: group.depth, value: group.value, isCollapsed: group.collapsed })
  }
  return points
}
