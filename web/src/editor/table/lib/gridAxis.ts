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
 * v2): the shown records' ids in the view's order and its group headers. The engine is the only
 * place a view's filter, sorts, groups and search are applied; the cells holding the search are
 * asked for a run of rows at a time (`gridHits`, {@link decodeGridHits}).
 */
export type GridAxis = {
  /** Which arrangement this is: a later one replaces an earlier one. */
  epoch: number
  /** The table's records, shown or not. */
  total: number
  ids: number[]
  groups: GridAxisGroup[]
}

const MAGIC = 'KTAX'
const VERSION = 2
const HEADER = 32

/** Reads an axis; anything but format v2 is an error (the page and the engine ship together). */
export function decodeGridAxis(bytes: Uint8Array): GridAxis {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < HEADER || String.fromCharCode(...bytes.subarray(0, 4)) !== MAGIC) throw new Error('not a grid axis')
  const version = view.getUint16(4, true)
  if (version !== VERSION) throw new Error(`grid axis version ${version}, not ${VERSION}`)
  const field = (index: number) => view.getUint32(8 + index * 4, true)
  const [epoch, total, count, groupCount, jsonBytes] = [0, 1, 2, 4, 5].map(field)
  const ids = new Array<number>(count)
  for (let index = 0; index < count; index += 1) ids[index] = view.getFloat64(HEADER + index * 8, true)
  const jsonAt = HEADER + count * 8
  const groups = jsonBytes ? (JSON.parse(new TextDecoder().decode(bytes.subarray(jsonAt, jsonAt + jsonBytes))) as GridAxisGroup[]) : []
  if (groups.length !== groupCount) throw new Error('the grid axis group headers do not match their count')
  return { epoch, total, ids, groups }
}

/** Which rows' search hits to ask for (`gridHits`): `count` rows of the axis `epoch` from `from`, in the fields `fieldIds`. */
export type GridHitsParams = {
  epoch: number
  from: number
  count: number
  fieldIds: number[]
}

const HITS_MAGIC = 'KTHT'
const HITS_VERSION = 1
const HITS_HEADER = 24

/**
 * Reads the search hits of a run of rows (kition-table-arrange's hits, format v1): for each row, a
 * bitmask over the fields asked for. Answers `[index into the axis's ids, field id]` of each cell
 * whose text holds the search, in `fieldIds` order.
 */
export function decodeGridHits(bytes: Uint8Array, fieldIds: number[]): Array<[number, number]> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < HITS_HEADER || String.fromCharCode(...bytes.subarray(0, 4)) !== HITS_MAGIC) throw new Error('not grid hits')
  const version = view.getUint16(4, true)
  if (version !== HITS_VERSION) throw new Error(`grid hits version ${version}, not ${HITS_VERSION}`)
  const [from, count, fields] = [12, 16, 20].map((at) => view.getUint32(at, true))
  if (fields !== fieldIds.length) throw new Error('the grid hits are for other fields')
  const stride = Math.ceil(fields / 8)
  if (bytes.byteLength < HITS_HEADER + count * stride) throw new Error('the grid hits are cut short')
  const hits: Array<[number, number]> = []
  for (let row = 0; row < count; row += 1) {
    for (let column = 0; column < fields; column += 1) {
      if (bytes[HITS_HEADER + row * stride + (column >> 3)] & (1 << (column & 7))) hits.push([from + row, fieldIds[column]])
    }
  }
  return hits
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
