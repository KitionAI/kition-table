import { createHash } from 'node:crypto'
import { readFileSync, statSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

import type { TableTemplateAssetManifest } from './templateAssets'
import { readTemplateAssetImageDimensions } from './templateAssetImageDimensions'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const templateRoot = resolve(packageRoot, 'public/templates/receipt-ocr-database')
const manifestPath = resolve(templateRoot, 'manifest.json')

describe('receipt OCR assets', () => {
  it('ships a generated cover and ten rendered receipt images with verified metadata', () => {
    const rawManifest = readFileSync(manifestPath, 'utf8')
    const manifest = JSON.parse(rawManifest) as TableTemplateAssetManifest
    const coverBytes = readFileSync(resolve(templateRoot, 'cover.webp'))

    expect(rawManifest).not.toContain('X-Amz-')
    expect(manifest.templateId).toBe('receipt-ocr-database')
    expect(manifest.source).toBe('Rendered by web/scripts/render-receipts.mjs (Kition)')
    expect(manifest.assetCount).toBe(10)
    expect(manifest.totalSizeBytes).toBe(
      manifest.assets.reduce((sum, asset) => sum + asset.sizeBytes, 0),
    )
    expect(manifest.totalSizeBytes).toBeLessThan(5_000_000)
    expect(new Set(manifest.assets.map((asset) => asset.id)).size).toBe(10)
    expect(manifest.assets.every((asset) => asset.field === 'Receipt Image')).toBe(true)
    expect(manifest.assets.every((asset) => asset.mimeType === 'image/webp')).toBe(true)
    expect(manifest.assets.every((asset) => asset.path.endsWith('.webp'))).toBe(true)
    expect(manifest.assets.every((asset) => asset.sourceName.endsWith('.webp'))).toBe(true)
    expect(readTemplateAssetImageDimensions(coverBytes)).toEqual({ width: 1672, height: 941 })

    for (const asset of manifest.assets) {
      const filePath = resolve(packageRoot, 'public', asset.path)
      const bytes = readFileSync(filePath)
      expect(statSync(filePath).size, asset.id).toBe(asset.sizeBytes)
      expect(createHash('sha256').update(bytes).digest('hex'), asset.id).toBe(asset.sha256)
      expect(readTemplateAssetImageDimensions(bytes), asset.id).toEqual({
        width: asset.width,
        height: asset.height,
      })
    }
  })
})
