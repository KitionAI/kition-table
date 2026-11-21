import { ArrowLeftIcon as ArrowLeft, BriefcaseBusinessIcon as BriefcaseBusiness, DatabaseIcon as Database, EyeIcon as Eye, ListChecksIcon as ListChecks, LayoutDashboardIcon as LayoutDashboard, LoaderCircleIcon as LoaderCircle, PlusIcon as Plus, SparklesIcon as Sparkles, WorkflowIcon as Workflow } from '@kition/tessera/icons/react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  getBuiltinTableTemplates,
  type TableTemplateCategory,
  type TableTemplateDefinition,
  type TableTemplateRecordValue,
  type TableTemplateResource,
} from './tableTemplates'
import {
  isTemplateAssetReference,
  loadTemplateAssetManifest,
  templateAssetURL,
  type TableTemplateAssetManifestItem,
} from './templateAssets'
import { cn } from '../lib/cn'
import { TEMPLATE_LIBRARY_DIALOG_CLASSNAME, TemplateLibraryFrame } from './TemplateLibraryFrame'
import { Button, Modal, ModalContent, ModalDescription, ModalTitle } from '@kition/sdk/editor'

type TemplateLibraryDialogProps = {
  open: boolean
  busy?: boolean
  onOpenChange: (open: boolean) => void
  onSelect: (template?: TableTemplateDefinition) => Promise<boolean>
}

const TEMPLATE_CATEGORIES: Array<{
  id: TableTemplateCategory
  label: string
  icon: typeof Sparkles
}> = [
  { id: 'recommended', label: 'Recommended', icon: Sparkles },
  { id: 'ai-workflows', label: 'AI workflows', icon: Workflow },
  { id: 'business', label: 'Business', icon: BriefcaseBusiness },
  { id: 'projects', label: 'Projects', icon: ListChecks },
]

export function TemplateLibraryDialog({
  open,
  busy = false,
  onOpenChange,
  onSelect,
}: TemplateLibraryDialogProps) {
  const { t } = useTranslation('table')
  const templates = useMemo(() => getBuiltinTableTemplates(t), [t])
  const [pendingId, setPendingId] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState('')
  const [selectedTemplate, setSelectedTemplate] = useState<TableTemplateDefinition | null>(null)
  const [activeResourceId, setActiveResourceId] = useState('')
  const [activeCategory, setActiveCategory] = useState<TableTemplateCategory>('recommended')
  const [query, setQuery] = useState('')
  const disabled = busy || pendingId !== null
  const filteredTemplates = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase()
    return templates.filter((template) => {
      const categoryMatches = normalizedQuery
        ? true
        : (template.categories || ['recommended']).includes(activeCategory)
      const queryMatches = !normalizedQuery
        || `${template.title} ${template.description}`.toLowerCase().includes(normalizedQuery)
      return categoryMatches && queryMatches
    })
  }, [activeCategory, query, templates])

  const openTemplate = (template: TableTemplateDefinition) => {
    setSelectedTemplate(template)
    setActiveResourceId(template.snapshot.defaultResourceId)
    setErrorMessage('')
  }

  const closeTemplate = () => {
    if (disabled) return
    setSelectedTemplate(null)
    setActiveResourceId('')
    setErrorMessage('')
  }

  const createTemplate = async (template?: TableTemplateDefinition) => {
    setPendingId(template?.id || 'blank')
    setErrorMessage('')
    try {
      if (!await onSelect(template)) {
        setErrorMessage(t('templateLibrary.createFailed'))
      }
    } finally {
      setPendingId(null)
    }
  }

  return (
    <Modal open={open} onOpenChange={(nextOpen) => (disabled ? null : onOpenChange(nextOpen))}>
      <ModalContent
        size="none"
        className={TEMPLATE_LIBRARY_DIALOG_CLASSNAME}
        data-testid="table-template-library-dialog"
        disableAutoFocus
      >
        {selectedTemplate ? (
          <TemplateDetail
            template={selectedTemplate}
            activeResourceId={activeResourceId}
            disabled={disabled}
            errorMessage={errorMessage}
            onActiveResourceChange={setActiveResourceId}
            onBack={closeTemplate}
            onUse={() => void createTemplate(selectedTemplate)}
          />
        ) : (
          <TemplateLibraryFrame
            title="Template Center"
            description="Choose a template or a blank workspace to create a new table workspace."
            icon={Sparkles}
            query={query}
            onQueryChange={setQuery}
            categories={TEMPLATE_CATEGORIES}
            activeCategory={activeCategory}
            onCategoryChange={(category) => {
              setActiveCategory(category)
              setQuery('')
            }}
          >
            <section className="mx-auto max-w-7xl">
              <div className="mb-6 flex items-center justify-between gap-4">
                <div>
                  <h3 className="text-xl font-semibold text-foreground">
                    {query ? `Search results for “${query}”` : TEMPLATE_CATEGORIES.find((item) => item.id === activeCategory)?.label}
                  </h3>
                  <p className="mt-1 text-sm text-muted-foreground">Choose a focused starting point and customize it after creation.</p>
                </div>
                {errorMessage ? <span className="text-xs text-destructive" role="alert">{errorMessage}</span> : null}
              </div>
              <div className="grid grid-cols-1 gap-x-6 gap-y-8 sm:grid-cols-2 xl:grid-cols-3">
                {!query && activeCategory === 'recommended' ? (
                  <BlankTemplateCard
                    busy={pendingId === 'blank'}
                    disabled={disabled}
                    onCreate={() => void createTemplate()}
                  />
                ) : null}
                {filteredTemplates.map((template) => (
                  <TemplateGalleryCard
                    key={template.id}
                    template={template}
                    disabled={disabled}
                    busy={pendingId === template.id}
                    onPreview={() => openTemplate(template)}
                    onUse={() => void createTemplate(template)}
                  />
                ))}
              </div>
              {!filteredTemplates.length ? (
                <div className="mt-16 text-center text-sm text-muted-foreground">No templates match this search.</div>
              ) : null}
            </section>
          </TemplateLibraryFrame>
        )}
      </ModalContent>
    </Modal>
  )
}

