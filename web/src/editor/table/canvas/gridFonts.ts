/**
 * The grid's faces on the page: the same files the engine shapes with (`fonts.lock.json`, copied
 * into the package's `fonts/` by `scripts/fonts.mjs`), loaded as `FontFace`s under names of their
 * own, so a text op's `ctx.font` names exactly one face and the browser never picks another.
 *
 * Every face declares the whole weight range `100 900`, also the ones without a weight axis
 * (Droid Sans Fallback, Nanum Gothic): without it WebKit widens synthetic bold and Chromium
 * thickens it, and the page's widths stop matching the engine's. The first frame waits only for
 * Inter; the CJK and Hangul faces load when a band first uses them (the package holds them, so
 * this is a lazy load, not a download from elsewhere). Emoji are tiles the engine draws, so the
 * page never loads the emoji face.
 */
import type { KitModuleManifest } from '@kition/sdk/editor'
import { Face } from './displayList'

/** The files under the package's `fonts/` folder: the page's faces and, for the engine, all four. */
export const FONT_FILES = {
  sans: 'inter/Inter-Variable.ttf',
  fallback: 'droid-sans-fallback/DroidSansFallbackFull.ttf',
  hangul: 'nanum-gothic/NanumGothic-Regular.ttf',
  emoji: 'noto-color-emoji/Noto-COLRv1.ttf',
} as const

/** The CSS family of each face. `Mono` has no file in the package: its runs are not drawn. */
export const FAMILIES: Record<Face, string> = {
  [Face.Sans]: 'KitionTableSans',
  [Face.Fallback]: 'KitionTableFallback',
  [Face.Hangul]: 'KitionTableHangul',
  [Face.Mono]: 'KitionTableMono',
}

const FILE_OF: Partial<Record<Face, string>> = {
  [Face.Sans]: FONT_FILES.sans,
  [Face.Fallback]: FONT_FILES.fallback,
  [Face.Hangul]: FONT_FILES.hangul,
}

/**
 * The font files the engine registers with `addFont`, as URLs: Inter before the table opens
 * (`KitModuleManifest.fonts`), the CJK, Hangul and emoji faces only once the engine's text wants
 * them (`lazyFonts`, about 11 MB), so the first frame never waits for them (grid plan D3).
 */
export function engineFonts(fontsBase: string): Pick<KitModuleManifest, 'fonts' | 'lazyFonts'> {
  const url = (file: string) => new URL(file, fontsBase).href
  return { fonts: [url(FONT_FILES.sans)], lazyFonts: [FONT_FILES.fallback, FONT_FILES.hangul, FONT_FILES.emoji].map(url) }
}

/**
 * The engine's lazy fonts as the grid follows them (`KitEngine.fontsPending`, `onFonts`,
 * `loadFonts`): until the engine has every face it lays out the characters of the faces it lacks
 * as tofu, which the page does not draw; each face it gets makes the bands stale.
 */
export type GridEngineFonts = {
  /** Whether the engine has not got every face yet. */
  pending(): boolean
  /** Calls `listener` each time the engine got a face; answers how to stop. */
  subscribe(listener: () => void): () => void
  /** Gives the engine every face now. */
  loadAll(): Promise<void>
}

export class GridFonts {
  /** The engine's lazy fonts, when the grid's source follows them: tofu waits until they are all in. */
  engine: GridEngineFonts | null = null
  private readonly loaded = new Set<Face>()
  private readonly loading = new Map<Face, Promise<void>>()
  private readonly failed = new Set<Face>()

  /** `fontsBase` is the package's `fonts/` folder as a URL; `onLoad` runs when a lazy face arrives. */
  constructor(
    private readonly fontsBase: string,
    private readonly onLoad: (face: Face) => void,
  ) {}

  /** Loads Inter, which the first frame waits for. */
  async start(): Promise<void> {
    await this.load(Face.Sans)
  }

  /** Whether runs in `face` can be drawn now; asks for the face when it is not loaded yet. */
  ready(face: Face): boolean {
    if (this.loaded.has(face)) return true
    if (!this.loading.has(face) && !this.failed.has(face)) void this.load(face).then(() => this.onLoad(face), () => undefined)
    return false
  }

  /** Whether tofu boxes can be drawn now: the engine has every face (a box may be a character a lazy face has). */
  tofuReady(): boolean {
    return !this.engine?.pending()
  }

  /** Every face the package holds, loaded on the page and in the engine (the tests compare pixels only once all are in). */
  async all(): Promise<void> {
    await Promise.all([...[Face.Sans, Face.Fallback, Face.Hangul].map((face) => this.load(face)), this.engine?.loadAll()])
  }

  private load(face: Face): Promise<void> {
    const file = FILE_OF[face]
    if (!file) {
      this.failed.add(face)
      return Promise.reject(new Error(`no file for face ${face}`))
    }
    let pending = this.loading.get(face)
    if (!pending) {
      pending = (async () => {
        const bytes = await (await fetch(new URL(file, this.fontsBase))).arrayBuffer()
        const font = new FontFace(FAMILIES[face], bytes, { weight: '100 900' })
        await font.load()
        document.fonts.add(font)
        this.loaded.add(face)
      })()
      pending.catch(() => this.failed.add(face))
      this.loading.set(face, pending)
    }
    return pending
  }
}
