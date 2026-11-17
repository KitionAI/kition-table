import {
  materializeDashboardSeeds,
  type DataDocument,
  type DataFieldSeed,
  type DataTable,
  type DataViewSeed,
  type TableDataSource,
} from '../editor'
import type { TableTemplateDefinition, TableTemplateTable } from './tableTemplates'
import {
  collectTemplateAssetIds,
  loadTemplateAssetManifest,
  resolveTemplateRecordValue,
  storeTemplateAttachments,
} from './templateAssets'

/** A blank table's fields and view, and the empty rows it starts with. */
export const BLANK_TABLE_FIELDS: DataFieldSeed[] = [
  { title: 'Title', type: 'text', primary: true, required: false },
  { title: 'Status', type: 'single_select', required: false, options: { choices: ['Not started', 'In progress', 'Done'] } },
  { title: 'Notes', type: 'long_text', required: false },
]
export const BLANK_TABLE_VIEWS: DataViewSeed[] = [{ title: 'Grid view', type: 'grid' }]
const BLANK_ROWS = 3

/** `title`, or `title 2`, `title 3`… when the document already has a table of that title. */
function unusedTitle(title: string, taken: Set<string>) {
  let candidate = title
  for (let next = 2; taken.has(candidate); next += 1) candidate = `${title} ${next}`
  taken.add(candidate)
  return candidate
}

function unusedId(id: string, taken: Set<string>) {
  let candidate = id
  for (let next = 2; taken.has(candidate); next += 1) candidate = `${id}-${next}`
  taken.add(candidate)
  return candidate
}

/** An AI field's prompt, its `{{Field title}}` placeholders naming the created fields. */
function resolvePrompt(prompt: string, fieldByTitle: Map<string, { name: string }>) {
  return prompt.replace(/{{\s*([^{}]+?)\s*}}/g, (placeholder, fieldTitle: string) => {
    const field = fieldByTitle.get(fieldTitle.trim())
    return field ? `{{${field.name}}}` : placeholder
  })
}

type Created = { template: TableTemplateTable; table: DataTable }

async function configureAIFields(source: TableDataSource, documentId: number, { template, table }: Created) {
  const fieldByTitle = new Map((table.fields || []).map((field) => [field.title, field]))
  for (const fieldTemplate of template.fields) {
    if (!fieldTemplate.aiConfig) continue
    const field = fieldByTitle.get(fieldTemplate.title)
    const sourceTitle = fieldTemplate.aiConfig.sourceFieldTitle
    const sourceField = sourceTitle ? fieldByTitle.get(sourceTitle) : undefined
    if (!field?.id || (sourceTitle && !sourceField?.id)) {
      throw new Error(`Template AI field could not be configured: ${fieldTemplate.title}`)
    }
    const { sourceFieldTitle: _sourceFieldTitle, ...config } = fieldTemplate.aiConfig
    const resolved = {
      ...config,
      ...('prompt' in config ? { prompt: resolvePrompt(config.prompt, fieldByTitle) } : {}),
      ...(sourceField?.id ? { source_field_id: sourceField.id } : {}),
    }
    await source.fields.update({
      documentId,
      tableId: table.id,
      fieldId: field.id,
      options: { ...(field.options || {}), ai_config: resolved },
    })
  }
}

async function configureViews(source: TableDataSource, documentId: number, { template, table }: Created) {
  const fieldByTitle = new Map((table.fields || []).map((field) => [field.title, field]))
  const viewByTitle = new Map((table.views || []).map((view) => [view.title, view]))
  for (const viewTemplate of template.views) {
    const view = viewByTitle.get(viewTemplate.title)
    if (!view?.id) continue
    const hidden = (viewTemplate.hiddenFieldTitles || []).map((title) => {
      const field = fieldByTitle.get(title)
      if (!field) throw new Error(`Template view field could not be configured: ${title}`)
      return field.name
    })
    if (viewTemplate.config || hidden.length) {
      await source.views.update({
        documentId,
        tableId: table.id,
        viewId: view.id,
        config: { ...(view.config || {}), ...(viewTemplate.config || {}), ...(hidden.length ? { hidden_field_names: hidden } : {}) },
      })
    }
    if (!viewTemplate.fieldLayouts?.length) continue
    for (const { fieldTitle, ...layout } of viewTemplate.fieldLayouts) {
      const field = fieldByTitle.get(fieldTitle)
      if (!field?.id) throw new Error(`Template view field could not be configured: ${fieldTitle}`)
      await source.views.patchField({ documentId, tableId: table.id, viewId: view.id, fieldId: field.id, ...layout })
    }
  }
}

