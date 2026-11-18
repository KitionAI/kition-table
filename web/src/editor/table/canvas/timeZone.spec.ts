// The page's time zone as the engine reads dates in it (case table.editor.grid/editor).
import { describe, expect, it } from 'vitest'
import { timeZoneChanges } from './timeZone'

describe('the time zone the grid reads dates in', () => {
  it('lists each change of the offset, oldest first, each as the browser reads it there', () => {
    const changes = timeZoneChanges(2000, 2030)
    expect(changes[0][0]).toBe(-8.64e15)
    for (let index = 1; index < changes.length; index += 1) {
      const [at, minutes] = changes[index]
      expect(at).toBeGreaterThan(changes[index - 1][0])
      expect(minutes).not.toBe(changes[index - 1][1])
      expect(-new Date(at).getTimezoneOffset()).toBe(minutes)
      expect(-new Date(at - 1).getTimezoneOffset()).toBe(changes[index - 1][1])
    }
    // Every instant in the range has the offset of the last change before it.
    for (const at of [Date.UTC(2001, 0, 15), Date.UTC(2015, 6, 1, 12), Date.UTC(2029, 10, 30)]) {
      const last = changes.filter(([from]) => from <= at).at(-1)
      expect(last?.[1]).toBe(-new Date(at).getTimezoneOffset())
    }
  })
})
