import type { KitEngineInfo, KitModuleManifest } from '@kition/kit-editor'

/** The engine, from this package's own folder. */
export function engineManifest(): KitModuleManifest {
  return {
    kitId: 'kit.table',
    base: new URL('./engine/', document.baseURI).href,
    module: 'kition_table_kit',
    documentClass: 'TableDocument',
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
