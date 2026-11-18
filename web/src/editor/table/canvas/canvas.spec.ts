// The page side of the grid's display list: the format v1 reader (field for field as the engine's
// `kition-table-display` writes it), the axes, the band cache, and the gridBands batch reader.
import { Axes, bandKeys } from './axes'
import { decodeBatch } from './bandBatch'
import { BandCache } from './bandCache'
import { Align, Axis, decodeList, type DisplayList, Face, Fit, HEADER_BYTES, Layer, Opcode } from './displayList'
import { linePixel, snap } from './player'

/** Writes a list the way the engine's encoder does (the byte format of `kition-table-display`). */
class Writer {
  bytes: number[] = []
  u8(v: number) {
    this.bytes.push(v & 255)
  }
  u16(v: number) {
    this.u8(v)
    this.u8(v >> 8)
  }
  u32(v: number) {
    const b = new Uint8Array(new Uint32Array([v >>> 0]).buffer)
    this.bytes.push(...b)
  }
  i32(v: number) {
    this.bytes.push(...new Uint8Array(new Int32Array([v]).buffer))
  }
  f32(...values: number[]) {
    for (const v of values) this.bytes.push(...new Uint8Array(new Float32Array([v]).buffer))
  }
  pad() {
    while (this.bytes.length % 4) this.bytes.push(0)
  }
  out() {
    return new Uint8Array(this.bytes)
  }
}

function sample(options: { version?: number; epoch?: number; rowBand?: number; layer?: Layer } = {}): Uint8Array {
  const strings = new TextEncoder().encode('Budget季度')
  const w = new Writer()
  w.bytes.push(...new TextEncoder().encode('KTDL'))
  w.u16(options.version ?? 1)
  w.u16(0)
  w.u32(options.epoch ?? 7)
  w.u16(1) // theme
  w.u16(3) // resources
  w.u8(options.layer ?? Layer.Body)
  w.u8(2) // col band
  w.u16(0)
  w.u32(options.rowBand ?? 5)
  w.f32(100, 200, 2048, 1024)
  w.u16(2) // palette
  w.u16(0)
  w.u32(12) // ops
  w.u32(strings.length)
  w.u32(1) // hits
  expect(w.bytes.length).toBe(HEADER_BYTES)
  w.bytes.push(255, 255, 255, 255, 12, 34, 56, 128)
  w.bytes.push(...strings)
  w.pad()
  const head = (op: number, arg: number, paint: number) => {
    w.u8(op)
    w.u8(arg)
    w.u16(paint)
  }
  head(Opcode.Rect, 1, 0)
  w.f32(1, 2, 3, 4)
  head(Opcode.RRect, 0, 1)
  w.f32(1, 2, 3, 4, 5)
  head(Opcode.RRectStroke, 0, 1)
  w.f32(1, 2, 3, 4, 5, 1)
  head(Opcode.Lines, Axis.Vertical, 1)
  w.u32(2)
  w.f32(10, 0, 32, 20, 0, 32)
  head(Opcode.Text, Align.Center, 1)
  w.u8(Face.Sans)
  w.u8(0)
  w.u16(600)
  w.f32(50, 60, 13, 44.5)
  w.u32(0)
  w.u16(6)
  w.u16(0)
  head(Opcode.Text, Align.Left, 1)
  w.u8(Face.Fallback)
  w.u8(0)
  w.u16(400)
  w.f32(95, 60, 13, 26)
  w.u32(6)
  w.u16(6)
  w.u16(0)
  head(Opcode.Tofu, 0, 1)
  w.f32(1, 2, 8, 10)
  head(Opcode.Emoji, 0, 0)
  w.u32(9)
  w.f32(5, 6, 13)
  head(Opcode.Icon, 1, 1)
  w.u16(4)
  w.u16(0)
  w.f32(1, 2, 16, 1.5)
  head(Opcode.Image, Fit.Contain, 0)
  w.u32(77)
  w.f32(1, 2, 30, 20, 4)
  head(Opcode.Clip, 0, 0)
  w.f32(0, 0, 10, 10)
  head(Opcode.Unclip, 0, 0)
  // hit
  w.u8(8)
  w.u8(0)
  w.u16(0)
  w.u16(0xffff)
  w.u16(0)
  w.u32(0xffffffff)
  w.f32(1, 2, 3, 4)
  return w.out()
}

