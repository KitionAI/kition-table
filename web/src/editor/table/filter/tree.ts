import type { FilterConjunction, FilterGroup, FilterNode, FilterOperator, FilterValue } from './types'

/**
 * The filter tree as the editor's filter menu holds it: reading it from a view's config, and which
 * of its conditions are written. Whether a record passes it is the engine's (kition-table-arrange),
 * which arranges the view's records.
 */

const KNOWN_OPERATORS = new Set<FilterOperator>([
  'is', 'isNot', 'isEmpty', 'isNotEmpty',
  'contains', 'doesNotContain',
  'isGreater', 'isGreaterEqual', 'isLess', 'isLessEqual',
  'isAnyOf', 'isNoneOf',
  'hasAnyOf', 'hasAllOf', 'isExactly', 'isNotExactly', 'hasNoneOf',
  'isWithIn', 'isBefore', 'isAfter', 'isOnOrBefore', 'isOnOrAfter',
])

const EMPTY_OPERATORS: FilterOperator[] = ['isEmpty', 'isNotEmpty']

/** Whether a filter tree holds a written condition (one with a field, and a value unless it tests for emptiness). */
export function filterTreeHasActiveCondition(node: FilterNode | null): boolean {
  if (!node) return false
  if (node.kind === 'condition') {
    if (EMPTY_OPERATORS.includes(node.operator)) return Boolean(node.field_name)
    const v = node.value
    if (v === null || v === undefined) return false
    if (typeof v === 'string' && v.trim() === '') return false
    if (Array.isArray(v) && v.length === 0) return false
    return Boolean(node.field_name)
  }
  return node.children.some((c) => filterTreeHasActiveCondition(c))
}

/** How many written conditions a filter tree holds. */
export function countFilterConditions(node: FilterNode | null): number {
  if (!node) return 0
  if (node.kind === 'condition') return filterTreeHasActiveCondition(node) ? 1 : 0
  return node.children.reduce((n, c) => n + countFilterConditions(c), 0)
}

function isConjunction(v: unknown): v is FilterConjunction {
  return v === 'and' || v === 'or'
}

function parseNode(raw: unknown): FilterNode | null {
  if (!raw || typeof raw !== 'object') return null
  const obj = raw as Record<string, unknown>
  if (obj.kind === 'condition') {
    if (typeof obj.id !== 'string') return null
    if (typeof obj.field_name !== 'string') return null
    if (typeof obj.operator !== 'string' || !KNOWN_OPERATORS.has(obj.operator as FilterOperator)) return null
    const value = obj.value
    const isValueOk =
      value === null ||
      typeof value === 'string' ||
      typeof value === 'number' ||
      typeof value === 'boolean' ||
      (Array.isArray(value) && value.every((x) => typeof x === 'string'))
    if (!isValueOk) return null
    return {
      id: obj.id,
      kind: 'condition',
      field_name: obj.field_name,
      operator: obj.operator as FilterOperator,
      value: value as FilterValue,
    }
  }
  if (obj.kind === 'group') {
    if (typeof obj.id !== 'string') return null
    if (!isConjunction(obj.conjunction)) return null
    if (!Array.isArray(obj.children)) return null
    const children: FilterNode[] = []
    for (const c of obj.children) {
      const parsed = parseNode(c)
      if (!parsed) return null
      children.push(parsed)
    }
    return { id: obj.id, kind: 'group', conjunction: obj.conjunction, children }
  }
  return null
}

/** A view config's filter tree, or null when it is no well-formed tree of known operators. */
export function parseFilterTree(raw: unknown): FilterGroup | null {
  const node = parseNode(raw)
  if (!node || node.kind !== 'group') return null
  return node
}
