// Draws the template library's cover pictures (`public/templates/table-covers/<template>.webp`,
// 960x540) from HTML: the template's name and a plain drawing of what it holds (rows, a chart, a
// board, a schedule), in Kition's light surfaces with the Table kit's teal and Kition's purple
// (kition docs/design.md).
//
//   node scripts/render-covers.mjs
//
// Needs Playwright's Chromium and `cwebp`. Text is set in Inter from the kition-fonts checkout next
// to the repository (or KITION_FONTS_DIR); the font is only used to draw, never shipped.
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const inter = join(resolve(process.env.KITION_FONTS_DIR ?? join(web, '../../kition-fonts'), 'fonts'), 'inter/Inter-Variable.ttf')
if (!existsSync(inter)) throw new Error(`no ${inter}: set KITION_FONTS_DIR to a kition-fonts checkout`)
const out = join(web, 'public/templates/table-covers')

const TEAL = '#0c8375'
const PURPLE = '#5645d4'
const SOFT = { teal: '#e3f3f0', purple: '#ecebfb', orange: '#fdeee2', green: '#e5f6e8', yellow: '#fdf6d8', pink: '#fde9f5' }
const INK = '#1f2328'
const MUTED = '#8a8f98'
const LINE = '#e6e7eb'

const bar = (width, color = '#dfe1e6', height = 9) => `<i class="bar" style="width:${width}px;height:${height}px;background:${color}"></i>`
const pill = (text, bg, fg) => `<span class="pill" style="background:${bg};color:${fg}">${text}</span>`
const header = (cells) => `<div class="tr th">${cells.map((c) => `<span>${c}</span>`).join('')}</div>`
const rows = (n, render) => Array.from({ length: n }, (_, i) => `<div class="tr">${render(i)}</div>`).join('')
const swatch = (color, w, h, extra = '') => `<i class="sw" style="width:${w}px;height:${h}px;background:${color};${extra}"></i>`