describe('display list format v1', () => {
  it('reads every field where the engine writes it', () => {
    const list = decodeList(sample())
    expect(list.header).toEqual({ flags: 0, epoch: 7, theme: 1, resources: 3, key: { layer: Layer.Body, colBand: 2, rowBand: 5 }, x: 100, y: 200, width: 2048, height: 1024 })
    expect(list.colors).toEqual(['rgb(255,255,255)', 'rgba(12,34,56,0.502)'])
    expect(list.ops.map((op) => op.op)).toEqual([
      Opcode.Rect,
      Opcode.RRect,
      Opcode.RRectStroke,
      Opcode.Lines,
      Opcode.Text,
      Opcode.Text,
      Opcode.Tofu,
      Opcode.Emoji,
      Opcode.Icon,
      Opcode.Image,
      Opcode.Clip,
      Opcode.Unclip,
    ])
    expect(list.ops[0]).toEqual({ op: Opcode.Rect, x: 1, y: 2, w: 3, h: 4, paint: 0, snap: true })
    expect(list.ops[2]).toMatchObject({ r: 5, width: 1, paint: 1 })
    const lines = list.ops[3]
    expect(lines.op === Opcode.Lines && [lines.axis, ...lines.lines]).toEqual([Axis.Vertical, 10, 0, 32, 20, 0, 32])
    expect(list.ops[4]).toEqual({ op: Opcode.Text, align: Align.Center, paint: 1, face: Face.Sans, weight: 600, x: 50, y: 60, size: 13, width: 44.5, text: 'Budget' })
    expect(list.ops[5]).toMatchObject({ face: Face.Fallback, text: '季度' })
    expect(list.ops[7]).toEqual({ op: Opcode.Emoji, atlas: 9, x: 5, y: 6, size: 13 })
    expect(list.ops[8]).toEqual({ op: Opcode.Icon, solid: true, paint: 1, icon: 4, x: 1, y: 2, size: 16, stroke: 1.5 })
    expect(list.ops[9]).toEqual({ op: Opcode.Image, fit: Fit.Contain, key: 77, x: 1, y: 2, w: 30, h: 20, radius: 4 })
    expect(list.hits).toEqual([{ kind: 8, part: 0, index: 0, col: -1, row: -1, x: 1, y: 2, w: 3, h: 4 }])
    expect(list.bytes).toBe(sample().byteLength)
  })

  it('refuses another version, short or long bytes, and unknown opcodes', () => {
    expect(() => decodeList(sample({ version: 2 }))).toThrow(/version 2/)
    const bytes = sample()
    expect(() => decodeList(bytes.subarray(0, bytes.length - 1))).toThrow()
    expect(() => decodeList(new Uint8Array([...bytes, 0, 0, 0, 0]))).toThrow(/length/)
    const bad = sample()
    // The first op's opcode byte: header, palette (8 B), strings (12 B padded).
    bad[HEADER_BYTES + 8 + 12] = 0x7f
    expect(() => decodeList(bad)).toThrow(/opcode/)
    expect(() => decodeList(new TextEncoder().encode('nope'))).toThrow()
  })
})

describe('device pixel alignment (the rules both players share)', () => {
  it('snaps edges by rounding and puts a line in the device pixel it starts in', () => {
    expect(snap(10.3, 0, 2)).toBe(21)
    expect(snap(10.2, 0.5, 1.5)).toBe(15)
    expect(linePixel(31.5, 0, 1)).toBe(31)
    expect(linePixel(31.5, 0, 2)).toBe(63)
    // A coordinate whole in exact arithmetic stays in its pixel despite float error.
    expect(linePixel(0.1 + 0.2, 0, 10)).toBe(3)
  })
})

function axes(rowRuns: Array<[number, number]> = [[32, 100]]) {
  return new Axes({
    epoch: 1,
    rowRuns,
    columns: [
      { x: 72, width: 200, frozen: true },
      { x: 272, width: 200, frozen: false },
      { x: 472, width: 3000, frozen: false },
    ],
    headerHeight: 32,
    rowHeadWidth: 72,
    bandRows: 32,
    bandWidth: 2048,
  })
}

