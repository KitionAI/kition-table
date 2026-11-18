// The grid's faces in the editor package: node scripts/fonts.mjs [--check]
// Copies the files fonts.lock.json names from a kition-fonts checkout (KITION_FONTS_DIR, else the
// kition-fonts checkout beside this repository) into public/fonts/, and refuses any file whose
// SHA-256 or size is not the lock's. With --check it only verifies public/fonts/. Local files
// only: no network (platform rule UI-K1 keeps the package self-contained).
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const web = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
export const LOCK = JSON.parse(fs.readFileSync(path.join(web, 'fonts.lock.json'), 'utf8'))
export const PACKAGE_FONTS = path.join(web, 'public', 'fonts')

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

/** The lock's files missing or different under `dir`, as messages (empty when all match). */
export function verify(dir = PACKAGE_FONTS) {
  const problems = []
  for (const [name, want] of Object.entries(LOCK.files)) {
    const file = path.join(dir, name)
    if (!fs.existsSync(file)) problems.push(`${name}: missing`)
    else if (fs.statSync(file).size !== want.bytes || sha256(file) !== want.sha256) problems.push(`${name}: not the locked file`)
  }
  return problems
}

function copy() {
  const source = path.join(process.env.KITION_FONTS_DIR ?? path.join(web, '..', '..', 'kition-fonts'), 'fonts')
  const problems = verify(source)
  if (problems.length) {
    console.error(`fonts: the kition-fonts checkout at ${source} does not hold the locked files (commit ${LOCK.commit}):\n  ${problems.join('\n  ')}`)
    process.exit(1)
  }
  for (const name of Object.keys(LOCK.files)) {
    fs.mkdirSync(path.dirname(path.join(PACKAGE_FONTS, name)), { recursive: true })
    fs.copyFileSync(path.join(source, name), path.join(PACKAGE_FONTS, name))
  }
  console.log(`fonts: ${Object.keys(LOCK.files).length} files in public/fonts`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--check')) {
    const problems = verify()
    if (problems.length) {
      console.error(`fonts: public/fonts does not match fonts.lock.json (run node scripts/fonts.mjs):\n  ${problems.join('\n  ')}`)
      process.exit(1)
    }
    console.log('fonts: public/fonts matches fonts.lock.json')
  } else copy()
}
