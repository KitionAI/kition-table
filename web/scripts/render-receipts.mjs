// Draws the Receipt OCR template's receipt pictures from `src/templates/receiptSamples.json` (fictional
// stores, addresses and items) and writes them, with the template's manifest, to
// `public/templates/receipt-ocr-database/`. The template seeds its records from the same data, so a
// picture and its extracted fields always agree.
//
//   node scripts/render-receipts.mjs
//
// Needs Playwright's Chromium and `cwebp`. The receipt is set in Cousine and the invoice in Inter, read
// from the kition-fonts checkout next to the repository (or KITION_FONTS_DIR); fonts are only used
// to draw, never shipped.
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const fonts = resolve(process.env.KITION_FONTS_DIR ?? join(web, '../../kition-fonts'), 'fonts')
const samples = JSON.parse(readFileSync(join(web, 'src/templates/receiptSamples.json'), 'utf8'))
const templateRoot = join(web, 'public/templates/receipt-ocr-database')
const manifestPath = join(templateRoot, 'manifest.json')
const SOURCE = 'Rendered by web/scripts/render-receipts.mjs (Kition)'

for (const file of ['cousine/Cousine-Regular.ttf', 'cousine/Cousine-Bold.ttf', 'inter/Inter-Variable.ttf']) {
  if (!existsSync(join(fonts, file))) throw new Error(`no ${join(fonts, file)}: set KITION_FONTS_DIR to a kition-fonts checkout`)
}

const SCENES = {
  linen: 'background: repeating-linear-gradient(90deg, rgba(0,0,0,.025) 0 2px, transparent 2px 6px), linear-gradient(160deg, #ece7dd, #ddd6c8);',
  walnut: 'background: repeating-linear-gradient(100deg, rgba(255,255,255,.035) 0 3px, transparent 3px 19px), linear-gradient(150deg, #4a2f20, #24150d);',
  desk: 'background: linear-gradient(160deg, #eef0f3, #d9dce2);',
  produce: 'background: repeating-linear-gradient(0deg, rgba(0,0,0,.18) 0 14px, transparent 14px 60px), repeating-linear-gradient(90deg, rgba(0,0,0,.18) 0 14px, transparent 14px 60px), linear-gradient(160deg, #3f7d45, #24502b);',
  tray: 'background: radial-gradient(circle at 30% 20%, rgba(255,255,255,.12), transparent 60%), linear-gradient(160deg, #c2402f, #8e2a1f);',
  dashboard: 'background: radial-gradient(ellipse at 50% 120%, #4b5058, transparent 70%), linear-gradient(170deg, #2d3036, #17191d);',
  plywood: 'background: repeating-linear-gradient(95deg, rgba(120,80,30,.12) 0 2px, transparent 2px 11px), linear-gradient(150deg, #e2c290, #c9a067);',
  marble: 'background: radial-gradient(ellipse at 20% 30%, rgba(160,160,170,.25), transparent 40%), radial-gradient(ellipse at 80% 70%, rgba(150,150,160,.2), transparent 45%), linear-gradient(150deg, #f6f5f2, #e4e2dd);',
  console: 'background: radial-gradient(ellipse at 50% 50%, #3a3a3e, transparent 75%), linear-gradient(160deg, #222226, #0f0f11);',
  concrete: 'background: repeating-radial-gradient(circle at 10% 20%, rgba(0,0,0,.035) 0 1px, transparent 1px 4px), linear-gradient(160deg, #b4b0a9, #8f8b84);',
}

const money = (sample, value) => `${sample.symbol}${value.toFixed(2)}`
const escape = (text) => String(text).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])

/** Both short edges of a thermal receipt are torn: a zigzag polygon for clip-path. */
function torn(teeth = 34) {
  const top = []
  const bottom = []
  for (let i = 0; i <= teeth; i += 1) {
    const x = (i / teeth) * 100
    top.push(`${x.toFixed(2)}% ${i % 2 ? 0 : 1.2}%`)
    bottom.unshift(`${x.toFixed(2)}% ${i % 2 ? 100 : 98.8}%`)
  }
  return `polygon(${[...top, ...bottom].join(', ')})`
}

function receiptBody(sample) {
  const rows = sample.items
    .map((item) => `<div class="row"><span>${escape(item.description)}</span><span>${money(sample, item.quantity * item.unitPrice)}</span></div>`)
    .join('')
  return `
    <div class="vendor">${escape(sample.vendor)}</div>
    ${sample.addressLines.map((line) => `<div class="center">${escape(line)}</div>`).join('')}
    <div class="center muted">${escape([sample.date, sample.time].filter(Boolean).join('  '))}</div>
    <div class="rule"></div>
    ${rows}
    <div class="rule"></div>
    <div class="row"><span>Subtotal</span><span>${money(sample, sample.subtotal)}</span></div>
    <div class="row"><span>${escape(sample.taxLabel)}</span><span>${money(sample, sample.tax)}</span></div>
    <div class="row total"><span>TOTAL</span><span>${money(sample, sample.total)}</span></div>
    <div class="rule"></div>
    <div class="center">${escape(sample.payment)}</div>
    <div class="center muted">Thank you for your visit</div>`
}

