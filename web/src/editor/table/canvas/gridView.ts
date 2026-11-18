/**
 * The grid on the page: a native scroll container (real scroll bars, trackpad inertia) over two
 * canvases fixed on the viewport. The content canvas plays the engine's display lists, band by
 * band, from a cache the view keeps filled around the viewport; the overlay canvas above it is for
 * what follows the pointer and the selection, so drawing them never repaints the content.
 *
 * - Scrolling only records the position; drawing happens in `requestAnimationFrame`.
 * - Bands come from the engine's `gridBands` (`GridSource`): at most one request on its way, at
 *   most `MAX_BANDS_PER_CALL` bands in it, the visible ones first, then one screen above and below
 *   and up to two ahead in the direction and speed of the scroll. A band not here yet is drawn as
 *   its skeleton (background and lines from the axes) until it arrives.
 * - The real `devicePixelRatio` (not rounded up); the canvases are `round(css * dpr)` device pixels
 *   and are rebuilt when it changes, which drops the emoji tiles and every band.
 * - A scroll extent beyond what a browser lays out is mapped onto `MAX_SCROLL_PX` proportionally.
 */
import { type Area, Axes, type AxesInput, bandKeys, keyId } from './axes'
import { decodeBatch } from './bandBatch'
import { BandCache } from './bandCache'
import { type BandKey, Layer } from './displayList'
import { GridFonts } from './gridFonts'
import { play } from './player'
import { GridResources } from './resources'

export const MAX_BANDS_PER_CALL = 2
/** The tallest or widest scroll extent the view lays out; larger grids map onto it. */
export const MAX_SCROLL_PX = 15_000_000
/** Scroll speed (CSS px per second) from which the view prefetches two screens ahead, not one. */
const FAST_SCROLL = 3000

/** The colors of a band not here yet, from the engine's theme. */
export type Skeleton = { background: string; line: string }

export type GridOpened = { axes: AxesInput; skeleton: Skeleton }

/** What the view asks the engine (through `KitEngine.invoke`). */
export type GridSource = {
  /** The axes of the arrangement now (`gridOpen`). */
  open(): Promise<GridOpened>
  /**
   * `gridBands`: the bands `keys` of arrangement `epoch` at `dpr`, with the icons from id `icons` on
   * and the emoji tiles not in `tiles` (`atlas:size:dpr`), as one batch (`bandBatch.ts`).
   */
  bands(epoch: number, keys: BandKey[], dpr: number, icons: number, tiles: string[]): Promise<Uint8Array>
}

/** The timings a performance test reads (`window.__ktableGridStats` on the development page). */
export type GridStats = {
  /** One entry per content frame: when, how long playing took, whether a band was a skeleton. */
  frames: Array<{ at: number; playMs: number; skeleton: boolean; bytes: number }>
  /** One entry per `gridBands` call: bands asked, round trip, bytes answered. */
  requests: Array<{ bands: number; ms: number; bytes: number }>
  /** When the first frame with every visible band's content was drawn (ms after the page's navigation started). */
  firstContent: number | null
}

type Region = { layer: 'body' | 'frozen' | 'rowHead' | 'header' | 'headerFrozen'; clip: Area; ox: number; oy: number }

export type GridViewOptions = {
  /** The package's `fonts/` folder, as a URL. */
  fontsBase: string
  /** Keeps `GridStats` (the development page's performance hook). */
  stats?: boolean
}

export class GridView {
  readonly element: HTMLDivElement
  readonly scroller: HTMLDivElement
  readonly content: HTMLCanvasElement
  readonly overlay: HTMLCanvasElement
  readonly stats: GridStats | null
  readonly resources = new GridResources()
  readonly cache = new BandCache()
  private readonly spacer: HTMLDivElement
  private readonly fonts: GridFonts
  private axes: Axes | null = null
  private skeleton: Skeleton = { background: '#fff', line: '#e5e5e5' }
  private dpr = 1
  private width = 0
  private height = 0
  private scrollX = 0
  private scrollY = 0
  private velocity = 0
  private lastScroll = { y: 0, at: 0 }
  private frame = 0
  private inFlight = false
  private received = 0
  private destroyed = false
  private overlayPainter: ((ctx: CanvasRenderingContext2D, dpr: number) => void) | null = null
  private readonly cleanup: Array<() => void> = []

