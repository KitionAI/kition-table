/**
 * The grid's display list, format v1, as the page reads it: the same bytes `kition-table-display`
 * (crates/view/display) encodes in the engine. A list is what one band of the grid looks like, in
 * document CSS pixels; the page plays it (`player.ts`) and never lays anything out itself.
 *
 * The byte layout is the Rust crate's documentation; this reader follows it field for field and
 * refuses another version, as the engine's decoder does. The page and the engine ship in one
 * package, so there is no reader for another version.
 */

export const MAGIC = 'KTDL'
export const VERSION = 1
/** A header flag: the list is a draft, made while the view jumps (text measured from advances, no emoji). */
export const DRAFT = 1
export const HEADER_BYTES = 56
export const HIT_BYTES = 28

/** Which part of the grid a list draws (`Layer` in the crate). */
export enum Layer {
  Body = 0,
  Frozen = 1,
  Header = 2,
  HeaderFrozen = 3,
  RowHead = 4,
  View = 5,
}

export enum Opcode {
  Rect = 0x01,
  RRect = 0x02,
  RRectStroke = 0x03,
  Lines = 0x04,
  Text = 0x05,
  Tofu = 0x06,
  Emoji = 0x07,
  Icon = 0x08,
  Image = 0x09,
  Shadow = 0x0a,
  Clip = 0x0b,
  Unclip = 0x0c,
}

/** Where a text op's `x` is: the run's left end, its middle, or its right end. */
export enum Align {
  Left = 0,
  Center = 1,
  Right = 2,
}

export enum Axis {
  Horizontal = 0,
  Vertical = 1,
}

/** The face a run is set in; the engine chose it per character (`Face` in the crate). */
export enum Face {
  Sans = 0,
  Fallback = 1,
  Hangul = 2,
  Mono = 3,
}

export enum Fit {
  Cover = 0,
  Contain = 1,
}

export type BandKey = { layer: Layer; colBand: number; rowBand: number }

export type Header = {
  flags: number
  epoch: number
  theme: number
  resources: number
  key: BandKey
  /** Where the band starts and how large it is, in document CSS pixels. */
  x: number
  y: number
  width: number
  height: number
}

export type Op =
  | { op: Opcode.Rect; x: number; y: number; w: number; h: number; paint: number; snap: boolean }
  | { op: Opcode.RRect; x: number; y: number; w: number; h: number; r: number; paint: number }
  | { op: Opcode.RRectStroke; x: number; y: number; w: number; h: number; r: number; width: number; paint: number }
  /** `lines` holds (pos, from, to) triples. */
  | { op: Opcode.Lines; axis: Axis; paint: number; lines: Float32Array }
  | { op: Opcode.Text; x: number; y: number; align: Align; face: Face; weight: number; size: number; paint: number; text: string; width: number }
  | { op: Opcode.Tofu; x: number; y: number; w: number; h: number; paint: number }
  | { op: Opcode.Emoji; atlas: number; x: number; y: number; size: number }
  | { op: Opcode.Icon; icon: number; x: number; y: number; size: number; stroke: number; paint: number; solid: boolean }
  | { op: Opcode.Image; key: number; x: number; y: number; w: number; h: number; fit: Fit; radius: number }
  | { op: Opcode.Shadow; x: number; y: number; w: number; h: number; r: number; blur: number; paint: number }
  | { op: Opcode.Clip; x: number; y: number; w: number; h: number }
  | { op: Opcode.Unclip }

/** A clickable part of a cell or header; `row` and `col` are -1 for none. */
export type Hit = { kind: number; part: number; index: number; col: number; row: number; x: number; y: number; w: number; h: number }

export type DisplayList = {
  header: Header
  /** The palette as CSS colors, and as straight RGBA bytes. */
  colors: string[]
  rgba: Uint8Array
  ops: Op[]
  hits: Hit[]
  /** The encoded size, for the band cache's budget. */
  bytes: number
}

export class DisplayListError extends Error {}

const decoder = new TextDecoder('utf-8', { fatal: true })

function cssColor(r: number, g: number, b: number, a: number): string {
  return a === 255 ? `rgb(${r},${g},${b})` : `rgba(${r},${g},${b},${+(a / 255).toFixed(4)})`
}