const COVERS = {
  'task-tracker': ['Task Tracker', 'Tasks, owners and status in one table', () => `
    ${header(['Task', 'Owner', 'Status', 'Due'])}
    ${rows(6, (i) => `<span>${swatch(i < 2 ? TEAL : '#fff', 14, 14, `border:2px solid ${i < 2 ? TEAL : '#c9ccd2'};border-radius:4px`)} ${bar(150 - i * 9)}</span><span>${swatch(['#f4b9a0', '#b9c8f4', '#c9e8c0'][i % 3], 22, 22, 'border-radius:50%')}</span><span>${pill(['Done', 'Done', 'In progress', 'In progress', 'To do', 'Blocked'][i], [SOFT.green, SOFT.green, SOFT.teal, SOFT.teal, '#f1f2f4', SOFT.pink][i], [TEAL, TEAL, TEAL, TEAL, MUTED, '#a02e6d'][i])}</span><span>${bar(56)}</span>`)}`],
  'business-analytics-dashboard': ['Business Analytics', 'Revenue, pipeline and KPIs at a glance', () => `
    <div class="kpis">${['$184k', '1,204', '32%'].map((v, i) => `<div class="kpi"><small>${['Revenue', 'Orders', 'Margin'][i]}</small><b>${v}</b>${bar(70, i === 2 ? PURPLE : TEAL, 5)}</div>`).join('')}</div>
    <div class="chart">${[46, 62, 54, 78, 70, 92, 84, 108, 98, 126].map((h, i) => swatch(i % 2 ? TEAL : PURPLE, 26, h, 'border-radius:5px 5px 0 0;opacity:.85')).join('')}</div>`],
  'leads-landing-page': ['Leads Landing Page', 'A form that fills a qualified leads table', () => `
    <div class="split"><div class="form"><b>Get a demo</b>${['Name', 'Work email', 'Company'].map((f) => `<small>${f}</small><div class="input"></div>`).join('')}<div class="button">Submit</div></div>
    <div class="mini">${header(['Lead', 'Score'])}${rows(5, (i) => `<span>${bar(110 - i * 8)}</span><span>${pill(['92', '87', '74', '61', '48'][i], i < 2 ? SOFT.teal : SOFT.purple, i < 2 ? TEAL : PURPLE)}</span>`)}</div></div>`],
  'lumiere-restaurant': ['Restaurant Reservations', 'Service schedule, guests and private events', () => `
    <div class="slots">${['18:00', '18:30', '19:00', '19:30', '20:00', '20:30'].map((t, i) => `<div class="slot ${[1, 2, 4].includes(i) ? 'busy' : ''}"><small>${t}</small>${bar(60, [1, 2, 4].includes(i) ? 'rgba(255,255,255,.7)' : '#dfe1e6', 7)}</div>`).join('')}</div>
    ${header(['Guest', 'Party', 'Table', 'Status'])}${rows(4, (i) => `<span>${bar(120 - i * 10)}</span><span>${['2', '4', '6', '2'][i]}</span><span>${['T4', 'T9', 'P1', 'T2'][i]}</span><span>${pill(['Seated', 'Confirmed', 'Event', 'Waitlist'][i], [SOFT.green, SOFT.teal, SOFT.purple, SOFT.yellow][i], [TEAL, TEAL, PURPLE, '#8a6d00'][i])}</span>`)}`],
  'product-launch-website': ['Product Launch Website', 'Sections, copy and assets ready to ship', () => `
    <div class="browser"><div class="dots">${swatch('#f4b9a0', 10, 10, 'border-radius:50%')}${swatch('#f5d75e', 10, 10, 'border-radius:50%')}${swatch('#9fd9a8', 10, 10, 'border-radius:50%')}</div>
    <div class="hero">${bar(220, INK, 14)}${bar(160, '#c9ccd2')}<div class="button">Pre-order</div></div>
    <div class="cards">${[SOFT.teal, SOFT.purple, SOFT.orange].map((c) => `<div class="card" style="background:${c}">${bar(70, 'rgba(0,0,0,.18)', 7)}</div>`).join('')}</div></div>`],
  'project-gantt': ['Project Gantt', 'Phases, dependencies and dates on a timeline', () => `
    <div class="gantt">${['Research', 'Design', 'Build', 'Test', 'Launch'].map((p, i) => `<div class="g"><small>${p}</small><div class="lane">${swatch(i % 2 ? PURPLE : TEAL, [150, 190, 260, 140, 90][i], 18, `margin-left:${[0, 110, 220, 420, 520][i]}px;border-radius:6px;opacity:.85`)}</div></div>`).join('')}</div>`],
  'receipt-ocr-database': ['Receipt OCR', 'Receipt pictures turned into searchable fields', () => `
    <div class="split"><div class="receipts">${[0, 1, 2].map((i) => `<div class="paper" style="transform:rotate(${[-6, 3, -1][i]}deg) translate(${i * 26}px, ${i * 10}px)">${bar(70, INK, 7)}${bar(90)}${bar(80)}${bar(96)}${bar(60, INK, 7)}</div>`).join('')}</div>
    <div class="mini">${header(['Vendor', 'Category', 'Total'])}${rows(5, (i) => `<span>${bar(90 - i * 6)}</span><span>${pill(['Food', 'Fuel', 'Food', 'Software', 'Fuel'][i], [SOFT.green, SOFT.orange, SOFT.green, SOFT.purple, SOFT.orange][i], [TEAL, '#b54700', TEAL, PURPLE, '#b54700'][i])}</span><span>${bar(40)}</span>`)}</div></div>`],
  'sdr-cold-call-manager': ['Cold Call Manager', 'A prioritised call queue with next actions', () => `
    ${header(['Lead', 'Priority', 'Attempts', 'Next action'])}${rows(6, (i) => `<span>${swatch(['#f4b9a0', '#b9c8f4', '#c9e8c0', '#f5d75e', '#d6b6f6', '#9fd9a8'][i], 22, 22, 'border-radius:50%')} ${bar(110 - i * 6)}</span><span>${pill(['High', 'High', 'Medium', 'Medium', 'Low', 'Low'][i], i < 2 ? SOFT.pink : i < 4 ? SOFT.yellow : '#f1f2f4', i < 2 ? '#a02e6d' : i < 4 ? '#8a6d00' : MUTED)}</span><span>${'●'.repeat(1 + (i % 3))}</span><span>${bar(90)}</span>`)}`],
  'simple-client-crm': ['Client CRM', 'Prospects and customers through the pipeline', () => `
    <div class="board">${['Lead', 'Qualified', 'Proposal', 'Won'].map((col, c) => `<div class="col"><small>${col}</small>${Array.from({ length: 3 - (c % 2) }, (_, i) => `<div class="tile">${bar(90 - i * 12, INK, 7)}${bar(60)}${pill(['$4k', '$12k', '$7k'][i], c === 3 ? SOFT.green : SOFT.purple, c === 3 ? TEAL : PURPLE)}</div>`).join('')}</div>`).join('')}</div>`],
  'batch-product-designer': ['Batch Product Designer', 'Briefs and generated product pictures side by side', () => `
    <div class="grid">${[SOFT.teal, SOFT.purple, SOFT.orange, SOFT.green, SOFT.yellow, SOFT.pink, SOFT.purple, SOFT.teal].map((c, i) => `<div class="shot" style="background:${c}">${swatch(i % 2 ? PURPLE : TEAL, 40 + (i % 3) * 10, 40 + (i % 3) * 10, `border-radius:${i % 2 ? 50 : 12}%;opacity:.55`)}</div>`).join('')}</div>`],
  'thumbnail-generator': ['Thumbnail Generator', '16:9 and 9:16 video thumbnails from one brief', () => `
    <div class="thumbs"><div class="wide">${bar(150, '#ffffff', 16)}${bar(110, '#f5d75e', 16)}</div><div class="wide alt">${bar(130, '#ffffff', 16)}${bar(90, '#ffffff', 16)}</div>
    <div class="tall">${bar(70, '#ffffff', 14)}${bar(56, '#f5d75e', 14)}</div><div class="tall alt">${bar(70, '#ffffff', 14)}${bar(50, '#ffffff', 14)}</div></div>`],
}

