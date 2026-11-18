import { describe, expect, it } from 'vitest'

import type { DataField } from '../../types/dataDocument'

import {
  displayValue,
  getSortDirectionLabels,
  normalizeLegacyViewTitle,
} from './tableEditorShared'

// Sorting, the search, group ids and group points are the engine's: those assertions moved to
// kition-table-arrange (crates/engine/arrange) with the editor's arrangement.

describe('normalizeLegacyViewTitle', () => {
  it('maps the included task table legacy grid title to Grid view', () => {
    expect(normalizeLegacyViewTitle({ title: 'All tasks', type: 'grid' })).toBe('Grid view')
    expect(normalizeLegacyViewTitle({ title: 'All messages', type: 'grid' })).toBe('Grid view')
  })

  it('preserves user-defined view titles', () => {
    expect(normalizeLegacyViewTitle({ title: 'My tasks', type: 'grid' })).toBe('My tasks')
    expect(normalizeLegacyViewTitle({ title: 'All tasks', type: 'kanban' })).toBe('All tasks')
  })
})

describe('displayValue — phase 2 ref shapes', () => {
  it('extracts LinkRef.display so filter/sort/search/group see link cells as text', () => {
    expect(displayValue({ row_key: 'r_1', display: 'Project Alpha' })).toBe('Project Alpha')
    expect(displayValue([{ row_key: 'r_1', display: 'Alpha' }, { row_key: 'r_2', display: 'Beta' }])).toBe(
      'Alpha, Beta',
    )
  })

  it('falls back to row_key when LinkRef has no display label', () => {
    expect(displayValue({ row_key: 'r_999' })).toBe('r_999')
  })

  it('extracts UserRef.name', () => {
    expect(displayValue([{ id: 'u1', name: 'Alice' }, { id: 'u2', name: 'Bob' }])).toBe('Alice, Bob')
  })

  it('returns empty string for an empty link/user array', () => {
    expect(displayValue([])).toBe('')
  })
})

describe('getSortDirectionLabels', () => {
  it('returns A → Z / Z → A for text-group fields', () => {
    expect(getSortDirectionLabels('text')).toEqual({ asc: 'A → Z', desc: 'Z → A' })
    expect(getSortDirectionLabels('long_text')).toEqual({ asc: 'A → Z', desc: 'Z → A' })
    expect(getSortDirectionLabels('url')).toEqual({ asc: 'A → Z', desc: 'Z → A' })
    expect(getSortDirectionLabels('single_select')).toEqual({ asc: 'A → Z', desc: 'Z → A' })
  })

  it('returns 1 → 9 / 9 → 1 for number-group fields', () => {
    expect(getSortDirectionLabels('number')).toEqual({ asc: '1 → 9', desc: '9 → 1' })
    expect(getSortDirectionLabels('rating')).toEqual({ asc: '1 → 9', desc: '9 → 1' })
    expect(getSortDirectionLabels('auto_number')).toEqual({ asc: '1 → 9', desc: '9 → 1' })
  })

  it('returns Earliest → Latest for date-group fields', () => {
    expect(getSortDirectionLabels('date')).toEqual({
      asc: 'Earliest → Latest',
      desc: 'Latest → Earliest',
    })
    expect(getSortDirectionLabels('datetime')).toEqual({
      asc: 'Earliest → Latest',
      desc: 'Latest → Earliest',
    })
    expect(getSortDirectionLabels('created_time')).toEqual({
      asc: 'Earliest → Latest',
      desc: 'Latest → Earliest',
    })
    expect(getSortDirectionLabels('last_modified_time')).toEqual({
      asc: 'Earliest → Latest',
      desc: 'Latest → Earliest',
    })
  })

  it('returns Unchecked → Checked for checkbox', () => {
    expect(getSortDirectionLabels('checkbox')).toEqual({
      asc: 'Unchecked → Checked',
      desc: 'Checked → Unchecked',
    })
  })

  it('falls back to Ascending / Descending for non-orderable / multi-value fields', () => {
    expect(getSortDirectionLabels('multi_select')).toEqual({
      asc: 'Ascending',
      desc: 'Descending',
    })
    expect(getSortDirectionLabels('attachment')).toEqual({
      asc: 'Ascending',
      desc: 'Descending',
    })
    expect(getSortDirectionLabels('user')).toEqual({
      asc: 'Ascending',
      desc: 'Descending',
    })
    expect(getSortDirectionLabels('formula')).toEqual({
      asc: 'Ascending',
      desc: 'Descending',
    })
  })
})