async function seedRecords(source: TableDataSource, documentId: number, created: Created, template: TableTemplateDefinition) {
  const { template: tableTemplate, table } = created
  const fieldNameByTitle = new Map((table.fields || []).map((field) => [field.title, field.name]))
  const assetIds = collectTemplateAssetIds(tableTemplate.records)
  if (assetIds.length && !template.assetManifestPath) throw new Error(`Template asset manifest was not configured: ${template.id}`)
  // The template's images go into the file as blobs, so the table carries them wherever it goes.
  const attachments =
    assetIds.length && template.assetManifestPath && source.attachments
      ? await storeTemplateAttachments({
          manifest: await loadTemplateAssetManifest(template.assetManifestPath),
          assetIds,
          put: (file) => source.attachments!.put({ documentId, tableId: table.id, file }),
        })
      : new Map()
  for (const record of tableTemplate.records) {
    const values = Object.fromEntries(
      Object.entries(record).flatMap(([title, value]) => {
        const name = fieldNameByTitle.get(title)
        if (!name) return []
        if (!source.attachments && typeof value === 'object' && value && 'assetIds' in value) return []
        return [[name, resolveTemplateRecordValue(value, attachments)]]
      }),
    )
    await source.records.create({ documentId, tableId: table.id, values })
  }
}

/**
 * Adds a starter table to the open document: the template's tables (their fields, views, AI field
 * configuration, and sample records, with the images stored in the file) and its dashboards, or,
 * without a template, a blank table with three empty rows. Answers the id of the table to show.
 */
export async function addTemplateToDocument(source: TableDataSource, document: DataDocument, template?: TableTemplateDefinition): Promise<number> {
  const documentId = document.id
  const titles = new Set((document.tables || []).map((table) => table.title))
  if (!template) {
    const table = await source.tables.create({ documentId, title: unusedTitle('Table', titles), fields: BLANK_TABLE_FIELDS, views: BLANK_TABLE_VIEWS })
    for (let row = 0; row < BLANK_ROWS; row += 1) await source.records.create({ documentId, tableId: table.id, values: {} })
    return table.id
  }

  const created: Created[] = []
  for (const tableTemplate of template.tables) {
    const { records: _records, fields, views, title, description } = tableTemplate
    const table = await source.tables.create({
      documentId,
      title: unusedTitle(title, titles),
      description,
      fields: fields.map(({ aiConfig: _aiConfig, ...field }) => ({ ...field, required: false })),
      views: views.map(({ hiddenFieldTitles: _hidden, fieldLayouts: _layouts, ...view }) => view),
    })
    created.push({ template: tableTemplate, table })
  }
  for (const item of created) {
    await configureAIFields(source, documentId, item)
    await configureViews(source, documentId, item)
    if (template.snapshot.includeData) await seedRecords(source, documentId, item, template)
  }

  if (template.dashboards?.length) {
    // Seeds name their source table by the template's title, which the created table may not keep.
    const seeded = { ...document, tables: created.map(({ template: tableTemplate, table }) => ({ ...table, title: tableTemplate.title })) }
    const taken = new Set((await source.dashboards.list({ documentId })).items.map((dashboard) => dashboard.id))
    for (const dashboard of materializeDashboardSeeds(seeded, template.dashboards)) {
      const id = unusedId(dashboard.id, taken)
      await source.dashboards.upsert({ documentId, ...dashboard, id, dashboardId: id })
    }
  }
  return created[0].table.id
}
