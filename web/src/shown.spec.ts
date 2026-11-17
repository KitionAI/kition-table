import { describe, expect, it } from 'vitest'
import { resolveShown, shownOfView, viewOfShown } from './shown'

describe('the table or dashboard a view names', () => {
  it('reads tables/<id> and dashboards/<id>, and nothing else', () => {
    expect(shownOfView({ path: 'tables/3' })).toEqual({ kind: 'table', id: 3 })
    expect(shownOfView({ path: 'dashboards/overview' })).toEqual({ kind: 'dashboard', id: 'overview' })
    expect(shownOfView({ path: 'tables/x' })).toBeNull()
    expect(shownOfView({ path: 'records/3' })).toBeNull()
    expect(shownOfView({ sheet: 'tables/3' })).toBeNull()
    expect(shownOfView(null)).toBeNull()
    expect(viewOfShown({ kind: 'dashboard', id: 'overview' })).toEqual({ path: 'dashboards/overview' })
    expect(shownOfView(viewOfShown({ kind: 'table', id: 7 }))).toEqual({ kind: 'table', id: 7 })
  })

  it('shows the chosen one while the document has it, else its first table, else its first dashboard', () => {
    const tables = [{ id: 1 }, { id: 2 }]
    const dashboards = [{ id: 'd' }]
    expect(resolveShown({ kind: 'table', id: 2 }, tables, dashboards)).toEqual({ kind: 'table', id: 2 })
    expect(resolveShown({ kind: 'dashboard', id: 'd' }, tables, dashboards)).toEqual({ kind: 'dashboard', id: 'd' })
    expect(resolveShown({ kind: 'table', id: 9 }, tables, dashboards)).toEqual({ kind: 'table', id: 1 })
    expect(resolveShown({ kind: 'dashboard', id: 'gone' }, [], dashboards)).toEqual({ kind: 'dashboard', id: 'd' })
    expect(resolveShown(null, [], [])).toBeNull()
  })
})
