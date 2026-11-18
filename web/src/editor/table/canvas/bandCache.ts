/**
 * The page's cache of display lists by band: least recently used first out, held under a count
 * and a byte budget (the encoded size), so a long scroll keeps the bands around the viewport and
 * drops the rest. Lists of another epoch (arrangement) are dropped when the epoch changes.
 *
 * A list can go stale (`markStale`: an edit changed what its band shows, the arrangement did not
 * change): it is still drawn (`get`, `drawn`) until its new list is put, but it no longer counts
 * as here (`has`, `peek`), so the view asks for it again.
 */
import { keyId } from './axes'
import type { BandKey, DisplayList } from './displayList'

export const CACHE_BANDS = 48
export const CACHE_BYTES = 2 * 1024 * 1024

export class BandCache {
  private readonly lists = new Map<string, DisplayList>()
  private readonly stale = new Set<string>()
  private size = 0

  constructor(
    private readonly maxBands = CACHE_BANDS,
    private readonly maxBytes = CACHE_BYTES,
  ) {}

  get bytes(): number {
    return this.size
  }

  get count(): number {
    return this.lists.size
  }

  /** Whether the current list of `key` is here (not missing, not stale). */
  has(key: BandKey): boolean {
    const id = keyId(key)
    return this.lists.has(id) && !this.stale.has(id)
  }

  /** The list of `key` to draw (a stale one until its new list is put), marked as just used. */
  get(key: BandKey): DisplayList | undefined {
    const id = keyId(key)
    const list = this.lists.get(id)
    if (list) {
      this.lists.delete(id)
      this.lists.set(id, list)
    }
    return list
  }

  /** The current list of `key` (none while it is stale), without marking it used (for prefetch planning). */
  peek(key: BandKey): DisplayList | undefined {
    const id = keyId(key)
    return this.stale.has(id) ? undefined : this.lists.get(id)
  }

  /** The list of `key` that is drawn (a stale one too), without marking it used (for hit testing). */
  drawn(key: BandKey): DisplayList | undefined {
    return this.lists.get(keyId(key))
  }

  /** Marks the held lists whose keys `which` takes as stale; answers how many. */
  markStale(which: (key: BandKey) => boolean): number {
    let count = 0
    for (const [id, list] of this.lists) {
      if (this.stale.has(id) || !which(list.header.key)) continue
      this.stale.add(id)
      count += 1
    }
    return count
  }

  put(list: DisplayList, keep: ReadonlySet<string> = new Set()) {
    const id = keyId(list.header.key)
    const old = this.lists.get(id)
    if (old) {
      this.size -= old.bytes
      this.lists.delete(id)
    }
    this.lists.set(id, list)
    this.stale.delete(id)
    this.size += list.bytes
    // Oldest first, but never a band the current frame needs.
    for (const [other, held] of this.lists) {
      if (this.lists.size <= this.maxBands && this.size <= this.maxBytes) break
      if (other === id || keep.has(other)) continue
      this.lists.delete(other)
      this.stale.delete(other)
      this.size -= held.bytes
    }
  }

  delete(key: BandKey) {
    const id = keyId(key)
    const old = this.lists.get(id)
    if (old) {
      this.size -= old.bytes
      this.lists.delete(id)
      this.stale.delete(id)
    }
  }

  /** Drops the lists not laid out for `epoch`. */
  keepEpoch(epoch: number) {
    for (const [id, list] of this.lists) {
      if (list.header.epoch !== epoch) {
        this.lists.delete(id)
        this.stale.delete(id)
        this.size -= list.bytes
      }
    }
  }

  clear() {
    this.lists.clear()
    this.stale.clear()
    this.size = 0
  }
}