function invoiceBody(sample) {
  const rows = sample.items
    .map((item) => `<tr><td>${escape(item.description)}</td><td>${money(sample, item.quantity * item.unitPrice)}</td></tr>`)
    .join('')
  return `
    <div class="issuer">${escape(sample.vendor)}<br>${sample.addressLines.map(escape).join('<br>')}</div>
    <h1>INVOICE</h1>
    <div class="meta">Date ${escape(sample.date)} · ${escape(sample.payment)}</div>
    <table>${rows}
      <tr class="sub"><td>Subtotal</td><td>${money(sample, sample.subtotal)}</td></tr>
      <tr class="sub"><td>${escape(sample.taxLabel)}</td><td>${money(sample, sample.tax)}</td></tr>
      <tr class="grand"><td>TOTAL</td><td>${money(sample, sample.total)}</td></tr>
    </table>`
}

function page(sample) {
  // A page set with setContent may not load file:// fonts: embed them.
  const font = (name, file, weight = 'normal') => `@font-face { font-family: ${name}; src: url(data:font/ttf;base64,${readFileSync(join(fonts, file)).toString('base64')}); font-weight: ${weight}; }`
  const landscape = sample.width > sample.height
  const invoice = sample.style === 'invoice'
  const tilt = (((sample.record * 37) % 7) - 3) * 0.8
  const paperWidth = invoice ? 0.74 : landscape ? 0.46 : 0.66
  const scale = (landscape ? sample.height * 0.62 : sample.width) / 1024 * (invoice ? 1.15 : 1.25)
  return `<!doctype html><html><head><style>
    ${font('Receipt', 'cousine/Cousine-Regular.ttf')}
    ${font('Receipt', 'cousine/Cousine-Bold.ttf', 'bold')}
    ${font('Invoice', 'inter/Inter-Variable.ttf', '100 900')}
    html, body { margin: 0; width: ${sample.width}px; height: ${sample.height}px; overflow: hidden; }
    body { ${SCENES[sample.scene]} display: flex; align-items: center; justify-content: center; }
    .paper { width: ${Math.round(sample.width * paperWidth)}px; transform: rotate(${tilt}deg);
      box-shadow: 0 ${24 * scale}px ${60 * scale}px rgba(0,0,0,.35), 0 ${4 * scale}px ${10 * scale}px rgba(0,0,0,.2); }
    .receipt { background: linear-gradient(180deg, #fdfcf8, #f4f1e8); padding: ${48 * scale}px ${40 * scale}px ${56 * scale}px;
      clip-path: ${torn()}; font-family: Receipt; color: #2b2a28; font-size: ${24 * scale}px; line-height: 1.45; }
    .vendor { text-align: center; font-weight: bold; font-size: ${30 * scale}px; letter-spacing: .04em; margin-bottom: ${6 * scale}px; }
    .center { text-align: center; } .muted { color: #6d6a63; }
    .rule { border-top: ${2 * scale}px dashed #9b978e; margin: ${14 * scale}px 0; }
    .row { display: flex; justify-content: space-between; gap: ${16 * scale}px; }
    .total { font-weight: bold; font-size: ${28 * scale}px; margin-top: ${6 * scale}px; }
    .invoice { background: #ffffff; padding: ${64 * scale}px ${56 * scale}px; font-family: Invoice; color: #1d1f23; font-size: ${22 * scale}px; }
    .invoice .issuer { color: #4b4f57; line-height: 1.5; }
    .invoice h1 { font-size: ${48 * scale}px; letter-spacing: .02em; margin: ${40 * scale}px 0 ${8 * scale}px; }
    .invoice .meta { color: #6b7079; margin-bottom: ${28 * scale}px; }
    .invoice table { width: 100%; border-collapse: collapse; }
    .invoice td { padding: ${12 * scale}px 0; border-bottom: 1px solid #e3e5e9; }
    .invoice td:last-child { text-align: right; }
    .invoice .grand td { font-weight: 700; font-size: ${30 * scale}px; border-top: 2px solid #1d1f23; border-bottom: 0; }
  </style></head><body>
    <div class="paper"><div class="${invoice ? 'invoice' : 'receipt'}">${invoice ? invoiceBody(sample) : receiptBody(sample)}</div></div>
  </body></html>`
}

const browser = await chromium.launch()
const scratch = join(tmpdir(), `kition-receipts-${process.pid}`)
mkdirSync(scratch, { recursive: true })
const assets = []
try {
  for (const sample of samples) {
    const tab = await browser.newPage({ viewport: { width: sample.width, height: sample.height } })
    await tab.setContent(page(sample), { waitUntil: 'load' })
    await tab.evaluate(() => document.fonts.ready)
    const png = join(scratch, `record-${sample.record}.png`)
    await tab.screenshot({ path: png })
    await tab.close()
    const record = `record-${String(sample.record).padStart(2, '0')}`
    const out = join(templateRoot, 'records', record, 'receipt.webp')
    mkdirSync(dirname(out), { recursive: true })
    execFileSync('cwebp', ['-quiet', '-q', '82', '-metadata', 'none', png, '-o', out])
    const bytes = readFileSync(out)
    assets.push({
      id: `${record}-receipt-image`,
      record: sample.record,
      field: 'Receipt Image',
      sourceName: sample.fileName.replace(/\.[a-z]+$/i, '.webp'),
      mimeType: 'image/webp',
      sizeBytes: statSync(out).size,
      width: sample.width,
      height: sample.height,
      sha256: createHash('sha256').update(bytes).digest('hex'),
      path: `templates/receipt-ocr-database/records/${record}/receipt.webp`,
    })
  }
} finally {
  await browser.close()
  rmSync(scratch, { recursive: true, force: true })
}

const manifest = {
  templateId: 'receipt-ocr-database',
  source: SOURCE,
  assetCount: assets.length,
  totalSizeBytes: assets.reduce((sum, asset) => sum + asset.sizeBytes, 0),
  assets,
}
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
console.log(`rendered ${assets.length} receipts into ${templateRoot}`)
