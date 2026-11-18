/**
 * What the engine's `gridBands` answers: one buffer holding the bands asked for and the resources
 * they need that the page does not hold yet (bindings/wasm `grid.rs` writes it), little endian,
 * every item starting on a 4-byte boundary:
 *
 * ```text
 * header  12 B  magic "KTDB" | version u16 = 1 | 0 u16 | item count u32
 * item          kind u8 | 0 u8 | 0 u16 | length u32 | payload, padded with zeros to 4 bytes
 *   1 band      a display list (format v1)
 *   2 icon      id u16 | 0 u16 | UTF-8 JSON { "grid", "paths", "fills" } (Tessera path data)
 *   3 emoji     atlas u32 | size f32 | dpr f32 | width u32 | height u32 | left i32 | top i32 | RGBA
 * ```
 */
import { decodeList, type DisplayList } from './displayList'
import type { EmojiTile, IconData } from './resources'

export const BATCH_MAGIC = 'KTDB'
export const BATCH_VERSION = 1

/** An attachment picture a band shows: its image key, its size as shown (0 when unknown) and where it is. */
export type ImageEntry = { key: number; width: number; height: number; url: string }

export type BandBatch = { bands: DisplayList[]; icons: Array<{ id: number; data: IconData }>; emoji: EmojiTile[]; bytes: number }

export function decodeBatch(source: ArrayBuffer | Uint8Array): BandBatch {
  const bytes = source instanceof Uint8Array ? source : new Uint8Array(source)
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < 12 || String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]) !== BATCH_MAGIC) throw new Error('not a band batch')
  const version = dv.getUint16(4, true)
  if (version !== BATCH_VERSION) throw new Error(`band batch version ${version}; this build reads version ${BATCH_VERSION}`)
  const count = dv.getUint32(8, true)
  const batch: BandBatch = { bands: [], icons: [], emoji: [], bytes: bytes.byteLength }
  const text = new TextDecoder()
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
      const id = dv.getUint16(start, true)
      batch.icons.push({ id, data: JSON.parse(text.decode(payload.subarray(4))) as IconData })
    } else if (kind === 3) {
      const width = dv.getUint32(start + 12, true)
      const height = dv.getUint32(start + 16, true)
      batch.emoji.push({
        atlas: dv.getUint32(start, true),
        size: dv.getFloat32(start + 4, true),
        dpr: dv.getFloat32(start + 8, true),
        width,
        height,
        left: dv.getInt32(start + 20, true),
        top: dv.getInt32(start + 24, true),
        rgba: payload.slice(28, 28 + width * height * 4),
      })
    } else throw new Error(`the band batch has an unknown item kind ${kind}`)
    at = start + ((length + 3) & ~3)
  }
  return batch
}
