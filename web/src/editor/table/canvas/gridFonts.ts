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

/**
 * How the page's Canvas sets Inter's optical size (`opsz`), which the engine must measure with
 * (`gridOpticalSizing`; `OpticalSizing` in `kition-table-fonts`): `auto`, the font size clamped to
 * the axis (Chromium's Canvas, and CSS text everywhere), or `fixed-default`, the axis's default
 * (14) at every size (Playwright's WebKit Canvas on Linux, WebKitGTK by the same engine). The
 * grid's own text is 10 to 13 px, under the axis's floor, where both are the same; larger text
 * differs by up to 9 %.
 */
export type OpticalSizing = 'auto' | 'fixed-default'

/** The probe `canvasOpticalSizing` measures (the Rust test `the_pages_probe_tells_the_optical_sizings_apart` reads these three). */
export const OPSZ_PROBE = 'Quarterly planning review'
/** The probe's width per em in Inter at 400 and 14 px, as the engine measures it (the same in both settings). */
export const OPSZ_PROBE_EM = 12.1284
/**
 * The probe's width per em at 32 px over its width per em at 14 px: 0.910 when the optical size
 * follows the size, exactly 1 when it stays at the default; above this, the Canvas keeps it.
 */
export const OPSZ_FIXED_ABOVE = 0.955

/** What `canvasOpticalSizing` found: the setting, and the ratio and width it read (for the tests and a report). */
export type OpticalSizingProbe = { sizing: OpticalSizing; ratio: number; em: number; measured: boolean }

/** The probe's width per em at `size` px in `family`, on `ctx`. */
function probeEm(ctx: CanvasRenderingContext2D, family: string, size: number): number {
  ctx.font = `400 ${size}px ${family}`
  return ctx.measureText(OPSZ_PROBE).width / size
}

let probed: Promise<OpticalSizingProbe> | null = null

/**
 * Finds out, once per page, whether the page's Canvas sets Inter's optical size from the font size
 * (`auto`) or keeps its default (`fixed-default`): it measures `OPSZ_PROBE` in the grid's Inter at
 * 14 and 32 px (the axis's ends) and compares their widths per em. Call it once Inter is loaded
 * (`GridFonts.start`: `FontFace.load()` resolved and the face added to `document.fonts`). It does
 * not misfire while a face is loading: the probe's width at 14 px must be Inter's (within 3 %) for
 * the ratio to count; otherwise it waits for `document.fonts.ready` and a frame and measures again,
 * up to three times, and then answers `auto` (the engine's default) with `measured` false.
 */
export function canvasOpticalSizing(): Promise<OpticalSizingProbe> {
  probed ??= (async () => {
    const ctx = document.createElement('canvas').getContext('2d')
    if (!ctx || !('fonts' in document)) return { sizing: 'auto', ratio: 0, em: 0, measured: false }
    if ('fontKerning' in ctx) ctx.fontKerning = 'normal'
    const family = FAMILIES[Face.Sans]
    let last = { ratio: 0, em: 0 }
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (document.fonts.check(`400 14px ${family}`, OPSZ_PROBE)) {
        const em = probeEm(ctx, family, 14)
        const ratio = probeEm(ctx, family, 32) / em
        last = { ratio, em }
        if (Math.abs(em - OPSZ_PROBE_EM) <= 0.03 * OPSZ_PROBE_EM) return { sizing: ratio > OPSZ_FIXED_ABOVE ? 'fixed-default' : 'auto', ratio, em, measured: true }
      }
      await document.fonts.ready
      await new Promise((done) => requestAnimationFrame(() => done(undefined)))
    }
    return { sizing: 'auto', ...last, measured: false }
  })()
  return probed
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
