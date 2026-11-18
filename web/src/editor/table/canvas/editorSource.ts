/**
 * The grid's `GridSource` in the editor (grid plan section 3.1, phase 5). The editor arranges the
 * view itself (`useTableArrangement`: `gridOpen` with its settings, search and collapsed groups,
 * then `gridSync` after every change), so the grid opens nothing: it lays out the axis the editor
 * holds (`TableDataSource.grid.layout`: `gridAxes` with the view's config as the editor holds it,
 * then `gridRows`), and asks for its bands and the pictures they show through the same data
 * source. The records never come to the page.
 */
import type { GridLayoutParams, TableDataSource } from '../ports/TableDataSource'
import type { BandKey } from './displayList'
import { blobHash, gridOpened, thumbnail } from './engineSource'
import type { GridOpened, GridSource } from './gridView'

/** What the grid lays out: the editor's latest axis (its bytes; null before the first) and how to draw it. */
export type ArrangedTarget = Omit<GridLayoutParams, 'axis'> & { axis: Uint8Array | null }

/** A `GridSource` whose target the editor changes (a new axis, settings, theme); `GridView.reopen` then lays it out again. */
export type ArrangedSource = GridSource & { readonly target: ArrangedTarget; update(changes: Partial<ArrangedTarget>): void }

export function arrangedSource(dataSource: Pick<TableDataSource, 'grid' | 'attachments'>, documentId: number, initial: ArrangedTarget): ArrangedSource {
  let target = { ...initial }
  return {
    get target() {
      return target
    },
    update(changes) {
      target = { ...target, ...changes }
    },
    async open(): Promise<GridOpened> {
      const { axis, ...params } = target
      if (!axis) throw new Error('the editor has not arranged the view yet')
      const { axes, rows } = await dataSource.grid.layout({ ...params, axis })
      return gridOpened(axes, rows)
    },
    bands(epoch: number, keys: BandKey[], dpr: number, session: number, draft: boolean): Promise<Uint8Array> {
      const flat = new Uint32Array(keys.length * 3)
      keys.forEach((key, index) => flat.set([key.layer, key.colBand, key.rowBand], index * 3))
      return dataSource.grid.bands(epoch, flat, dpr, session, draft)
    },
    async image(url: string, width: number, height: number): Promise<ImageBitmap | null> {
      const hash = blobHash(url)
      let blob: Blob | null = null
      if (hash && dataSource.attachments) {
        const held = await dataSource.attachments.read({ documentId, hash })
        blob = new Blob([held.data as BlobPart], { type: held.mimeType })
      } else if (url.startsWith('data:')) blob = await (await fetch(url)).blob()
      return blob ? thumbnail(blob, width, height) : null
    },
  }
}
