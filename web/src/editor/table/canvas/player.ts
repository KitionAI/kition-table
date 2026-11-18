/**
 * Plays a display list on a Canvas 2D context: the page's player. The engine's rasterizer
 * (`kition-table-display`'s `raster.rs`) draws the same list by the same rules, so the screen and
 * the headless picture agree:
 *
 * - a snapped `RECT`, a `TOFU` box, a `CLIP`, a line's ends and a text run's baseline go to the
 *   device pixel edge `round((v - o) * dpr)` ([`snap`]); a line of `LINES` is the one device pixel row or column
 *   `floor((pos - o) * dpr)` ([`linePixel`]), one device pixel thick at every ratio;
 * - an `EMOJI` and an `ICON` are the engine's tiles, put at the snapped pen position and the
 *   snapped top left of the icon's box: the browser strokes no path;
 * - everything else is drawn at its exact position, antialiased.
 *
 * `o` is the canvas origin in document pixels. A text op is drawn with `ctx.font` naming its one
 * face, its `align` as `textAlign`, on the alphabetic baseline: the engine already cut, wrapped
 * and placed it. A run whose face has not loaded yet is skipped, and the caller draws again when
 * it loads (`GridFonts`).
 */
import { Align, Axis, type DisplayList, Fit, Opcode } from './displayList'
import { FAMILIES, type GridFonts } from './gridFonts'
import { type GridResources, packedColor } from './resources'

/** The device pixel edge of the document coordinate `v` on a canvas whose origin is `origin`. */
export function snap(v: number, origin: number, dpr: number): number {
  return Math.round((v - origin) * dpr)
}

/** The device pixel row (or column) a line at `pos` covers (1/10000 device pixel tolerance). */
export function linePixel(pos: number, origin: number, dpr: number): number {
  return Math.floor((pos - origin) * dpr + 1e-4)
}

/** The color of an image the page has not decoded (the rasterizer's placeholder). */
const IMAGE_PLACEHOLDER = 'rgb(240,240,240)'
const ALIGNS: CanvasTextAlign[] = ['left', 'center', 'right']

export type PlayTarget = {
  ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D
  /** The document point at the canvas's top left. */
  x: number
  y: number
  dpr: number
}

/** What a play left out: runs in faces still loading, icon and emoji tiles not here yet. */
export type PlayResult = { missingFaces: number; missingTiles: number }

