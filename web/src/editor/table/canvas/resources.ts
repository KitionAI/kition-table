/**
 * What a display list's ids refer to, on the page: Tessera icons (path data the engine sends once,
 * built into `Path2D`s), emoji tiles (pixels the engine rasterizes at the device size: WebKit has
 * no COLRv1, and both sides then show the same pixels), and attachment pictures the page decodes.
 */

/** An icon: SVG path data on Tessera's grid (`grid` units square). */
export type IconData = { grid: number; paths: string[]; fills: string[] }

/** An emoji at one font size and device pixel ratio: straight RGBA, placed from the snapped pen. */
export type EmojiTile = { atlas: number; size: number; dpr: number; width: number; height: number; left: number; top: number; rgba: Uint8Array }

type Icon = { grid: number; paths: Path2D[]; fills: Path2D[] }
type Tile = { image: CanvasImageSource; left: number; top: number }

export function emojiKey(atlas: number, size: number, dpr: number): string {
  return `${atlas}:${size}:${dpr}`
}

function tileCanvas(width: number, height: number): HTMLCanvasElement | OffscreenCanvas {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(width, height)
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  return canvas
}

export class GridResources {
  private readonly icons = new Map<number, Icon>()
  private readonly tiles = new Map<string, Tile>()
  private readonly images = new Map<number, CanvasImageSource & { width: number; height: number }>()

  addIcon(id: number, data: IconData) {
    this.icons.set(id, { grid: data.grid, paths: data.paths.map((path) => new Path2D(path)), fills: data.fills.map((path) => new Path2D(path)) })
  }

  icon(id: number): Icon | undefined {
    return this.icons.get(id)
  }

  /** How many icons the page holds: the engine sends the ones from this id on. */
  get iconCount(): number {
    return this.icons.size
  }

  addEmoji(tile: EmojiTile) {
    if (tile.width === 0 || tile.height === 0) return
    const canvas = tileCanvas(tile.width, tile.height)
    const ctx = canvas.getContext('2d') as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null
    if (!ctx) return
    const pixels = new ImageData(tile.width, tile.height)
    pixels.data.set(tile.rgba)
    ctx.putImageData(pixels, 0, 0)
    this.tiles.set(emojiKey(tile.atlas, tile.size, tile.dpr), { image: canvas, left: tile.left, top: tile.top })
  }

  emoji(atlas: number, size: number, dpr: number): Tile | undefined {
    return this.tiles.get(emojiKey(atlas, size, dpr))
  }

  /** The emoji tiles the page holds (`atlas:size:dpr`), so the engine does not send them again. */
  emojiKeys(): string[] {
    return [...this.tiles.keys()]
  }

  setImage(key: number, image: CanvasImageSource & { width: number; height: number }) {
    this.images.set(key, image)
  }

  image(key: number) {
    return this.images.get(key)
  }

  /** Drops what depends on the device pixel ratio (the emoji tiles). */
  dropTiles() {
    this.tiles.clear()
  }

  clear() {
    this.icons.clear()
    this.tiles.clear()
    this.images.clear()
  }
}
