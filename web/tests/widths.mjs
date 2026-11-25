// The text width gate (grid plan section 3.5.7): every run the engine sets is as wide as the
// browser measures it, so the page never lays text out and the screen matches the picture:
// node tests/widths.mjs [chromium|webkit]
//
// The page's engine splits each string into runs of one face (`gridMeasure`, the grid's own
// shaping, with the optical sizing the page found its Canvas uses: `canvasOpticalSizing`) and gives
// each run's width; the page measures the same run with `measureText` in the same face, weight and
// size. The page's finding is printed and must be a measurement (the probe read Inter, not a face
// still loading). Every run within 200 px must agree within 0.5 CSS px; above 0.1 px it is
// reported as a warning. The corpus holds Latin, kerning pairs, ligature candidates, numbers and
// dates, punctuation, CJK, Japanese, Hangul and mixed lines at weights 400, 500 and 600 and sizes
// 12, 13 and 14 px, and at 16 and 20 px (where Inter's optical size follows the size in Chromium's
// Canvas and stays at its default, 14, in WebKit's). Characters no face covers are tofu boxes the
// engine places itself; they are not measured.
//
// It also reports (INFO, not checked) whether this browser's Canvas would let the page pin Inter's
// optical size itself instead: a FontFace with a `variationSettings` descriptor, and the canvas
// element's `font-optical-sizing: none` and `font-variation-settings` styles.
import fs from 'node:fs'
import path from 'node:path'
import { REPO, checkPackage, launch, nodeEngine, openGrid, startServer } from './gridPage.mjs'

const which = process.argv[2] ?? 'chromium'
// The gate and the warning: case table.editor.grid/parity's tiers.
const tiers = JSON.parse(fs.readFileSync(path.join(REPO, 'cases', 'table.editor.grid', 'parity', 'expected', 'tiers.json'), 'utf8'))
const GATE = tiers.width
const WARN = tiers.widthWarn

export const CORPUS = [
  'Quarterly planning review',
  'AVATAR WAVE Typography office fjord',
  'To-do: follow up with the customer',
  '-> => != <= >= 1/2 x2 ff fi fl ffi',
  '1,234,567.89',
  '$ 98.10 € 1.000,00 ¥ 3,200',
  '2026-11-17 14:05',
  '+1 (415) 555-0134',
  '“Quotes”, ‘single’ — and … (parens) [brackets] {braces}',
  'Shipping delayed (approved)',
  '季度计划',
  '客户反馈：第二轮需求评审',
  '项目进度（已完成）',
  '東京オフィス 移転のお知らせ',
  'カタカナとひらがな',
  '서울 지사 회의록',
  '한국어 텍스트와 English',
  'Mixed 中文 English 한국어 日本語 text',
  'Launch 发布 2026',
  'iiiiii WWWWWW',
]
const SIZES = [12, 13, 14, 16, 20]
const WEIGHTS = [400, 500, 600]

checkPackage()
const sdk = await nodeEngine()
const doc = sdk.TableDocument.create('Widths.ktable', 0, 0)
const asks = []
for (const text of CORPUS) for (const size of SIZES) for (const weight of WEIGHTS) asks.push({ text, size, weight })
// The page measures with the faces the development page loads; any table opens it.
doc.markSaved()
const bytes = doc.save()
doc.free()

/** Whether this browser's Canvas lets a page pin Inter's optical size (reported, not checked); runs in the page. */
async function pinnable() {
  const probe = 'Quarterly planning review'
  const bytes = await (await fetch('/fonts/inter/Inter-Variable.ttf')).arrayBuffer()
  const ctx = document.createElement('canvas').getContext('2d')
  const em = (family, size, target = ctx) => {
    target.font = `400 ${size}px ${family}`
    return target.measureText(probe).width / size
  }
  const out = { plain: em('KitionTableSans', 32) / em('KitionTableSans', 14) }
  for (const [name, settings] of [['descriptor opsz 14', "'opsz' 14"], ['descriptor opsz 32', "'opsz' 32"]]) {
    const family = `OpszProbe${name.replace(/\W/g, '')}`
    try {
      const face = new FontFace(family, bytes, { weight: '100 900', variationSettings: settings })
      await face.load()
      document.fonts.add(face)
      out[name] = em(family, 32) / em('KitionTableSans', 14)
    } catch (error) {
      out[name] = `error ${error}`
    }
  }
  for (const [name, css] of [['canvas style font-optical-sizing none', 'font-optical-sizing:none'], ["canvas style font-variation-settings 'opsz' 14", "font-variation-settings:'opsz' 14"]]) {
    const canvas = document.createElement('canvas')
    canvas.style.cssText = css
    document.body.append(canvas)
    out[name] = em('KitionTableSans', 32, canvas.getContext('2d')) / em('KitionTableSans', 14)
    canvas.remove()
  }
  return out
}