/** Reads one display list; bytes of another version, or that are not a whole list, are refused. */
export function decodeList(source: ArrayBuffer | Uint8Array): DisplayList {
  const bytes = source instanceof Uint8Array ? source : new Uint8Array(source)
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const end = bytes.byteLength
  const need = (at: number, n: number) => {
    if (at + n > end) throw new DisplayListError('the display list is cut short')
  }
  need(0, HEADER_BYTES)
  if (String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]) !== MAGIC) throw new DisplayListError('not a display list')
  const version = dv.getUint16(4, true)
  if (version !== VERSION) throw new DisplayListError(`display list version ${version}; this build reads version ${VERSION}`)
  const layer = bytes[16]
  if (layer > Layer.View) throw new DisplayListError('the display list has an invalid layer')
  const header: Header = {
    flags: dv.getUint16(6, true),
    epoch: dv.getUint32(8, true),
    theme: dv.getUint16(12, true),
    resources: dv.getUint16(14, true),
    key: { layer: layer as Layer, colBand: bytes[17], rowBand: dv.getUint32(20, true) },
    x: dv.getFloat32(24, true),
    y: dv.getFloat32(28, true),
    width: dv.getFloat32(32, true),
    height: dv.getFloat32(36, true),
  }
  const paletteCount = dv.getUint16(40, true)
  const opCount = dv.getUint32(44, true)
  const stringBytes = dv.getUint32(48, true)
  const hitCount = dv.getUint32(52, true)
  let at = HEADER_BYTES
  need(at, paletteCount * 4)
  const rgba = bytes.slice(at, at + paletteCount * 4)
  const colors: string[] = new Array(paletteCount)
  for (let i = 0; i < paletteCount; i += 1) colors[i] = cssColor(rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2], rgba[i * 4 + 3])
  at += paletteCount * 4
  need(at, stringBytes)
  const strings = bytes.subarray(at, at + stringBytes)
  at += (stringBytes + 3) & ~3
  const f = (offset: number) => dv.getFloat32(at + offset, true)
  const ops: Op[] = new Array(opCount)
  for (let i = 0; i < opCount; i += 1) {
    need(at, 4)
    const code = bytes[at]
    const arg = bytes[at + 1]
    const paint = dv.getUint16(at + 2, true)
    let op: Op
    let size: number
    switch (code) {
      case Opcode.Rect:
        size = 20
        need(at, size)
        op = { op: code, x: f(4), y: f(8), w: f(12), h: f(16), paint, snap: (arg & 1) === 1 }
        break
      case Opcode.RRect:
        size = 24
        need(at, size)
        op = { op: code, x: f(4), y: f(8), w: f(12), h: f(16), r: f(20), paint }
        break
      case Opcode.RRectStroke:
        size = 28
        need(at, size)
        op = { op: code, x: f(4), y: f(8), w: f(12), h: f(16), r: f(20), width: f(24), paint }
        break
      case Opcode.Lines: {
        if (arg > Axis.Vertical) throw new DisplayListError('the display list has an invalid axis')
        need(at, 8)
        const n = dv.getUint32(at + 4, true)
        size = 8 + 12 * n
        need(at, size)
        const lines = new Float32Array(n * 3)
        for (let k = 0; k < n * 3; k += 1) lines[k] = dv.getFloat32(at + 8 + 4 * k, true)
        op = { op: code, axis: arg as Axis, paint, lines }
        break
      }
      case Opcode.Text: {
        size = 32
        need(at, size)
        if (arg > Align.Right) throw new DisplayListError('the display list has an invalid align')
        const face = bytes[at + 4]
        if (face > Face.Mono) throw new DisplayListError('the display list has an invalid face')
        const offset = dv.getUint32(at + 24, true)
        const length = dv.getUint16(at + 28, true)
        if (offset + length > stringBytes) throw new DisplayListError('the display list has an invalid string reference')
        let text: string
        try {
          text = decoder.decode(strings.subarray(offset, offset + length))
        } catch {
          throw new DisplayListError('the display list has an invalid string reference')
        }
        op = { op: code, align: arg as Align, paint, face: face as Face, weight: dv.getUint16(at + 6, true), x: f(8), y: f(12), size: f(16), width: f(20), text }
        break
      }
      case Opcode.Tofu:
        size = 20
        need(at, size)
        op = { op: code, x: f(4), y: f(8), w: f(12), h: f(16), paint }
        break
      case Opcode.Emoji:
        size = 20
        need(at, size)
        op = { op: code, atlas: dv.getUint32(at + 4, true), x: f(8), y: f(12), size: f(16) }
        break
      case Opcode.Icon:
        size = 24
        need(at, size)
        op = { op: code, solid: (arg & 1) === 1, paint, icon: dv.getUint16(at + 4, true), x: f(8), y: f(12), size: f(16), stroke: f(20) }
        break
      case Opcode.Image:
        size = 28
        need(at, size)
        if (arg > Fit.Contain) throw new DisplayListError('the display list has an invalid fit')
        op = { op: code, fit: arg as Fit, key: dv.getUint32(at + 4, true), x: f(8), y: f(12), w: f(16), h: f(20), radius: f(24) }
        break
      case Opcode.Shadow:
        size = 28
        need(at, size)
        op = { op: code, x: f(4), y: f(8), w: f(12), h: f(16), r: f(20), blur: f(24), paint }
        break
      case Opcode.Clip:
        size = 20
        need(at, size)
        op = { op: code, x: f(4), y: f(8), w: f(12), h: f(16) }
        break
      case Opcode.Unclip:
        size = 4
        op = { op: code }
        break
      default:
        throw new DisplayListError('the display list has an invalid opcode')
    }
    ops[i] = op
    at += size
  }
  if (hitCount > (end - at) / HIT_BYTES) throw new DisplayListError('the display list is cut short')
  const hits: Hit[] = new Array(hitCount)
  for (let i = 0; i < hitCount; i += 1, at += HIT_BYTES) {
    const kind = bytes[at]
    if (kind > 12) throw new DisplayListError('the display list has an invalid hit kind')
    const col = dv.getUint16(at + 4, true)
    const row = dv.getUint32(at + 8, true)
    hits[i] = { kind, part: bytes[at + 1], index: dv.getUint16(at + 2, true), col: col === 0xffff ? -1 : col, row: row === 0xffffffff ? -1 : row, x: f(12), y: f(16), w: f(20), h: f(24) }
  }
  if (at !== end) throw new DisplayListError('the display list has an invalid length')
  return { header, colors, rgba, ops, hits, bytes: end }
}
