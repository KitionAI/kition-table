/**
 * The page's time zone as the engine reads dates in it (`gridAxes`' `zone`): the UTC offset in
 * minutes east from each instant on where it changes, between `from` and `to` (years), oldest
 * first. The engine has no time zone database; the browser's, read once here, carries its daylight
 * saving rules, so the grid shows an instant's wall time as the old grid did with `Date`.
 */
const WEEK = 7 * 86_400_000
/** Before every instant a date can name (`new Date` refuses earlier ones). */
const FIRST = -8.64e15

let cached: Array<[number, number]> | null = null

export function timeZoneChanges(from = 1900, to = 2100): Array<[number, number]> {
  if (cached && from === 1900 && to === 2100) return cached
  const offset = (at: number) => -new Date(at).getTimezoneOffset()
  const end = Date.UTC(to, 0, 1)
  let at = Date.UTC(from, 0, 1)
  let current = offset(at)
  const changes: Array<[number, number]> = [[FIRST, current]]
  while (at < end) {
    const next = Math.min(at + WEEK, end)
    if (offset(next) !== current) {
      // The first millisecond with the new offset.
      let low = at
      let high = next
      while (high - low > 1) {
        const middle = Math.floor((low + high) / 2)
        if (offset(middle) === current) low = middle
        else high = middle
      }
      current = offset(high)
      changes.push([high, current])
      at = high
      continue
    }
    at = next
  }
  if (from === 1900 && to === 2100) cached = changes
  return changes
}
