import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TemplateLibraryDialog } from './TemplateLibraryDialog'

;(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true

let container: HTMLDivElement
let root: Root | null = null

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
})

afterEach(() => {
  act(() => root?.unmount())
  root = null
  container.remove()
  vi.clearAllMocks()
  vi.unstubAllGlobals()
})

async function mount(onSelect = vi.fn().mockResolvedValue(true)) {
  await act(async () => {
    root = createRoot(container)
    root.render(createElement(TemplateLibraryDialog, {
      open: true,
      onOpenChange: vi.fn(),
      onSelect,
    }))
    await Promise.resolve()
  })
  return onSelect
}

describe('TemplateLibraryDialog', () => {
  it('renders the categorized template center and a blank option', async () => {
    await mount()

    expect(document.body.textContent).toContain('Template Center')
    const dialog = document.querySelector('[data-testid="table-template-library-dialog"]')
    expect(dialog?.className).toContain('md:!w-[min(1040px')
    expect(dialog?.className).not.toContain('md:w-full')
    expect(document.body.textContent).toContain('New table workspace')
    expect(document.querySelectorAll('button[data-testid^="workspace-template-category-"]').length).toBe(4)
    expect(document.querySelector('[data-testid="table-template-task-tracker"]')).not.toBeNull()
    expect(document.querySelector('[data-testid="table-template-email-inbox-sync"]')).toBeNull()
    expect(document.querySelector('[data-testid="table-template-product-launch-website"]')).not.toBeNull()
    expect(document.querySelector('[data-testid="table-template-blank"]')).not.toBeNull()
    expect(document.querySelector('textarea')).toBeNull()
  })

  it('keeps the business catalog focused on active templates', async () => {
    await mount()

    await act(async () => {
      const category = document.querySelector('[data-testid="workspace-template-category-business"]') as HTMLButtonElement
      category.click()
      await Promise.resolve()
    })

    expect(document.querySelector('[data-testid="table-template-ecommerce-orders-returns"]')).toBeNull()
    expect(document.querySelector('[data-testid="table-template-recruitment-pipeline"]')).toBeNull()
    expect(document.querySelector('[data-testid="table-template-product-launch-website"]')).not.toBeNull()
  })

  it('shows the generated thumbnail template cover and only the source table resource', async () => {
    const manifest = {
      templateId: 'thumbnail-generator',
      source: 'Test assets',
      assetCount: 1,
      totalSizeBytes: 128,
      assets: [{
        id: 'record-01-face-photo-01',
        record: 1,
        field: 'Face Photo',
        sourceName: 'face-photo-01.png',
        mimeType: 'image/png',
        sizeBytes: 128,
        width: 1024,
        height: 1536,
        sha256: 'test-sha256',
        path: 'templates/youtube-tiktok-thumbnail-generator/records/record-01/face-photo-01.png',
      }],
    }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify(manifest), {
      headers: { 'content-type': 'application/json' },
    })))
    await mount()

    const card = document.querySelector('[data-testid="table-template-thumbnail-generator"]') as HTMLButtonElement
    const preview = card.querySelector('img')
    expect(preview?.getAttribute('src')).toBe(
      new URL('templates/table-covers/thumbnail-generator.webp', document.baseURI).href,
    )

    await act(async () => {
      card.click()
      await Promise.resolve()
    })

    expect(document.querySelector('[data-testid="table-template-resource-thumbnail-workbench"]')).not.toBeNull()
    expect(document.body.textContent).not.toContain('Thumbnail generation')
    expect(document.querySelector('[data-testid="table-template-back"] .tessera-icon-arrow-left')).not.toBeNull()
    await vi.waitFor(() => {
      const image = document.querySelector('[data-testid="table-template-preview-asset-record-01-face-photo-01"]')
      expect(image?.getAttribute('src')).toBe(
        new URL('templates/youtube-tiktok-thumbnail-generator/records/record-01/face-photo-01.png', document.baseURI).href,
      )
    })
    expect(document.body.textContent).not.toContain('[object Object]')
  })

  it('shows the generated product design template cover', async () => {
    await mount()

    await act(async () => {
      const category = document.querySelector('[data-testid="workspace-template-category-business"]') as HTMLButtonElement
      category.click()
      await Promise.resolve()
    })

    const card = document.querySelector('[data-testid="table-template-batch-product-designer"]') as HTMLButtonElement
    const preview = card.querySelector('img')
    expect(preview?.getAttribute('src')).toBe(
      new URL('templates/table-covers/batch-product-designer.webp', document.baseURI).href,
    )
  })

  it('opens a read-only resource preview before using the selected package', async () => {
    const onSelect = await mount()

    await act(async () => {
      const card = document.querySelector('[data-testid="table-template-simple-client-crm"]') as HTMLButtonElement
      card.click()
      await Promise.resolve()
    })

    expect(onSelect).not.toHaveBeenCalled()
    expect(document.querySelector('[data-testid="table-template-detail"]')).not.toBeNull()
    expect(document.querySelector('[data-testid="table-template-resource-clients"]')).not.toBeNull()
    expect(document.body.textContent).toContain('Read-only preview')

    await act(async () => {
      const useButton = document.querySelector('[data-testid="table-template-use"]') as HTMLButtonElement
      useButton.click()
      await Promise.resolve()
    })

    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({
      id: 'simple-client-crm',
      snapshot: expect.objectContaining({
        includeData: true,
        defaultResourceId: 'clients',
      }),
      tables: expect.arrayContaining([
        expect.objectContaining({ title: 'Clients', records: expect.any(Array) }),
        expect.objectContaining({ title: 'Quotes', records: expect.any(Array) }),
      ]),
    }))
  })
})
