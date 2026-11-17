import type {
  TableTemplateAssetReference,
  TableTemplateRecordValue,
} from './tableTemplates'
import type { DataAttachment, DataRecordValue } from '../editor'

/**
 * The URL of a template asset in this package (`templates/…` under `public/`): assets ship inside
 * the kit's package, so they load from beside the page wherever a host serves it.
 */
export function templateAssetURL(path: string) {
  return new URL(path.replace(/^\/+/, ''), document.baseURI).href
}

async function fetchAsset(path: string, signal?: AbortSignal) {
  const response = await fetch(templateAssetURL(path), { signal })
  if (!response.ok) throw new Error(`Template asset could not be loaded: ${path}`)
  return response
}

export type TableTemplateAssetManifestItem = {
  id: string
  record: number
  field: string
  sourceName: string
  mimeType: string
  sizeBytes: number
  width: number
  height: number
  sha256: string
  path: string
}

export type TableTemplateAssetManifest = {
  templateId: string
  source: string
  assetCount: number
  totalSizeBytes: number
  assets: TableTemplateAssetManifestItem[]
}

export function isTemplateAssetReference(
  value: unknown,
): value is TableTemplateAssetReference {
  return Boolean(
    value
    && typeof value === 'object'
    && !Array.isArray(value)
    && Array.isArray((value as TableTemplateAssetReference).assetIds),
  )
}

export function collectTemplateAssetIds(
  records: Array<Record<string, TableTemplateRecordValue>>,
) {
  const ids = new Set<string>()
  for (const record of records) {
    for (const value of Object.values(record)) {
      if (!isTemplateAssetReference(value)) continue
      for (const assetId of value.assetIds) ids.add(assetId)
    }
  }
  return Array.from(ids)
}

export async function loadTemplateAssetManifest(
  manifestPath: string,
): Promise<TableTemplateAssetManifest> {
  const manifest = JSON.parse(
    await (await fetchAsset(manifestPath)).text(),
  ) as TableTemplateAssetManifest
  if (!Array.isArray(manifest.assets) || manifest.assetCount !== manifest.assets.length) {
    throw new Error(`Template asset manifest is invalid: ${manifestPath}`)
  }
  return manifest
}

/**
 * Stores each asset in the document as an attachment of `tableId` (a blob held in the file, through
 * `put`), and answers the attachment for each asset id.
 */
export async function storeTemplateAttachments({
  manifest,
  assetIds,
  put,
}: {
  manifest: TableTemplateAssetManifest
  assetIds: string[]
  put: (file: File) => Promise<DataAttachment>
}) {
  const manifestAssetById = new Map(manifest.assets.map((asset) => [asset.id, asset]))
  const stored = new Map<string, DataAttachment>()
  for (const assetId of assetIds) {
    const asset = manifestAssetById.get(assetId)
    if (!asset) throw new Error(`Template asset is missing from the manifest: ${assetId}`)
    const bytes = await (await fetchAsset(asset.path)).arrayBuffer()
    stored.set(assetId, await put(new File([bytes], asset.sourceName, { type: asset.mimeType })))
  }
  return stored
}

export function resolveTemplateRecordValue(
  value: TableTemplateRecordValue,
  attachmentByAssetId: Map<string, DataAttachment>,
): DataRecordValue {
  if (!isTemplateAssetReference(value)) return value
  return value.assetIds.map((assetId) => {
    const attachment = attachmentByAssetId.get(assetId)
    if (!attachment) throw new Error(`Template asset was not resolved: ${assetId}`)
    return attachment
  })
}
