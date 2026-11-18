/**
 * The grid's linear rows on the page (`gridRows`): what each row of the axes is, as the engine laid
 * them out for one arrangement (`epoch`): a record, a group header, or an add-record row. The page
 * hit-tests, selects and names records with it; it never derives rows from the arrangement itself.
 *
 * Bytes (little endian):
 *
 * ```text
 * header  16 B   magic "KTRW" | version u16 = 1 | 0 u16 | epoch u32 | count u32
 * kinds   u8 x count (0 record, 1 group header, 2 add row), zero-padded to a multiple of 8
 * values  f64 x count: a record's id; a group header's index in the arrangement's groups; for an
 *                      add row, the id of the nearest record above it, or -1
 * ```
 */

export enum RowKind {
  Record = 0,
  Group = 1,
  Append = 2,
}

const MAGIC = 'KTRW'
const VERSION = 1
const HEADER = 16

export class GridRowsError extends Error {}

export class GridRows {
  private byRecord: Map<number, number> | null = null

  constructor(
    readonly epoch: number,
    private readonly kinds: Uint8Array,
    private readonly values: Float64Array,
  ) {}

  /** Reads `gridRows`' bytes; another version, or bytes that are not whole, are refused. */
  static decode(bytes: Uint8Array): GridRows {
    if (bytes.byteLength < HEADER || String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]) !== MAGIC) throw new GridRowsError('not grid rows')
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    const version = view.getUint16(4, true)
    if (version !== VERSION) throw new GridRowsError(`grid rows version ${version}; this build reads version ${VERSION}`)
    const epoch = view.getUint32(8, true)
    const count = view.getUint32(12, true)
    const kindsEnd = HEADER + Math.ceil(count / 8) * 8
    if (bytes.byteLength !== kindsEnd + count * 8) throw new GridRowsError('the grid rows have an invalid length')
    const kinds = bytes.slice(HEADER, HEADER + count)
    for (const kind of kinds) if (kind > RowKind.Append) throw new GridRowsError('the grid rows have an invalid kind')
    const values = new Float64Array(count)
    for (let index = 0; index < count; index += 1) values[index] = view.getFloat64(kindsEnd + index * 8, true)
    return new GridRows(epoch, kinds, values)
  }

  /** Rows from plain lists (tests, and the page's own fixtures). */
  static of(epoch: number, rows: Array<[RowKind, number]>): GridRows {
    return new GridRows(epoch, Uint8Array.from(rows.map(([kind]) => kind)), Float64Array.from(rows.map(([, value]) => value)))
  }

  get count(): number {
    return this.kinds.length
  }

  kind(row: number): RowKind | null {
    return row >= 0 && row < this.kinds.length ? (this.kinds[row] as RowKind) : null
  }

  isRecord(row: number): boolean {
    return this.kind(row) === RowKind.Record
  }

  /** The record a row shows, or null. */
  recordId(row: number): number | null {
    return this.kind(row) === RowKind.Record ? this.values[row] : null
  }

  /** A group header row's index in the arrangement's groups, or null. */
  groupIndex(row: number): number | null {
    return this.kind(row) === RowKind.Group ? this.values[row] : null
  }

  /** For an add row, the record above it whose group a new record joins (null when none). */
  appendAnchor(row: number): number | null {
    if (this.kind(row) !== RowKind.Append) return null
    const id = this.values[row]
    return id < 0 ? null : id
  }

  /** The row that shows `recordId`, or -1. */
  rowOf(recordId: number): number {
    if (!this.byRecord) {
      this.byRecord = new Map()
      for (let row = 0; row < this.kinds.length; row += 1) if (this.kinds[row] === RowKind.Record) this.byRecord.set(this.values[row], row)
    }
    return this.byRecord.get(recordId) ?? -1
  }

  /** The nearest record row from `row` on in direction `step` (+1 or -1), `row` itself included; -1 when none. */
  recordFrom(row: number, step: 1 | -1): number {
    for (let at = row; at >= 0 && at < this.kinds.length; at += step) if (this.kinds[at] === RowKind.Record) return at
    return -1
  }

  /** The first and the last record row (-1 when there is none). */
  get firstRecord(): number {
    return this.recordFrom(0, 1)
  }

  get lastRecord(): number {
    return this.recordFrom(this.kinds.length - 1, -1)
  }
}
