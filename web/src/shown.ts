import type { KitView } from '@kition/sdk/editor'

/**
 * What the editor shows: one of the document's tables or dashboards. A host names it with a view
 * (Kit UI protocol `document.view` and `document/reveal`): `{ "path": "tables/<id>" }` or
 * `{ "path": "dashboards/<id>" }`, the ids `inspect()` lists.
 */
export type Shown = { kind: 'table'; id: number } | { kind: 'dashboard'; id: string }

/** The table or dashboard a view names; null for a view of anything else. */
export function shownOfView(view: KitView | null | undefined): Shown | null {
  const match = /^(tables|dashboards)\/(.+)$/.exec(view?.path ?? '')
  if (!match) return null
  if (match[1] === 'dashboards') return { kind: 'dashboard', id: match[2] }
  const id = Number(match[2])
  return Number.isInteger(id) && id > 0 ? { kind: 'table', id } : null
}

/** The view that names `shown`. */
export function viewOfShown(shown: Shown): KitView {
  return { path: `${shown.kind === 'table' ? 'tables' : 'dashboards'}/${shown.id}` }
}

/**
 * What to show: the one chosen while the document has it, else its first table, else its first
 * dashboard.
 */
export function resolveShown(
  chosen: Shown | null,
  tables: Array<{ id: number }>,
  dashboards: Array<{ id: string }>,
): Shown | null {
  if (chosen?.kind === 'table' && tables.some((table) => table.id === chosen.id)) return chosen
  if (chosen?.kind === 'dashboard' && dashboards.some((dashboard) => dashboard.id === chosen.id)) return chosen
  if (tables[0]) return { kind: 'table', id: tables[0].id }
  return dashboards[0] ? { kind: 'dashboard', id: dashboards[0].id } : null
}