function BlankTemplateCard({
  busy,
  disabled,
  onCreate,
}: {
  busy: boolean
  disabled: boolean
  onCreate: () => void
}) {
  const { t } = useTranslation('table')
  return (
    <article className="group min-w-0" data-testid="table-template-blank-card">
      <button
        type="button"
        className="relative block aspect-[16/9] w-full overflow-hidden rounded-xl border border-border bg-muted/25 transition hover:border-primary/35 hover:shadow-soft disabled:opacity-60"
        disabled={disabled}
        onClick={onCreate}
        data-testid="table-template-blank"
      >
        <span className="absolute inset-0 grid place-items-center">
          <span className="text-center">
            <span className="relative mx-auto block size-20">
              <span className="absolute inset-2 -rotate-6 rounded-xl border border-dashed border-primary/25 bg-background" />
              <span className="absolute inset-3 rotate-6 rounded-xl border border-border bg-background shadow-sm" />
              <span className="absolute inset-0 grid place-items-center"><Plus className="size-7 text-primary" /></span>
            </span>
            <span className="mt-2 block text-xs font-medium text-muted-foreground">{t('templateLibrary.blankWorkspace')}</span>
          </span>
        </span>
        {busy ? <span className="absolute inset-0 grid place-items-center bg-background/70"><LoaderCircle className="size-6 animate-spin text-primary" /></span> : null}
      </button>
      <h4 className="mt-3 text-sm font-semibold text-foreground">{t('templateLibrary.newWorkspace')}</h4>
      <p className="mt-1 truncate text-xs text-muted-foreground">Start from a clean data workspace.</p>
    </article>
  )
}

