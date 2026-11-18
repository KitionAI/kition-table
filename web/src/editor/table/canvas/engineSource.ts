/**
 * The grid's `GridSource` over the kit's engine (`KitEngine.invoke`, a Worker): the arrangement
 * (`gridOpen`, `kition-table-arrange`), handed back to the engine to lay out its rows and columns
 * (`gridAxes`, with the overlay's style and the group headers' ids), what each linear row is (`gridRows`), and the bands
 * (`gridBands`). Typed arrays come back transferred, without a copy.
 */
import type { KitEngine } from '@kition/kit-editor'
import type { AxesInput } from './axes'
import type { BandKey } from './displayList'
import type { GridGroup, GridOpened, GridSource, Skeleton } from './gridView'
import type { OverlayStyle } from './overlay'
import { GridRows } from './rows'

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

/** A `GridSource` whose target can change (the search, the collapsed groups); `GridView.reopen` then arranges again. */
export type EngineSource = GridSource & { readonly target: GridTarget; update(changes: Partial<GridTarget>): void }

export function engineSource(engine: KitEngine, initial: GridTarget): EngineSource {
  let target = { ...initial }
  return {
    get target() {
      return target
    },
    update(changes: Partial<GridTarget>) {
      target = { ...target, ...changes }
    },
    async open(): Promise<GridOpened> {
      const view = { tableId: target.tableId, viewId: target.viewId ?? null }
      const axis = await engine.invoke<Uint8Array>('gridOpen', JSON.stringify({ ...view, query: target.query ?? '', collapsed: target.collapsed ?? [] }))
      const answer = JSON.parse(await engine.invoke<string>('gridAxes', axis, JSON.stringify({ ...view, theme: target.theme ?? 'light', brand: target.brand ?? null }))) as AxesAnswer
      const rows = GridRows.decode(await engine.invoke<Uint8Array>('gridRows'))
      const { skeleton, overlay, groups, ...axes } = answer
      return { axes, skeleton, overlay, rows, groups }
    },
    bands(epoch: number, keys: BandKey[], dpr: number, icons: number, tiles: string[]): Promise<Uint8Array> {
      const flat = new Uint32Array(keys.length * 3)
      keys.forEach((key, index) => flat.set([key.layer, key.colBand, key.rowBand], index * 3))
      return engine.invoke<Uint8Array>('gridBands', epoch, flat, dpr, icons, JSON.stringify(tiles))
    },
  }
}