export function play(target: PlayTarget, list: DisplayList, resources: GridResources, fonts: GridFonts | null): PlayResult {
  const { ctx, x: ox, y: oy, dpr } = target
  const colors = list.colors
  const result: PlayResult = { missingFaces: 0, missingTiles: 0 }
  // The transform: document space (antialiased shapes, text, icons) or device space (snapped).
  let space = 0
  const toDocument = () => {
    if (space !== 1) {
      ctx.setTransform(dpr, 0, 0, dpr, -ox * dpr, -oy * dpr)
      space = 1
    }
  }
  const toDevice = () => {
    if (space !== 2) {
      ctx.setTransform(1, 0, 0, 1, 0, 0)
      space = 2
    }
  }
  // Canvas state is cached; a clip's restore resets it.
  let fill = ''
  let font = ''
  let align = -1
  const setFill = (color: string) => {
    if (fill !== color) {
      ctx.fillStyle = color
      fill = color
    }
  }
  ctx.save()
  ctx.textBaseline = 'alphabetic'
  if ('fontKerning' in ctx) ctx.fontKerning = 'normal'
  let clips = 0
  for (const op of list.ops) {
    switch (op.op) {
      case Opcode.Rect: {
        setFill(colors[op.paint])
        if (op.snap) {
          toDevice()
          const x0 = snap(op.x, ox, dpr)
          const y0 = snap(op.y, oy, dpr)
          const x1 = snap(op.x + op.w, ox, dpr)
          const y1 = snap(op.y + op.h, oy, dpr)
          if (x1 > x0 && y1 > y0) ctx.fillRect(x0, y0, x1 - x0, y1 - y0)
        } else {
          toDocument()
          ctx.fillRect(op.x, op.y, op.w, op.h)
        }
        break
      }
      case Opcode.RRect:
        toDocument()
        setFill(colors[op.paint])
        ctx.beginPath()
        ctx.roundRect(op.x, op.y, op.w, op.h, Math.max(0, op.r))
        ctx.fill()
        break
      case Opcode.RRectStroke: {
        // The border lies inside the box: the path runs half a width in.
        toDocument()
        const half = op.width / 2
        ctx.strokeStyle = colors[op.paint]
        ctx.lineWidth = op.width
        ctx.beginPath()
        ctx.roundRect(op.x + half, op.y + half, Math.max(0, op.w - op.width), Math.max(0, op.h - op.width), Math.max(0, op.r - half))
        ctx.stroke()
        break
      }
      case Opcode.Lines: {
        toDevice()
        setFill(colors[op.paint])
        const lines = op.lines
        for (let k = 0; k < lines.length; k += 3) {
          if (op.axis === Axis.Horizontal) {
            const row = linePixel(lines[k], oy, dpr)
            const x0 = snap(lines[k + 1], ox, dpr)
            const x1 = snap(lines[k + 2], ox, dpr)
            if (x1 > x0) ctx.fillRect(x0, row, x1 - x0, 1)
          } else {
            const column = linePixel(lines[k], ox, dpr)
            const y0 = snap(lines[k + 1], oy, dpr)
            const y1 = snap(lines[k + 2], oy, dpr)
            if (y1 > y0) ctx.fillRect(column, y0, 1, y1 - y0)
          }
        }
        break
      }
      case Opcode.Text: {
        if (fonts && !fonts.ready(op.face)) {
          result.missingFaces += 1
          break
        }
        toDocument()
        const wanted = `${op.weight} ${op.size}px ${FAMILIES[op.face]}`
        if (font !== wanted) {
          ctx.font = wanted
          font = wanted
        }
        if (align !== op.align) {
          ctx.textAlign = ALIGNS[op.align] ?? 'left'
          align = op.align
        }
        setFill(colors[op.paint])
        // The baseline sits on a device pixel row, as Chromium puts it and the engine draws it.
        ctx.fillText(op.text, op.x, oy + snap(op.y, oy, dpr) / dpr)
        break
      }
      case Opcode.Tofu: {
        toDevice()
        setFill(colors[op.paint])
        const x0 = snap(op.x, ox, dpr)
        const y0 = snap(op.y, oy, dpr)
        const x1 = snap(op.x + op.w, ox, dpr)
        const y1 = snap(op.y + op.h, oy, dpr)
        if (x1 - x0 < 1 || y1 - y0 < 1) break
        ctx.fillRect(x0, y0, x1 - x0, 1)
        if (y1 - y0 > 1) ctx.fillRect(x0, y1 - 1, x1 - x0, 1)
        if (y1 - y0 > 2) {
          ctx.fillRect(x0, y0 + 1, 1, y1 - y0 - 2)
          if (x1 - x0 > 1) ctx.fillRect(x1 - 1, y0 + 1, 1, y1 - y0 - 2)
        }
        break
      }
      case Opcode.Emoji: {
        const tile = resources.emoji(op.atlas, op.size, dpr)
        if (!tile) {
          result.missingTiles += 1
          break
        }
        toDevice()
        ctx.drawImage(tile.image, snap(op.x, ox, dpr) + tile.left, snap(op.y, oy, dpr) + tile.top)
        break
      }
      case Opcode.Icon: {
        const tile = resources.icon(op.icon, op.size, op.stroke, op.solid, packedColor(list.rgba, op.paint), dpr)
        if (!tile) {
          result.missingTiles += 1
          break
        }
        toDevice()
        ctx.drawImage(tile.image, snap(op.x, ox, dpr) + tile.left, snap(op.y, oy, dpr) + tile.top)
        break
      }
      case Opcode.Image: {
        toDocument()
        const image = resources.image(op.key)
        ctx.beginPath()
        ctx.roundRect(op.x, op.y, op.w, op.h, Math.max(0, op.radius))
        if (!image || image.width === 0 || image.height === 0) {
          setFill(IMAGE_PLACEHOLDER)
          ctx.fill()
          break
        }
        const scale = op.fit === Fit.Cover ? Math.max(op.w / image.width, op.h / image.height) : Math.min(op.w / image.width, op.h / image.height)
        const w = image.width * scale
        const h = image.height * scale
        ctx.save()
        ctx.clip()
        ctx.drawImage(image, op.x + (op.w - w) / 2, op.y + (op.h - h) / 2, w, h)
        ctx.restore()
        break
      }
      case Opcode.Shadow: {
        // The shadow alone: the box is drawn far off the canvas and its shadow moved back.
        toDevice()
        const away = 100_000
        ctx.save()
        ctx.shadowColor = colors[op.paint]
        ctx.shadowBlur = 2 * op.blur * dpr
        ctx.shadowOffsetX = away
        ctx.fillStyle = '#000'
        ctx.beginPath()
        ctx.roundRect((op.x - ox) * dpr - away, (op.y - oy) * dpr, op.w * dpr, op.h * dpr, Math.max(0, op.r * dpr))
        ctx.fill()
        ctx.restore()
        break
      }
      case Opcode.Clip: {
        toDevice()
        ctx.save()
        const x0 = snap(op.x, ox, dpr)
        const y0 = snap(op.y, oy, dpr)
        ctx.beginPath()
        ctx.rect(x0, y0, snap(op.x + op.w, ox, dpr) - x0, snap(op.y + op.h, oy, dpr) - y0)
        ctx.clip()
        clips += 1
        break
      }
      case Opcode.Unclip:
        if (clips > 0) {
          ctx.restore()
          clips -= 1
          // The restore brought back the state from before the clip, transform included.
          space = 0
          fill = ''
          font = ''
          align = -1
        }
        break
    }
  }
  while (clips > 0) {
    ctx.restore()
    clips -= 1
  }
  ctx.restore()
  return result
}

/** A run's left end (the engine's `Align::left`): for the tests that compare text boxes. */
export function runLeft(align: Align, x: number, width: number): number {
  return align === Align.Center ? x - width / 2 : align === Align.Right ? x - width : x
}