function TemplateGalleryCard({
  template,
  disabled,
  busy,
  onPreview,
  onUse,
}: {
  template: TableTemplateDefinition
  disabled: boolean
  busy: boolean
  onPreview: () => void
  onUse: () => void
}) {
  return (
    <article className="group min-w-0" data-testid={`table-template-card-${template.id}`}>
      <div className="relative">
        <button
          type="button"
          className="block w-full text-left disabled:opacity-60"
          disabled={disabled}
          onClick={onPreview}
          data-testid={`table-template-${template.id}`}
        >
          <TemplatePreviewCard template={template} busy={busy} />
        </button>
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 rounded-xl bg-foreground/0 opacity-0 transition group-hover:bg-foreground/10 group-hover:opacity-100 group-focus-within:opacity-100">
          <Button type="button" size="sm" variant="secondary" className="pointer-events-auto bg-background" disabled={disabled} onClick={onPreview}>
            Preview
          </Button>
          <Button variant="primary" type="button" size="sm" className="pointer-events-auto" disabled={disabled} onClick={onUse}>
            Use
          </Button>
        </div>
      </div>
      <div className="mt-3 flex min-w-0 items-center gap-2">
        <h4 className="min-w-0 flex-1 truncate text-sm font-semibold text-foreground">{template.title}</h4>
      </div>
      <p className="mt-1 line-clamp-2 min-h-8 text-xs leading-4 text-muted-foreground">{template.description}</p>
    </article>
  )
}

function TemplateDetail({
  template,
  activeResourceId,
  disabled,
  errorMessage,
  onActiveResourceChange,
  onBack,
  onUse,
}: {
  template: TableTemplateDefinition
  activeResourceId: string
  disabled: boolean
  errorMessage: string
  onActiveResourceChange: (resourceId: string) => void
  onBack: () => void
  onUse: () => void
}) {
  const { t } = useTranslation('table')
  const activeResource = template.snapshot.resources.find((resource) => resource.id === activeResourceId)
    || template.snapshot.resources[0]

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="table-template-detail">
      <div className="flex items-start gap-4 border-b px-6 py-4">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={disabled}
          onClick={onBack}
          data-testid="table-template-back"
        >
          <ArrowLeft className="size-4" />
          All templates
        </Button>
        <div className="min-w-0 flex-1">
          <ModalTitle className="truncate text-xl">{template.title}</ModalTitle>
          <ModalDescription className="mt-1 line-clamp-2">{template.description}</ModalDescription>
          <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
            <span className="inline-flex items-center gap-1"><Eye className="size-3.5" />Read-only preview</span>
            <span>{template.snapshot.resources.length} resources</span>
            <span>{template.snapshot.includeData ? 'Sample data included' : 'Structure only'}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-3">
          {errorMessage ? <span className="max-w-48 text-xs text-destructive" role="alert">{errorMessage}</span> : null}
          <Button
            variant="primary"
            size="md"
            type="button"
            disabled={disabled}
            onClick={onUse}
            data-testid="table-template-use"
          >
            {disabled ? <LoaderCircle className="size-4 animate-spin" /> : <Sparkles className="size-4" />}
            Use template
          </Button>
        </div>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[240px_minmax(0,1fr)] bg-muted/30 max-md:grid-cols-1">
        <aside className="min-h-0 overflow-y-auto border-r bg-background p-3 max-md:border-b max-md:border-r-0">
          <div className="mb-2 px-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            Included resources
          </div>
          <div className="space-y-1">
            {template.snapshot.resources.map((resource) => (
              <button
                key={resource.id}
                type="button"
                className={cn(
                  'flex w-full items-start gap-2 rounded-lg px-2.5 py-2 text-left transition-colors',
                  resource.id === activeResource?.id
                    ? 'bg-accent text-accent-foreground'
                    : 'text-foreground hover:bg-muted',
                )}
                onClick={() => onActiveResourceChange(resource.id)}
                data-testid={`table-template-resource-${resource.id}`}
              >
                <ResourceIcon resource={resource} />
                <span className="min-w-0">
                  <span className="block truncate text-sm font-medium">{resource.title}</span>
                  <span className="mt-0.5 block line-clamp-2 text-[11px] leading-4 text-muted-foreground">
                    {resource.description}
                  </span>
                </span>
              </button>
            ))}
          </div>
        </aside>

        <main className="min-h-0 overflow-auto p-5">
          <div className="mx-auto min-h-full max-w-5xl overflow-hidden rounded-xl border border-border bg-background shadow-soft">
            <TemplateResourcePreview template={template} resource={activeResource} />
          </div>
        </main>
      </div>
    </div>
  )
}