describe('axes', () => {
  it('finds rows by y over runs of heights', () => {
    const a = axes([
      [32, 10],
      [48, 1],
      [32, 5],
    ])
    expect(a.rows).toBe(16)
    expect(a.height).toBe(32 + 32 * 10 + 48 + 32 * 5)
    expect(a.rowTop(10)).toBe(32 + 320)
    expect(a.rowHeight(10)).toBe(48)
    expect(a.rowAt(32 + 320 + 47)).toBe(10)
    expect(a.rowAt(32 + 320 + 48)).toBe(11)
    expect(a.rowAt(10)).toBe(-1)
    expect(a.rowAt(a.height)).toBe(16)
    for (let row = 0; row < a.rows; row += 1) expect(a.rowAt(a.rowTop(row))).toBe(row)
  })

  it('splits rows and scrolling columns into bands', () => {
    const a = axes()
    expect(a.frozenWidth).toBe(200)
    expect(a.bodyLeft).toBe(272)
    expect(a.colBands).toBe(2)
    expect(a.columnAt(300)).toBe(1)
    expect(a.columnAt(10)).toBe(-1)
    expect(a.bandArea({ layer: Layer.Body, colBand: 1, rowBand: 1 })).toEqual({ x: 272 + 2048, y: 32 + 1024, width: 3200 - 2048, height: 1024 })
    const keys = bandKeys(a, 32, 32 + 1100, 272, 1000)
    expect(keys).toEqual([
      { layer: Layer.Body, colBand: 0, rowBand: 0 },
      { layer: Layer.Frozen, colBand: 0, rowBand: 0 },
      { layer: Layer.RowHead, colBand: 0, rowBand: 0 },
      { layer: Layer.Body, colBand: 0, rowBand: 1 },
      { layer: Layer.Frozen, colBand: 0, rowBand: 1 },
      { layer: Layer.RowHead, colBand: 0, rowBand: 1 },
    ])
  })
})

function list(rowBand: number, bytes: number, epoch = 1): DisplayList {
  return {
    header: { flags: 0, epoch, theme: 0, resources: 0, key: { layer: Layer.Body, colBand: 0, rowBand }, x: 0, y: 0, width: 0, height: 0 },
    colors: [],
    rgba: new Uint8Array(),
    ops: [],
    hits: [],
    bytes,
  }
}

describe('band cache', () => {
  it('drops the least recently used bands past its count or byte budget, never the kept ones', () => {
    const cache = new BandCache(3, 1000)
    cache.put(list(0, 100))
    cache.put(list(1, 100))
    cache.put(list(2, 100))
    cache.get({ layer: Layer.Body, colBand: 0, rowBand: 0 })
    cache.put(list(3, 100))
    expect(cache.has({ layer: Layer.Body, colBand: 0, rowBand: 1 })).toBe(false)
    expect(cache.has({ layer: Layer.Body, colBand: 0, rowBand: 0 })).toBe(true)
    cache.put(list(4, 900), new Set(['0:0:0']))
    expect(cache.bytes).toBeLessThanOrEqual(1000)
    expect(cache.has({ layer: Layer.Body, colBand: 0, rowBand: 0 })).toBe(true)
    expect(cache.has({ layer: Layer.Body, colBand: 0, rowBand: 4 })).toBe(true)
    cache.put(list(5, 10, 2))
    cache.keepEpoch(2)
    expect(cache.count).toBe(1)
  })
})

describe('gridBands batch', () => {
  it('reads bands, icons and emoji tiles', () => {
    const band = sample()
    const icon = new TextEncoder().encode(JSON.stringify({ grid: 24, paths: ['M0 0L24 24'], fills: [] }))
    const w = new Writer()
    w.bytes.push(...new TextEncoder().encode('KTDB'))
    w.u16(1)
    w.u16(0)
    w.u32(3)
    w.u8(1)
    w.u8(0)
    w.u16(0)
    w.u32(band.length)
    w.bytes.push(...band)
    w.pad()
    w.u8(2)
    w.u8(0)
    w.u16(0)
    w.u32(4 + icon.length)
    w.u16(3)
    w.u16(0)
    w.bytes.push(...icon)
    w.pad()
    w.u8(3)
    w.u8(0)
    w.u16(0)
    w.u32(28 + 2 * 1 * 4)
    w.u32(9)
    w.f32(13, 2)
    w.u32(2)
    w.u32(1)
    w.i32(0)
    w.i32(-28)
    w.bytes.push(1, 2, 3, 4, 5, 6, 7, 8)
    const batch = decodeBatch(w.out())
    expect(batch.bands).toHaveLength(1)
    expect(batch.bands[0].header.epoch).toBe(7)
    expect(batch.icons).toEqual([{ id: 3, data: { grid: 24, paths: ['M0 0L24 24'], fills: [] } }])
    expect(batch.emoji).toEqual([{ atlas: 9, size: 13, dpr: 2, width: 2, height: 1, left: 0, top: -28, rgba: new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]) }])
  })
})
