import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import type { TFunction } from 'i18next'
import { describe, expect, it } from 'vitest'

import { getBuiltinTableTemplates } from './tableTemplates'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const t = ((key: string) => key) as TFunction<'table'>

describe('table template cover assets', () => {
  it('ships a generated WebP cover for every built-in template', () => {
    for (const template of getBuiltinTableTemplates(t)) {
      const bytes = readFileSync(resolve(packageRoot, 'public', template.coverImage))

      expect(bytes.byteLength).toBeGreaterThan(10_000)
      expect(bytes.subarray(0, 4).toString('ascii')).toBe('RIFF')
      expect(bytes.subarray(8, 12).toString('ascii')).toBe('WEBP')
    }
  })
})