function ResourceIcon({ resource }: { resource: TableTemplateResource }) {
  if (resource.kind === 'dashboard') return <LayoutDashboard className="mt-0.5 size-4 shrink-0" />
  return <Database className="mt-0.5 size-4 shrink-0" />
}

function TemplateResourcePreview({
  template,
  resource,
}: {
  template: TableTemplateDefinition
  resource?: TableTemplateResource
}) {
  const { t } = useTranslation('table')
  const [assetById, setAssetById] = useState<Map<string, TableTemplateAssetManifestItem>>(() => new Map())
  const [assetManifestStatus, setAssetManifestStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle')

  useEffect(() => {
    const manifestPath = template.assetManifestPath
    if (!manifestPath) {
      setAssetById(new Map())
      setAssetManifestStatus('idle')
      return
    }

    let cancelled = false
    setAssetById(new Map())
    setAssetManifestStatus('loading')
    void loadTemplateAssetManifest(manifestPath)
      .then((manifest) => {
        if (cancelled) return
        setAssetById(new Map(manifest.assets.map((asset) => [asset.id, asset])))
        setAssetManifestStatus('ready')
      })
      .catch(() => {
        if (cancelled) return
        setAssetManifestStatus('error')
      })

    return () => {
      cancelled = true
    }
  }, [template.assetManifestPath])

  if (!resource) return null

  if (resource.kind === 'table') {
    const table = template.tables.find((candidate) => candidate.title === resource.tableTitle) || template.tables[0]
    if (!table) return null
    return (
      <div className="min-h-[520px] bg-background">
        <div className="flex items-center justify-between border-b bg-surface-soft px-4 py-3">
          <div>
            <h3 className="text-sm font-semibold text-foreground">{resource.title}</h3>
            <p className="text-xs text-muted-foreground">{resource.description}</p>
          </div>
          <span className="rounded-md border bg-background px-2 py-1 text-xs text-muted-foreground">{t('templateLibrary.readOnly')}</span>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[760px] border-collapse text-left text-xs">
            <thead className="bg-muted/50 text-muted-foreground">
              <tr>{table.fields.slice(0, 7).map((field) => <th key={field.title} className="border-b border-r px-3 py-2 font-medium last:border-r-0">{field.title}</th>)}</tr>
            </thead>
            <tbody>
              {table.records.slice(0, 10).map((record, rowIndex) => (
                <tr key={rowIndex} className="text-foreground">
                  {table.fields.slice(0, 7).map((field) => {
                    const value = record[field.title]
                    const isAsset = isTemplateAssetReference(value)
                    return (
                      <td
                        key={field.title}
                        className={cn(
                          'border-b border-r px-3 py-2.5 last:border-r-0',
                          isAsset ? 'min-w-36' : 'max-w-56 truncate',
                        )}
                      >
                        {renderPreviewValue(value, field.title, assetById, assetManifestStatus, t)}
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    )
  }

  if (resource.kind === 'dashboard') {
    const dashboard = template.dashboards?.find((candidate) => candidate.id === resource.id)
    return (
      <div className="min-h-[520px] bg-muted/30 p-5">
        <div className="mb-4 flex items-center gap-3 rounded-xl border bg-background px-4 py-3 shadow-sm">
          <span className="grid size-9 place-items-center rounded-lg bg-primary/10 text-primary">
            <LayoutDashboard className="size-4" />
          </span>
          <div>
            <h3 className="text-sm font-semibold text-foreground">{dashboard?.title || resource.title}</h3>
            <p className="text-xs text-muted-foreground">{t('templateLibrary.liveReporting')}</p>
          </div>
        </div>
        <div className="grid grid-cols-4 gap-3">
          {[['20', 'Total tasks'], ['7', 'Completed'], ['8', 'Important'], ['4', 'Not started']].map(([value, label]) => (
            <div key={label} className="rounded-xl border bg-background p-4 shadow-sm">
              <div className="text-2xl font-semibold text-primary">{value}</div>
              <div className="mt-1 text-xs text-muted-foreground">{label}</div>
            </div>
          ))}
        </div>
        <div className="mt-3 grid grid-cols-[0.8fr_1.2fr] gap-3">
          <div className="rounded-xl border bg-background p-4 shadow-sm">
            <div className="text-xs font-medium text-foreground">{t('templateLibrary.progressDistribution')}</div>
            <div className="mt-5 flex items-center justify-center">
              <div
                className="grid size-32 place-items-center rounded-full"
                style={{ background: 'conic-gradient(#5645d4 0 35%, #2a9d99 35% 80%, #dd5b00 80% 100%)' }}
              >
                <div className="size-20 rounded-full bg-background" />
              </div>
            </div>
          </div>
          <div className="rounded-xl border bg-background p-4 shadow-sm">
            <div className="text-xs font-medium text-foreground">{t('templateLibrary.tasksByAssignee')}</div>
            <div className="mt-5 flex h-32 items-end gap-2">
              {[38, 72, 48, 88, 55, 34, 62].map((height, index) => (
                <div
                  key={`${height}-${index}`}
                  className="flex-1 rounded-t bg-primary/70"
                  style={{ height: `${height}%` }}
                />
              ))}
            </div>
          </div>
        </div>
      </div>
    )
  }

  return null
}

function renderPreviewValue(
  value: TableTemplateRecordValue | undefined,
  fieldTitle: string,
  assetById: Map<string, TableTemplateAssetManifestItem>,
  assetManifestStatus: 'idle' | 'loading' | 'ready' | 'error',
  t: (key: string) => string,
) {
  if (isTemplateAssetReference(value)) {
    const assets = value.assetIds
      .map((assetId) => assetById.get(assetId))
      .filter((asset): asset is TableTemplateAssetManifestItem => Boolean(asset))
    if (assets.length) {
      return (
        <span className="flex items-center gap-1.5">
          {assets.slice(0, 2).map((asset, index) => (
            asset.mimeType.startsWith('image/') ? (
              <img
                key={asset.id}
                alt={`${fieldTitle} preview ${index + 1}`}
                className="h-12 w-16 shrink-0 rounded-md border bg-muted object-cover"
                loading="lazy"
                src={templateAssetURL(asset.path)}
                data-testid={`table-template-preview-asset-${asset.id}`}
              />
            ) : (
              <span key={asset.id} className="max-w-32 truncate text-xs text-muted-foreground">
                {asset.sourceName}
              </span>
            )
          ))}
          {assets.length > 2 ? <span className="text-[11px] text-muted-foreground">+{assets.length - 2}</span> : null}
        </span>
      )
    }
    if (assetManifestStatus === 'loading') {
      return <LoaderCircle className="size-4 animate-spin text-muted-foreground" aria-label={t('templateLibrary.previewLoading')} />
    }
    return <span className="text-xs text-muted-foreground">{t('templateLibrary.previewUnavailable')}</span>
  }
  if (Array.isArray(value)) return value.join(', ')
  if (typeof value === 'boolean') return value ? 'Yes' : 'No'
  return value == null ? '' : String(value)
}

function TemplatePreviewCard({
  template,
  busy,
}: {
  template: TableTemplateDefinition
  busy: boolean
}) {
  return (
    <span className="relative block aspect-[16/9] overflow-hidden rounded-xl border border-border bg-muted/40 transition group-hover:border-primary/35 group-hover:shadow-soft">
      <img
        alt=""
        className="size-full object-cover transition duration-300 group-hover:scale-[1.02]"
        src={templateAssetURL(template.coverImage)}
      />
      {busy ? (
        <span className="absolute inset-0 grid place-items-center bg-background/70 backdrop-blur-[1px]">
          <LoaderCircle className="size-6 animate-spin text-primary" />
        </span>
      ) : null}
    </span>
  )
}
