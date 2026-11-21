import type { KitEngineInfo, KitModuleManifest } from '@kition/sdk/editor'
import { engineFonts } from './editor/table/canvas/gridFonts'

/**
 * The engine, from this package's own folder, with the faces the grid's bands are shaped with
 * (the package's `fonts/`, the files the page draws them with: platform rule E9, `addFont`):
 * Inter first, the others once the engine's text wants them (`lazyFonts`).
 */
export function engineManifest(): KitModuleManifest {
  return {
    kitId: 'kit.table',
    base: new URL('./engine/', document.baseURI).href,
    module: 'kition_table_kit',
    documentClass: 'TableDocument',
    ...engineFonts(new URL('./fonts/', document.baseURI).href),
  }
}

/** What the shell reads from the engine's `inspect()`; the table editor reads the rest itself. */
export type TableInfo = {
  tables: Array<{ id: number; title: string }>
  dashboards: Array<{ id: string; title: string }>
  readOnly: string | null
}

export function describe(info: KitEngineInfo): TableInfo {
  const inspected = info.inspected as Partial<TableInfo>
  return { tables: inspected.tables ?? [], dashboards: inspected.dashboards ?? [], readOnly: inspected.readOnly ?? null }
}