  constructor(
    host: HTMLElement,
    private readonly source: GridSource,
    options: GridViewOptions,
  ) {
    this.stats = options.stats ? { frames: [], requests: [], firstContent: null } : null
    this.fonts = new GridFonts(options.fontsBase, () => this.invalidate())
    this.element = document.createElement('div')
    this.element.style.cssText = 'position:relative;width:100%;height:100%;overflow:hidden'
    this.scroller = document.createElement('div')
    this.scroller.dataset.ktableGridStage = 'true'
    this.scroller.style.cssText = 'position:absolute;inset:0;overflow:auto;overscroll-behavior:contain'
    this.spacer = document.createElement('div')
    this.spacer.style.cssText = 'width:1px;height:1px'
    this.scroller.append(this.spacer)
    this.content = document.createElement('canvas')
    this.content.dataset.ktableGridContent = 'true'
    this.overlay = document.createElement('canvas')
    this.overlay.dataset.ktableGridOverlay = 'true'
    for (const canvas of [this.content, this.overlay]) canvas.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none'
    this.element.append(this.scroller, this.content, this.overlay)
    host.append(this.element)

    const onScroll = () => this.onScroll()
    this.scroller.addEventListener('scroll', onScroll, { passive: true })
    this.cleanup.push(() => this.scroller.removeEventListener('scroll', onScroll))
    const resize = new ResizeObserver(() => this.resize())
    resize.observe(this.scroller)
    this.cleanup.push(() => resize.disconnect())
    this.watchDpr()
  }

  /** Loads Inter and the axes, then draws. */
  async open(): Promise<void> {
    const [opened] = await Promise.all([this.source.open(), this.fonts.start()])
    this.setAxes(opened)
  }

  /** New axes (a new arrangement): bands of another epoch go. */
  setAxes(opened: GridOpened) {
    this.axes = new Axes(opened.axes)
    this.skeleton = opened.skeleton
    this.cache.keepEpoch(this.axes.epoch)
    this.resize()
  }

  /** Every face loaded (the tests compare pixels only then). */
  allFonts(): Promise<void> {
    return this.fonts.all()
  }

  /** What the overlay canvas shows; it is redrawn without touching the content. */
  setOverlay(painter: ((ctx: CanvasRenderingContext2D, dpr: number) => void) | null) {
    this.overlayPainter = painter
    this.drawOverlay()
  }

  /** Scrolls the document point (`x`, `y`) of the scrolling area to the viewport's top left. */
  scrollTo(x: number, y: number) {
    const map = this.scrollMap()
    this.scroller.scrollLeft = x / map.x
    this.scroller.scrollTop = y / map.y
    this.onScroll()
  }

  /** Whether every band the viewport shows is here (no skeleton). */
  complete(): boolean {
    return this.needed().every((key) => this.cache.has(key))
  }

  /** Whether a `gridBands` request is on its way (the view fetches bands one request at a time). */
  get fetching(): boolean {
    return this.inFlight
  }

  /** The bands the viewport shows now (header strips first). */
  neededKeys(): BandKey[] {
    return this.needed()
  }

  get viewport(): { scrollX: number; scrollY: number; width: number; height: number; dpr: number } {
    return { scrollX: this.scrollX, scrollY: this.scrollY, width: this.width, height: this.height, dpr: this.dpr }
  }

  get currentAxes(): Axes | null {
    return this.axes
  }

  /** Draws again (a face loaded, a resource arrived). */
  invalidate() {
    if (!this.frame && !this.destroyed) this.frame = requestAnimationFrame((at) => this.draw(at))
  }

  destroy() {
    this.destroyed = true
    if (this.frame) cancelAnimationFrame(this.frame)
    for (const undo of this.cleanup) undo()
    this.element.remove()
  }

