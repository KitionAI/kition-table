/**
 * The grid's `GridSource` over the kit's engine (`KitEngine.invoke`, a Worker): the arrangement
 * (`gridOpen`, `kition-table-arrange`), handed back to the engine to lay out its rows and columns
 * (`gridAxes`, with the overlay's style and the group headers' ids), what each linear row is (`gridRows`), the bands
 * (`gridBands`), and the pictures the bands show (`attachments.read` for a blob the document holds, or a `data:` url;
 * the kit loads nothing from the network), decoded by the browser, or by the engine (`gridPicture`) when the browser
 * cannot. Typed arrays come back transferred, without a copy.
 *
 * After a write, `sync` brings the arrangement up to date (`gridSync`: only the records the write
 * touched move) and lays it out again only when its axis changed.
 */
import type { KitEngine } from '@kition/kit-editor'
import type { AxesInput } from './axes'
import type { BandKey } from './displayList'
import type { GridGroup, GridOpened, GridSource, Skeleton } from './gridView'
import type { OverlayStyle } from './overlay'
import { GridRows } from './rows'

/** An attachment url naming a blob the document holds. */
const BLOB_URL = /^kition-blob:([0-9a-f]{64})$/
/** The longest side of a picture's thumbnail, as the engine decodes it (`THUMBNAIL` in bindings/wasm/src/bands.rs). */
const THUMBNAIL = 384

/** What the editor arranges and how it draws it. */
export type GridTarget = {
  tableId: number
  viewId?: number | null
  /** The editor's search. */
  query?: string
  /** The collapsed group ids. */
  collapsed?: string[]
  theme?: 'light' | 'dark'
  /** The kit's brand color, as `#rrggbb`. */
  brand?: string | null
}

type AxesAnswer = AxesInput & { skeleton: Skeleton; overlay: OverlayStyle; groups: GridGroup[] }

/** What a layout is to the view: `gridAxes`' answer (parsed) and `gridRows`' bytes. */
export function gridOpened(answer: unknown, rows: Uint8Array): GridOpened {
  const { skeleton, overlay, groups, ...axes } = answer as AxesAnswer
  return { axes, skeleton, overlay, rows: GridRows.decode(rows), groups }
}

/** The blob a picture's url names among the document's (`kition-blob:<sha256>`), or null. */
export function blobHash(url: string): string | null {
  return BLOB_URL.exec(url.trim())?.[1] ?? null
}

/** The engine's decoder (`gridPicture`): a picture's bytes to width and height (u32 each) and straight RGBA8, or no bytes. */
export type PictureDecoder = (bytes: Uint8Array, maxSide: number) => Promise<Uint8Array>

/** The size that fits `width` x `height` in `maxSide` on its longest side, as `createImageBitmap`'s resize options (none when it fits). */
function fitted(width: number, height: number, maxSide: number): ImageBitmapOptions {
  const scale = Math.min(1, maxSide / Math.max(width, height, 1))
  return width > 0 && height > 0 && scale < 1 ? { resizeWidth: Math.max(1, Math.round(width * scale)), resizeHeight: Math.max(1, Math.round(height * scale)), resizeQuality: 'high' } : {}
}

/**
 * A picture decoded at most at `maxSide` on its longest side (the engine's thumbnail size by default), as the headless
 * picture decodes it: by the browser (a GIF's first frame, the picture turned by its EXIF orientation), else by the
 * engine (`decode`), which reads what browsers do not (an SVG, which `createImageBitmap` refuses everywhere; a HEIC
 * outside Safari) as the picture does. `width` and `height` are its size when known (0 when not). Null when neither
 * reads it.
 */
export async function thumbnail(blob: Blob, width: number, height: number, decode: PictureDecoder, maxSide = THUMBNAIL): Promise<ImageBitmap | null> {
  let bitmap: ImageBitmap | null = null
  try {
    bitmap = await createImageBitmap(blob, fitted(width, height, maxSide))
  } catch {
    // Not a format this browser decodes: the engine's turn.
    return decodedPicture(await decode(new Uint8Array(await blob.arrayBuffer()), maxSide))
  }
  // A picture whose size was not known comes whole: scaled down once it is.
  const resize = fitted(bitmap.width, bitmap.height, maxSide)
  if (!resize.resizeWidth) return bitmap
  const smaller = await createImageBitmap(bitmap, resize)
  bitmap.close()
  return smaller
}

/** What `gridPicture` answered as a bitmap, or null for no picture. */
export function decodedPicture(answer: Uint8Array): Promise<ImageBitmap> | null {
  if (answer.byteLength <= 8) return null
  const header = new DataView(answer.buffer, answer.byteOffset, 8)
  const width = header.getUint32(0, true)
  const height = header.getUint32(4, true)
  const pixels = new Uint8ClampedArray(answer.buffer as ArrayBuffer, answer.byteOffset + 8, width * height * 4)
  return createImageBitmap(new ImageData(pixels, width, height))
}

/**
 * A `GridSource` whose target can change (the search, the collapsed groups); `GridView.reopen` then
 * arranges again. `sync` answers the new layout after a write when the arrangement's axis changed,
 * or null when it did not (the bands of the records written are then the only ones to fetch again).
 */
export type EngineSource = GridSource & { readonly target: GridTarget; update(changes: Partial<GridTarget>): void; sync(): Promise<GridOpened | null> }

export function engineSource(engine: KitEngine, initial: GridTarget): EngineSource {
  let target = { ...initial }
  const layout = async (axis: Uint8Array): Promise<GridOpened> => {
    const view = { tableId: target.tableId, viewId: target.viewId ?? null }
    const answer = JSON.parse(await engine.invoke<string>('gridAxes', axis, JSON.stringify({ ...view, theme: target.theme ?? 'light', brand: target.brand ?? null }))) as unknown
    return gridOpened(answer, await engine.invoke<Uint8Array>('gridRows'))
  }
  return {
    get target() {
      return target
    },
    update(changes: Partial<GridTarget>) {
      target = { ...target, ...changes }
    },
    async open(): Promise<GridOpened> {
      const view = { tableId: target.tableId, viewId: target.viewId ?? null }
      return layout(await engine.invoke<Uint8Array>('gridOpen', JSON.stringify({ ...view, query: target.query ?? '', collapsed: target.collapsed ?? [] })))
    },
    async sync(): Promise<GridOpened | null> {
      const axis = await engine.invoke<Uint8Array>('gridSync')
      return axis.byteLength ? layout(axis) : null
    },
    async image(url: string, width: number, height: number): Promise<ImageBitmap | null> {
      const hash = blobHash(url)
      let blob: Blob | null = null
      if (hash) {
        const answer = JSON.parse(await engine.invoke<string>('call', 'attachments.read', JSON.stringify({ hash }))) as { result?: { data: string; mimeType: string } }
        if (answer.result) blob = new Blob([Uint8Array.from(atob(answer.result.data), (c) => c.charCodeAt(0))], { type: answer.result.mimeType })
      } else if (url.startsWith('data:')) blob = await (await fetch(url)).blob()
      return blob ? thumbnail(blob, width, height, (bytes, maxSide) => engine.invoke<Uint8Array>('gridPicture', bytes, maxSide)) : null
    },
    bands(epoch: number, keys: BandKey[], dpr: number, session: number, draft: boolean): Promise<Uint8Array> {
      const flat = new Uint32Array(keys.length * 3)
      keys.forEach((key, index) => flat.set([key.layer, key.colBand, key.rowBand], index * 3))
      return engine.invoke<Uint8Array>('gridBands', epoch, flat, dpr, session, draft)
    },
  }
}
