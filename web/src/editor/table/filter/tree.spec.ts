import { describe, expect, it } from 'vitest'
import { countFilterConditions, filterTreeHasActiveCondition, parseFilterTree } from './tree'
import type { FilterCondition, FilterGroup } from './types'

// Whether a record passes a filter is the engine's: those assertions moved to kition-table-arrange
// (crates/engine/arrange/src/filter.rs) with the editor's filter.

function cond(over: Partial<FilterCondition>): FilterCondition {
  return { id: 'c1', kind: 'condition', field_name: 'label', operator: 'is', value: null, ...over }
}
function group(children: FilterGroup['children'], conjunction: 'and' | 'or' = 'and'): FilterGroup {
  return { id: 'g1', kind: 'group', conjunction, children }
}

describe('filterTreeHasActiveCondition / countFilterConditions', () => {
  it('counts only active conditions', () => {
    const tree = group([
      cond({ operator: 'contains', value: 'x' }),       // active
      cond({ operator: 'contains', value: '' }),         // inactive
      cond({ field_name: 'qty', operator: 'isGreater', value: 0 }), // active
    ])
    expect(countFilterConditions(tree)).toBe(2)
    expect(filterTreeHasActiveCondition(tree)).toBe(true)
  })
  it('returns 0/false for null tree', () => {
    expect(countFilterConditions(null)).toBe(0)
    expect(filterTreeHasActiveCondition(null)).toBe(false)
  })
})

describe('parseFilterTree', () => {
  it('accepts a well-formed tree', () => {
    const raw = { kind: 'group', id: 'g', conjunction: 'and', children: [
      { kind: 'condition', id: 'c', field_name: 'label', operator: 'is', value: 'x' },
    ] }
    expect(parseFilterTree(raw)).toEqual(raw)
  })
  it('returns null when top node is not a group', () => {
    expect(parseFilterTree({ kind: 'condition', id: 'c', field_name: 'label', operator: 'is', value: 'x' })).toBeNull()
  })
  it('returns null when operator is unknown (e.g. legacy "equals")', () => {
    const raw = { kind: 'group', id: 'g', conjunction: 'and', children: [
      { kind: 'condition', id: 'c', field_name: 'label', operator: 'equals', value: 'x' },
    ] }
    expect(parseFilterTree(raw)).toBeNull()
  })
  it('returns null when raw is null/undefined/not-object', () => {
    expect(parseFilterTree(null)).toBeNull()
    expect(parseFilterTree(undefined)).toBeNull()
    expect(parseFilterTree('garbage')).toBeNull()
  })
  it('accepts nested groups', () => {
    const raw = {
      kind: 'group', id: 'g0', conjunction: 'and', children: [
        { kind: 'group', id: 'g1', conjunction: 'or', children: [
          { kind: 'condition', id: 'c', field_name: 'label', operator: 'is', value: 'x' },
        ] },
      ],
    }
    expect(parseFilterTree(raw)).toEqual(raw)
  })
})