  private watchDpr() {
    const media = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`)
    const changed = () => {
      media.removeEventListener('change', changed)
      this.resources.dropTiles()
      this.cache.clear()
      this.resize()
      this.watchDpr()
    }
    media.addEventListener('change', changed)
    this.cleanup.push(() => media.removeEventListener('change', changed))
  }

  /** CSS px of document per CSS px of scroll position (1 unless the grid exceeds `MAX_SCROLL_PX`). */
  private scrollMap(): { x: number; y: number } {
    const axes = this.axes
    if (!axes) return { x: 1, y: 1 }
    const fit = (total: number, view: number) => {
      const shown = Math.min(total, MAX_SCROLL_PX)
      return shown > view && total > shown ? (total - view) / (shown - view) : 1
    }
    return { x: fit(axes.width, this.width), y: fit(axes.height, this.height) }
  }

  private resize() {
    if (this.destroyed) return
    this.dpr = window.devicePixelRatio || 1
    this.width = this.scroller.clientWidth
    this.height = this.scroller.clientHeight
    for (const canvas of [this.content, this.overlay]) {
      const w = Math.round(this.width * this.dpr)
      const h = Math.round(this.height * this.dpr)
      if (canvas.width !== w) canvas.width = w
      if (canvas.height !== h) canvas.height = h
      canvas.style.width = `${this.width}px`
      canvas.style.height = `${this.height}px`
    }
    if (this.axes) {
      this.spacer.style.width = `${Math.min(this.axes.width, MAX_SCROLL_PX)}px`
      this.spacer.style.height = `${Math.min(this.axes.height, MAX_SCROLL_PX)}px`
    }
    this.onScroll()
    this.drawOverlay()
  }

  private onScroll() {
    const map = this.scrollMap()
    // The canvas origin sits on a device pixel: scroll positions go to whole device pixels.
    const quantize = (v: number) => Math.round(v * this.dpr) / this.dpr
    const y = quantize(this.scroller.scrollTop * map.y)
    const now = performance.now()
    const dt = now - this.lastScroll.at
    if (dt > 0 && dt < 250) this.velocity = ((y - this.lastScroll.y) / dt) * 1000
    else if (dt >= 250) this.velocity = 0
    this.lastScroll = { y, at: now }
    this.scrollX = quantize(this.scroller.scrollLeft * map.x)
    this.scrollY = y
    this.invalidate()
  }

  /** The screen regions of the layers and the document point at each one's canvas origin. */
  private regions(axes: Axes): Region[] {
    const { width: w, height: h, scrollX: sx, scrollY: sy } = this
    const { rowHeadWidth: head, bodyLeft, headerHeight: top } = axes
    const regions: Region[] = [
      { layer: 'body', clip: { x: bodyLeft, y: top, width: w - bodyLeft, height: h - top }, ox: sx, oy: sy },
      { layer: 'rowHead', clip: { x: 0, y: top, width: head, height: h - top }, ox: 0, oy: sy },
      { layer: 'header', clip: { x: bodyLeft, y: 0, width: w - bodyLeft, height: top }, ox: sx, oy: 0 },
      { layer: 'headerFrozen', clip: { x: 0, y: 0, width: bodyLeft, height: top }, ox: 0, oy: 0 },
    ]
    if (axes.frozenWidth > 0) regions.splice(1, 0, { layer: 'frozen', clip: { x: head, y: top, width: axes.frozenWidth, height: h - top }, ox: 0, oy: sy })
    return regions
  }

  /** The bands of the document rows `top`..`bottom` and the scrolling xs `left`..`right`. */
  private keysFor(axes: Axes, top: number, bottom: number, left: number, right: number): BandKey[] {
    const keys: BandKey[] = [{ layer: Layer.HeaderFrozen, colBand: 0, rowBand: 0 }]
    const firstStrip = Math.max(0, Math.floor((left - axes.bodyLeft) / axes.bandWidth))
    const lastStrip = Math.min(axes.colBands - 1, Math.floor((right - axes.bodyLeft) / axes.bandWidth))
    for (let colBand = firstStrip; colBand <= lastStrip; colBand += 1) keys.push({ layer: Layer.Header, colBand, rowBand: 0 })
    return keys.concat(bandKeys(axes, top, bottom, left, right))
  }

  /** The bands the viewport shows now. */
  private needed(): BandKey[] {
    const axes = this.axes
    if (!axes) return []
    const top = this.scrollY + axes.headerHeight
    const left = this.scrollX + axes.bodyLeft
    return this.keysFor(axes, top, this.scrollY + this.height, left, this.scrollX + this.width)
  }

  /** The bands to fetch next: the visible ones missing, then the prefetch, nearest first. */
  private wanted(): BandKey[] {
    const axes = this.axes
    if (!axes) return []
    const screen = Math.max(1, this.height - axes.headerHeight)
    const ahead = Math.abs(this.velocity) > FAST_SCROLL ? 2 : 1
    const down = this.velocity >= 0
    const top = this.scrollY + axes.headerHeight
    const bottom = this.scrollY + this.height
    const left = this.scrollX + axes.bodyLeft
    const right = this.scrollX + this.width
    const viewWidth = Math.max(1, right - left)
    const order = [
      this.keysFor(axes, top, bottom, left, right),
      down ? this.keysFor(axes, bottom, bottom + screen * ahead, left, right) : this.keysFor(axes, top - screen * ahead, top, left, right),
      down ? this.keysFor(axes, top - screen, top, left, right) : this.keysFor(axes, bottom, bottom + screen, left, right),
      this.keysFor(axes, top, bottom, right, right + viewWidth),
      this.keysFor(axes, top, bottom, left - viewWidth, left),
    ]
    const seen = new Set<string>()
    const out: BandKey[] = []
    for (const keys of order)
      for (const key of keys) {
        const id = keyId(key)
        if (seen.has(id) || this.cache.has(key)) continue
        seen.add(id)
        out.push(key)
      }
    return out
  }

  private request() {
    const axes = this.axes
    if (!axes || this.inFlight || this.destroyed) return
    const keys = this.wanted().slice(0, MAX_BANDS_PER_CALL)
    if (!keys.length) return
    this.inFlight = true
    const dpr = this.dpr
    const epoch = axes.epoch
    const started = performance.now()
    const tiles = this.resources.emojiKeys()
    this.source
      .bands(epoch, keys, dpr, this.resources.iconCount, tiles)
      .then((bytes) => {
        const batch = decodeBatch(bytes)
        this.stats?.requests.push({ bands: keys.length, ms: performance.now() - started, bytes: batch.bytes })
        this.received += batch.bytes
        for (const icon of batch.icons) this.resources.addIcon(icon.id, icon.data)
        // Tiles are keyed by the ratio the page asked for (the batch holds it as an f32).
        if (dpr === this.dpr) for (const tile of batch.emoji) this.resources.addEmoji({ ...tile, dpr })
        const keep = new Set(this.needed().map(keyId))
        for (const list of batch.bands) if (this.axes && list.header.epoch === this.axes.epoch) this.cache.put(list, keep)
      })
      .catch((error: unknown) => console.error('grid: gridBands failed', error))
      .finally(() => {
        this.inFlight = false
        this.invalidate()
        this.request()
      })
  }

  private draw(at: number) {
    this.frame = 0
    const axes = this.axes
    const ctx = this.content.getContext('2d')
    if (!axes || !ctx) return
    const started = performance.now()
    const dpr = this.dpr
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.fillStyle = this.skeleton.background
    ctx.fillRect(0, 0, this.content.width, this.content.height)
    let skeleton = false
    const needed = this.needed()
    for (const region of this.regions(axes)) {
      const clip = region.clip
      if (clip.width <= 0 || clip.height <= 0) continue
      ctx.save()
      ctx.beginPath()
      const x0 = Math.round(clip.x * dpr)
      const y0 = Math.round(clip.y * dpr)
      ctx.rect(x0, y0, Math.round((clip.x + clip.width) * dpr) - x0, Math.round((clip.y + clip.height) * dpr) - y0)
      ctx.clip()
      for (const key of needed) {
        if (regionOf(key.layer) !== region.layer) continue
        const list = this.cache.get(key)
        if (list) play({ ctx, x: region.ox, y: region.oy, dpr }, list, this.resources, this.fonts)
        else {
          skeleton = true
          this.drawSkeleton(ctx, axes, key, region.ox, region.oy)
        }
      }
      ctx.restore()
    }
    const playMs = performance.now() - started
    if (this.stats) {
      this.stats.frames.push({ at, playMs, skeleton, bytes: this.received })
      if (!skeleton && this.stats.firstContent === null) this.stats.firstContent = performance.now()
    }
    this.received = 0
    this.request()
  }

  /** A band not here yet: its background and lines, from the axes. */
  private drawSkeleton(ctx: CanvasRenderingContext2D, axes: Axes, key: BandKey, ox: number, oy: number) {
    const area = axes.bandArea(key)
    const dpr = this.dpr
    const x0 = Math.round((area.x - ox) * dpr)
    const y0 = Math.round((area.y - oy) * dpr)
    const x1 = Math.round((area.x + area.width - ox) * dpr)
    const y1 = Math.round((area.y + area.height - oy) * dpr)
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.fillStyle = this.skeleton.background
    ctx.fillRect(x0, y0, x1 - x0, y1 - y0)
    ctx.fillStyle = this.skeleton.line
    if (key.layer !== Layer.Header && key.layer !== Layer.HeaderFrozen) {
      const first = key.rowBand * axes.bandRows
      const last = Math.min(axes.rows, first + axes.bandRows)
      for (let row = first; row < last; row += 1) {
        const bottom = axes.rowTop(row + 1)
        ctx.fillRect(x0, Math.floor((bottom - 0.5 - oy) * dpr + 1e-4), x1 - x0, 1)
      }
    }
    for (const column of axes.columns) {
      const edge = column.x + column.width
      if (edge > area.x && edge <= area.x + area.width) ctx.fillRect(Math.floor((edge - 0.5 - ox) * dpr + 1e-4), y0, 1, y1 - y0)
    }
  }

  private drawOverlay() {
    const ctx = this.overlay.getContext('2d')
    if (!ctx) return
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height)
    this.overlayPainter?.(ctx, this.dpr)
  }
}

function regionOf(layer: Layer): Region['layer'] {
  switch (layer) {
    case Layer.Body:
      return 'body'
    case Layer.Frozen:
      return 'frozen'
    case Layer.RowHead:
      return 'rowHead'
    case Layer.Header:
      return 'header'
    default:
      return 'headerFrozen'
  }
}
