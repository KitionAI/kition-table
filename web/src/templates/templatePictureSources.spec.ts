import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

const web = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const templates = join(web, 'public/templates')
const attribution = readFileSync(join(web, '../docs/attribution.md'), 'utf8')

function pictures(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return pictures(path)
    return /\.(webp|png|jpe?g)$/i.test(entry.name) ? [relative(join(web, 'public'), path)] : []
  })
}

/** Pictures a template manifest lists, with the source the manifest records for them. */
function manifestSources(): Map<string, string> {
  const sources = new Map<string, string>()
  for (const name of readdirSync(templates)) {
    let manifest: { source?: string, assets?: Array<{ path: string }> }
    try {
      manifest = JSON.parse(readFileSync(join(templates, name, 'manifest.json'), 'utf8'))
    } catch {
      continue
    }
    for (const asset of manifest.assets ?? []) sources.set(asset.path, manifest.source ?? '')
  }
  return sources
}

/** docs/attribution.md's asset rows: the paths each row names (in backticks) and its licence column. */
function attributionRows(): Array<{ paths: string[], licence: string }> {
  return attribution.split('\n')
    .filter((line) => line.startsWith('| `web/public/templates/'))
    .map((line) => {
      const cells = line.split('|').map((cell) => cell.trim())
      return { paths: [...cells[1].matchAll(/`([^`]+)`/g)].map((match) => match[1]), licence: cells[4] ?? '' }
    })
}

describe('template pictures', () => {
  it('each has a recorded source: its manifest, or a docs/attribution.md row for its folder with a known licence', () => {
    const listed = manifestSources()
    const rows = attributionRows()
    const unsourced = pictures(templates).filter((picture) => {
      const fromManifest = listed.get(picture)
      if (fromManifest && !/unknown/i.test(fromManifest)) return false
      const row = rows.find((candidate) => candidate.paths.some((path) => `web/public/${picture}`.startsWith(path.replace(/\*\*\/\*\.webp$/, ''))))
      return !row || /unknown/i.test(row.licence)
    })
    expect(unsourced).toEqual([])
  })
})