const { server, origin } = await startServer()
const browser = await launch(which)
const version = browser.version()
let failures = 0
try {
  for (const dpr of [1, 2]) {
    const { context, page } = await openGrid(browser, origin, { dpr, viewport: { width: 800, height: 600 }, files: { 'widths.ktable': bytes }, query: { file: '/__grid/widths.ktable' } })
    await page.evaluate(() => window.__ktableGrid.view.allFonts())
    const sizing = await page.evaluate(() => window.__ktableGrid.view.textSizing)
    const detected = Boolean(sizing?.measured)
    if (!detected) failures += 1
    console.log(`${detected ? 'PASS' : 'FAIL'} widths @${dpr} optical sizing - the page's Canvas: ${sizing?.sizing} (probe 32/14 px per-em ratio ${sizing?.ratio?.toFixed(4)}, ${sizing?.em?.toFixed(4)} em at 14 px${detected ? '' : ', not measured'})`)
    if (dpr === 1) {
      const pins = await page.evaluate(pinnable)
      console.log(`INFO optical size pinning in this Canvas (32/14 px per-em ratio; 1 = opsz 14 kept, 0.910 = opsz follows size): ${Object.entries(pins).map(([name, value]) => `${name} ${typeof value === 'number' ? value.toFixed(4) : value}`).join('; ')}`)
    }
    const measured = JSON.parse(await page.evaluate((asks) => window.__ktableGrid.engine.invoke('gridMeasure', JSON.stringify(asks)), asks))
    const runs = measured.flatMap((runs, index) => runs.map((run) => ({ ...run, size: asks[index].size, weight: asks[index].weight })))
    const widths = await page.evaluate((runs) => {
      const ctx = document.createElement('canvas').getContext('2d')
      const families = ['KitionTableSans', 'KitionTableFallback', 'KitionTableHangul', 'KitionTableMono']
      if ('fontKerning' in ctx) ctx.fontKerning = 'normal'
      return runs.map((run) => {
        ctx.font = `${run.weight} ${run.size}px ${families[run.face]}`
        return ctx.measureText(run.text).width
      })
    }, runs)
    const compared = runs.map((run, index) => ({ ...run, page: widths[index], diff: Math.abs(widths[index] - run.width) })).filter((run) => run.width <= 200)
    const over = compared.filter((run) => run.diff > GATE)
    const loose = compared.filter((run) => run.diff > WARN)
    const worst = compared.reduce((worst, run) => (run.diff > worst.diff ? run : worst), { diff: 0 })
    const ok = compared.length > 0 && over.length === 0
    if (!ok) failures += 1
    console.log(`${ok ? 'PASS' : 'FAIL'} widths @${dpr} - ${compared.length} runs, max ${worst.diff.toFixed(3)} px${worst.text ? ` ("${worst.text}" ${worst.weight} ${worst.size}px)` : ''}, ${over.length} over ${GATE} px`)
    for (const run of over.slice(0, 10)) console.log(`  "${run.text}" face ${run.face} ${run.weight} ${run.size}px: engine ${run.width.toFixed(3)}, page ${run.page.toFixed(3)}`)
    if (loose.length) console.log(`WARN widths @${dpr} - ${loose.length} runs differ by more than ${WARN} px`)
    await context.close()
  }
} catch (error) {
  failures += 1
  console.log(`FAIL widths - ${error instanceof Error ? error.stack : error}`)
} finally {
  await browser.close()
  await server.close()
}
console.log(`${failures ? 'FAIL' : 'PASS'} widths (${which} ${version})`)
process.exit(failures ? 1 : 0)
