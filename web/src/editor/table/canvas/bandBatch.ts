/**
 * What the engine's `gridBands` answers: one buffer holding the bands asked for and, in one
 * resource table, the resources they use that the page does not hold yet (bindings/wasm
 * `bands.rs` writes it), little endian, every item starting on a 4-byte boundary:
 *
 * ```text
 * header  12 B  magic "KTDB" | version u16 = 2 | 0 u16 | item count u32
 * item          kind u8 | 0 u8 | 0 u16 | length u32 | payload, padded with zeros to 4 bytes
 *   1 band      a display list (format v1)
 *   2 resources a resource table (at most one, after the bands)
 * ```
 *
 * The resource table is `kition-table-display`'s ("Resource table" in the crate's documentation;
 * `table.rs` writes it), read here field for field:
 *
 * ```text
 * header  12 B  magic "KTRS" | version u16 = 1 | resources u16 | entry count u32
 * entry         kind u8 | 0 u8 | 0 u16 | payload length u32 | payload, padded with zeros to 4 B
 *   1 icon      id u16 | solid u8 | 0 u8 | size f32 | stroke f32 | dpr f32 | paint RGBA u32
 *               | width u32 | height u32 | left i32 | top i32 | straight RGBA8
 *   2 emoji     atlas u32 | size f32 | dpr f32 | width u32 | height u32 | left i32 | top i32 | straight RGBA8
 *   3 image     key u32 | width u32 | height u32 | url byte length u32 | UTF-8 url
 * ```
 *
 * Another version of either is refused: the page and the engine ship in one package.
 */
import { decodeList, type DisplayList } from './displayList'

export const BATCH_MAGIC = 'KTDB'
export const BATCH_VERSION = 2
export const RESOURCE_MAGIC = 'KTRS'
export const RESOURCE_VERSION = 1

/** Pixels the engine rasterized: straight RGBA, `width` x `height`, placed `left`, `top` device pixels from a snapped point. */
export type TilePixels = { width: number; height: number; left: number; top: number; rgba: Uint8Array }

/** An icon drawn in one style at one device pixel ratio; `paint` is its color as packed RGBA (R in the lowest byte). */
export type IconTile = TilePixels & { id: number; size: number; stroke: number; solid: boolean; paint: number; dpr: number }

/** An emoji at one font size and device pixel ratio. */
export type EmojiTile = TilePixels & { atlas: number; size: number; dpr: number }

/** An attachment picture a band shows: its image key, its size as shown (0 when unknown) and where it is. */
export type ImageEntry = { key: number; width: number; height: number; url: string }

export type ResourceTable = { version: number; icons: IconTile[]; emoji: EmojiTile[]; images: ImageEntry[] }

export type BandBatch = { bands: DisplayList[]; resources: ResourceTable | null; bytes: number }

const text = new TextDecoder('utf-8', { fatal: true })

function magic(bytes: Uint8Array): string {
  return String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3])
}

/** A tile's fields from `at` (width, height, left, top, then the pixels). */
function tile(dv: DataView, bytes: Uint8Array, at: number, end: number): TilePixels {
  const width = dv.getUint32(at, true)
  const height = dv.getUint32(at + 4, true)
  const start = at + 16
  if (start + width * height * 4 !== end) throw new Error('a resource tile is not its size')
  return { width, height, left: dv.getInt32(at + 8, true), top: dv.getInt32(at + 12, true), rgba: bytes.slice(start, end) }
}

/** Reads a resource table. */
export function decodeResources(source: Uint8Array): ResourceTable {
  const bytes = source
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < 12 || magic(bytes) !== RESOURCE_MAGIC) throw new Error('not a resource table')
  const format = dv.getUint16(4, true)
  if (format !== RESOURCE_VERSION) throw new Error(`resource table version ${format}; this build reads version ${RESOURCE_VERSION}`)
  const table: ResourceTable = { version: dv.getUint16(6, true), icons: [], emoji: [], images: [] }
  const count = dv.getUint32(8, true)
  let at = 12
  for (let i = 0; i < count; i += 1) {
    if (at + 8 > bytes.byteLength) throw new Error('the resource table is cut short')
    const kind = bytes[at]
    const length = dv.getUint32(at + 4, true)
    const start = at + 8
    const end = start + length
    if (end > bytes.byteLength) throw new Error('the resource table is cut short')
    if (kind === 1) {
      table.icons.push({
        id: dv.getUint16(start, true),
        solid: bytes[start + 2] !== 0,
        size: dv.getFloat32(start + 4, true),
        stroke: dv.getFloat32(start + 8, true),
        dpr: dv.getFloat32(start + 12, true),
        paint: dv.getUint32(start + 16, true),
        ...tile(dv, bytes, start + 20, end),
      })
    } else if (kind === 2) {
      table.emoji.push({ atlas: dv.getUint32(start, true), size: dv.getFloat32(start + 4, true), dpr: dv.getFloat32(start + 8, true), ...tile(dv, bytes, start + 12, end) })
    } else if (kind === 3) {
      const urlLength = dv.getUint32(start + 12, true)
      if (start + 16 + urlLength !== end) throw new Error('an image entry is not its length')
      table.images.push({ key: dv.getUint32(start, true), width: dv.getUint32(start + 4, true), height: dv.getUint32(start + 8, true), url: text.decode(bytes.subarray(start + 16, end)) })
    } else throw new Error(`the resource table has an unknown entry kind ${kind}`)
    at = start + ((length + 3) & ~3)
  }
  if (at !== bytes.byteLength) throw new Error('the resource table has bytes after its entries')
  return table
}

export function decodeBatch(source: ArrayBuffer | Uint8Array): BandBatch {
  const bytes = source instanceof Uint8Array ? source : new Uint8Array(source)
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < 12 || magic(bytes) !== BATCH_MAGIC) throw new Error('not a band batch')
  const version = dv.getUint16(4, true)
  if (version !== BATCH_VERSION) throw new Error(`band batch version ${version}; this build reads version ${BATCH_VERSION}`)
  const count = dv.getUint32(8, true)
  const batch: BandBatch = { bands: [], resources: null, bytes: bytes.byteLength }
  let at = 12
  for (let i = 0; i < count; i += 1) {
    if (at + 8 > bytes.byteLength) throw new Error('the band batch is cut short')
    const kind = bytes[at]
    const length = dv.getUint32(at + 4, true)
    const start = at + 8
    if (start + length > bytes.byteLength) throw new Error('the band batch is cut short')
    const payload = bytes.subarray(start, start + length)
    if (kind === 1) batch.bands.push(decodeList(payload))
    else if (kind === 2) {
      if (batch.resources) throw new Error('the band batch has two resource tables')
      batch.resources = decodeResources(payload)
    } else throw new Error(`the band batch has an unknown item kind ${kind}`)
    at = start + ((length + 3) & ~3)
  }
  return batch
}
