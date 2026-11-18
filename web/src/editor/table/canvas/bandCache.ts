/**
 * The page's cache of display lists by band: least recently used first out, held under a count
 * and a byte budget (the encoded size), so a long scroll keeps the bands around the viewport and
 * drops the rest. Lists of another epoch (arrangement) are dropped when the epoch changes.
 */
import { keyId } from './axes'
import type { BandKey, DisplayList } from './displayList'

export const CACHE_BANDS = 48
export const CACHE_BYTES = 2 * 1024 * 1024

export class BandCache {
  private readonly lists = new Map<string, DisplayList>()
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

  has(key: BandKey): boolean {
    return this.lists.has(keyId(key))
  }

  /** The list of `key`, marked as just used. */
  get(key: BandKey): DisplayList | undefined {
    const id = keyId(key)
    const list = this.lists.get(id)
    if (list) {
      this.lists.delete(id)
      this.lists.set(id, list)
    }
    return list
  }

  /** Looks without marking the list used (for prefetch planning). */
  peek(key: BandKey): DisplayList | undefined {
    return this.lists.get(keyId(key))
  }

  put(list: DisplayList, keep: ReadonlySet<string> = new Set()) {
    const id = keyId(list.header.key)
    const old = this.lists.get(id)
    if (old) {
      this.size -= old.bytes
      this.lists.delete(id)
    }
    this.lists.set(id, list)
    this.size += list.bytes
    // Oldest first, but never a band the current frame needs.
    for (const [other, held] of this.lists) {
      if (this.lists.size <= this.maxBands && this.size <= this.maxBytes) break
      if (other === id || keep.has(other)) continue
      this.lists.delete(other)
      this.size -= held.bytes
    }
  }

  delete(key: BandKey) {
    const id = keyId(key)
    const old = this.lists.get(id)
    if (old) {
      this.size -= old.bytes
      this.lists.delete(id)
    }
  }

  /** Drops the lists not laid out for `epoch`. */
  keepEpoch(epoch: number) {
    for (const [id, list] of this.lists) {
      if (list.header.epoch !== epoch) {
        this.lists.delete(id)
        this.size -= list.bytes
      }
    }
  }

  clear() {
    this.lists.clear()
    this.size = 0
  }
}