const page = ([title, subtitle, body]) => `<!doctype html><html><head><style>
  @font-face { font-family: Inter; src: url(data:font/ttf;base64,${readFileSync(inter).toString('base64')}); font-weight: 100 900; }
  html, body { margin: 0; width: 960px; height: 540px; overflow: hidden; }
  body { font-family: Inter; color: ${INK}; background: radial-gradient(ellipse at 85% 0%, ${SOFT.purple}, transparent 55%), radial-gradient(ellipse at 0% 100%, ${SOFT.teal}, transparent 50%), #f7f7f9; }
  .title { position: absolute; left: 56px; top: 44px; }
  .title b { display: block; font-size: 34px; font-weight: 700; letter-spacing: -.01em; }
  .title small { display: block; margin-top: 6px; font-size: 17px; color: ${MUTED}; }
  .panel { position: absolute; left: 56px; right: 56px; top: 150px; bottom: 40px; background: #fff; border: 1px solid ${LINE}; border-radius: 12px; box-shadow: 0 10px 30px rgba(31,35,40,.08); padding: 20px 24px; box-sizing: border-box; overflow: hidden; font-size: 14px; }
  .bar, .sw { display: inline-block; border-radius: 5px; vertical-align: middle; }
  .tr { display: grid; grid-template-columns: 2.4fr 1fr 1fr 1fr; align-items: center; gap: 12px; padding: 8px 0; border-bottom: 1px solid ${LINE}; }
  .th { color: ${MUTED}; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; }
  .pill { display: inline-block; padding: 3px 10px; border-radius: 6px; font-size: 12px; font-weight: 600; }
  .kpis { display: flex; gap: 16px; } .kpi { flex: 1; border: 1px solid ${LINE}; border-radius: 10px; padding: 10px 14px; }
  .kpi small { color: ${MUTED}; display: block; } .kpi b { font-size: 24px; display: block; margin: 2px 0 6px; }
  .chart { display: flex; align-items: flex-end; gap: 14px; height: 140px; margin-top: 18px; padding-left: 8px; border-bottom: 1px solid ${LINE}; }
  .split { display: grid; grid-template-columns: 1fr 1.3fr; gap: 28px; height: 100%; }
  .split .mini .tr { grid-template-columns: 1.6fr 1fr 0.7fr; }
  .form { border: 1px solid ${LINE}; border-radius: 10px; padding: 14px 16px; } .form b { font-size: 18px; display: block; margin-bottom: 6px; }
  .form small { color: ${MUTED}; display: block; margin-top: 8px; } .input { height: 26px; border: 1px solid ${LINE}; border-radius: 6px; margin-top: 4px; }
  .button { display: inline-block; margin-top: 12px; background: ${PURPLE}; color: #fff; border-radius: 8px; padding: 7px 16px; font-weight: 600; }
  .slots { display: flex; gap: 10px; margin-bottom: 10px; } .slot { flex: 1; border: 1px solid ${LINE}; border-radius: 8px; padding: 6px 10px; }
  .slot small { display: block; color: ${MUTED}; } .slot.busy { background: ${TEAL}; border-color: ${TEAL}; } .slot.busy small { color: #fff; }
  .browser { border: 1px solid ${LINE}; border-radius: 10px; height: 100%; overflow: hidden; } .dots { display: flex; gap: 6px; padding: 8px 12px; border-bottom: 1px solid ${LINE}; }
  .hero { padding: 16px 22px; display: grid; gap: 10px; justify-items: start; } .cards { display: flex; gap: 14px; padding: 0 22px; }
  .card { flex: 1; height: 70px; border-radius: 10px; padding: 12px; box-sizing: border-box; }
  .gantt .g { display: grid; grid-template-columns: 90px 1fr; align-items: center; padding: 9px 0; border-bottom: 1px solid ${LINE}; }
  .gantt small { color: ${MUTED}; } .lane { background: repeating-linear-gradient(90deg, transparent 0 69px, ${LINE} 69px 70px); }
  .receipts { position: relative; } .paper { position: absolute; top: 4px; left: 20px; width: 130px; padding: 14px 12px; background: #fdfcf8; border: 1px solid ${LINE}; border-radius: 4px; box-shadow: 0 6px 16px rgba(0,0,0,.08); display: grid; gap: 8px; }
  .board { display: grid; grid-template-columns: repeat(4, 1fr); gap: 14px; height: 100%; } .col { background: #f6f7f9; border-radius: 10px; padding: 10px; display: grid; gap: 8px; align-content: start; }
  .col small { color: ${MUTED}; font-weight: 600; } .tile { background: #fff; border: 1px solid ${LINE}; border-radius: 8px; padding: 8px; display: grid; gap: 6px; justify-items: start; }
  .grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 14px; height: 100%; } .shot { border-radius: 10px; display: flex; align-items: center; justify-content: center; }
  .thumbs { display: grid; grid-template-columns: 1.6fr 1.6fr 0.8fr 0.8fr; gap: 14px; height: 100%; align-items: center; }
  .wide, .tall { border-radius: 10px; background: linear-gradient(135deg, ${PURPLE}, #2c2470); padding: 14px; box-sizing: border-box; display: grid; gap: 8px; align-content: center; }
  .wide { aspect-ratio: 16 / 9; } .tall { aspect-ratio: 9 / 16; } .alt { background: linear-gradient(135deg, ${TEAL}, #064c44); }
</style></head><body><div class="title"><b>${title}</b><small>${subtitle}</small></div><div class="panel">${body()}</div></body></html>`

const names = Object.keys(COVERS)
const browser = await chromium.launch()
const scratch = join(tmpdir(), `kition-covers-${process.pid}`)
mkdirSync(scratch, { recursive: true })
try {
  for (const name of names) {
    const tab = await browser.newPage({ viewport: { width: 960, height: 540 } })
    await tab.setContent(page(COVERS[name]), { waitUntil: 'load' })
    await tab.evaluate(() => document.fonts.ready)
    const png = join(scratch, `${name}.png`)
    await tab.screenshot({ path: png })
    await tab.close()
    execFileSync('cwebp', ['-quiet', '-q', '95', '-metadata', 'none', png, '-o', join(out, `${name}.webp`)])
  }
} finally {
  await browser.close()
  rmSync(scratch, { recursive: true, force: true })
}
console.log(`rendered ${names.length} covers into ${out}`)
