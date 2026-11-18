/**
 * What a display list's ids refer to, on the page: icon and emoji tiles (pixels the engine
 * rasterizes at the device size, so both sides show the same pixels: no path is stroked by the
 * browser, and WebKit has no COLRv1) and the attachment pictures the page decodes.
 *
 * The page names what it holds by a session number it sends with every `gridBands` call: the
 * engine sends each resource once per session. Dropping what is held (another device pixel
 * ratio, a batch that could not be read) starts a new session, so the engine sends it all again.
 */
import type { EmojiTile, IconTile } from './bandBatch'

type Tile = { image: CanvasImageSource; left: number; top: number }

export function emojiKey(atlas: number, size: number, dpr: number): string {
  return `${atlas}:${size}:${dpr}`
}

export function iconKey(id: number, size: number, stroke: number, solid: boolean, paint: number, dpr: number): string {
  return `${id}:${size}:${stroke}:${solid ? 1 : 0}:${paint}:${dpr}`
}

/** A palette entry's straight RGBA bytes packed as the resource table packs a color (R in the lowest byte). */
export function packedColor(rgba: Uint8Array, index: number): number {
  const at = index * 4
  return (rgba[at] | (rgba[at + 1] << 8) | (rgba[at + 2] << 16) | (rgba[at + 3] << 24)) >>> 0
}

function tileCanvas(width: number, height: number): HTMLCanvasElement | OffscreenCanvas {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(width, height)
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  return canvas
}

function toTile(tile: { width: number; height: number; left: number; top: number; rgba: Uint8Array }): Tile | null {
  if (tile.width === 0 || tile.height === 0) return null
  const canvas = tileCanvas(tile.width, tile.height)
  const ctx = canvas.getContext('2d') as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null
  if (!ctx) return null
  const pixels = new ImageData(tile.width, tile.height)
  pixels.data.set(tile.rgba)
  ctx.putImageData(pixels, 0, 0)
  return { image: canvas, left: tile.left, top: tile.top }
}

function newSession(): number {
  return Math.floor(Math.random() * 0x1_0000_0000) >>> 0
}

export class GridResources {
  private readonly icons = new Map<string, Tile>()
  private readonly tiles = new Map<string, Tile>()
  private readonly images = new Map<number, CanvasImageSource & { width: number; height: number }>()
  private sessionId = newSession()

  /** The session the page asks the engine for resources in. */
  get session(): number {
    return this.sessionId
  }

  /** An icon tile, keyed by the ratio the page asked for (the table holds it as an f32). */
  addIcon(tile: IconTile, dpr: number) {
    const made = toTile(tile)
    if (made) this.icons.set(iconKey(tile.id, tile.size, tile.stroke, tile.solid, tile.paint, dpr), made)
  }

  icon(id: number, size: number, stroke: number, solid: boolean, paint: number, dpr: number): Tile | undefined {
    return this.icons.get(iconKey(id, size, stroke, solid, paint, dpr))
  }

  /** An emoji tile, keyed by the ratio the page asked for. */
  addEmoji(tile: EmojiTile, dpr: number) {
    const made = toTile(tile)
    if (made) this.tiles.set(emojiKey(tile.atlas, tile.size, dpr), made)
  }

  emoji(atlas: number, size: number, dpr: number): Tile | undefined {
    return this.tiles.get(emojiKey(atlas, size, dpr))
  }

  setImage(key: number, image: CanvasImageSource & { width: number; height: number }) {
    this.images.set(key, image)
  }

  image(key: number) {
    return this.images.get(key)
  }

  /** Drops what depends on the device pixel ratio (the tiles) and starts a new session. */
  dropTiles() {
    this.icons.clear()
    this.tiles.clear()
    this.sessionId = newSession()
  }

  /** What the page holds may differ from what the engine sent (a batch went unread): a new session. */
  renewSession() {
    this.sessionId = newSession()
  }

  clear() {
    this.icons.clear()
    this.tiles.clear()
    this.images.clear()
    this.sessionId = newSession()
  }
}
