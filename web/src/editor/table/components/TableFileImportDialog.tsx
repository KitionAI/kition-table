import { useEffect, useState } from 'react'
import { TriangleAlertIcon as AlertTriangle, FileSpreadsheetIcon as FileSpreadsheet, LoaderCircleIcon as LoaderCircle } from '@kition/tessera-icons/react'
import { useTranslation } from 'react-i18next'
import type {
  DataImportField,
  DataImportResult,
  DataImportWriteMode,
} from '../../types/dataImports'
import {
  useTableHost,
  type PreparedTableFileImport,
  type TableFileImportTarget,
} from '../ports/TableHost'
import { Button, Modal, ModalBody, ModalCloseButton, ModalContent, ModalFooter, ModalHeader, ModalTitle, Select, TextInput } from '@kition/kit-editor'

const IMPORT_FIELD_TYPES: DataImportField['type'][] = [
  'text',
  'long_text',
  'number',
  'date',
  'datetime',
  'single_select',
  'multi_select',
  'checkbox',
  'url',
]

function displayCellValue(value: string | number | boolean | null) {
  if (value === null) return ''
  return String(value)
}

function defaultTableTitle(file: File | null) {
  return file?.name.replace(/\.(?:csv|tsv|xlsx)$/i, '').trim() || 'Imported table'
}

export function TableFileImportDialog({
  file,
  onCompleted,
  onOpenChange,
  open,
  target,
}: {
  file: File | null
  onCompleted: (result: DataImportResult) => void | Promise<void>
  onOpenChange: (open: boolean) => void
  open: boolean
  target: Extract<TableFileImportTarget, { kind: 'existing_table' }> | { kind: 'new_table'; documentId: number }
}) {
  const { t } = useTranslation('table')
  const { fileImport } = useTableHost()
  const [prepared, setPrepared] = useState<PreparedTableFileImport | null>(null)
  const [fieldTypes, setFieldTypes] = useState<DataImportField['type'][]>([])
  const [writeMode, setWriteMode] = useState<DataImportWriteMode>('append')
  const [selectedSheet, setSelectedSheet] = useState('')
  const [tableTitle, setTableTitle] = useState('')
  const [loading, setLoading] = useState(false)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (!open || !file || !fileImport) return
    let active = true
    setLoading(true)
    setPrepared(null)
    setError('')
    void fileImport.prepare(file, { sheet: selectedSheet || undefined })
      .then((next) => {
        if (!active) return
        setPrepared(next)
        setFieldTypes(next.preview.fields.map((field) => field.type))
        if (!selectedSheet && next.preview.selected_sheet) {
          setSelectedSheet(next.preview.selected_sheet)
        }
      })
      .catch((requestError) => {
        if (active) setError(requestError instanceof Error ? requestError.message : t('fileImport.previewFailed'))
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [file, fileImport, open, selectedSheet, t])

  useEffect(() => {
    if (!open || target.kind !== 'new_table') return
    setTableTitle(defaultTableTitle(file))
  }, [file, open, target.kind])

  useEffect(() => {
    if (open) return
    setPrepared(null)
    setFieldTypes([])
    setSelectedSheet('')
    setError('')
  }, [open])

  async function handleImport() {
    if (!prepared || !fileImport) return
    setRunning(true)
    setError('')
    try {
      const completed = await fileImport.execute({
        fieldTypes,
        prepared,
        target: target.kind === 'existing_table'
          ? target
          : { kind: 'new_table', documentId: target.documentId, tableTitle: tableTitle.trim() || defaultTableTitle(file) },
        writeMode,
      })
      await onCompleted(completed.result)
      onOpenChange(false)
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : t('fileImport.importFailed'))
    } finally {
      setRunning(false)
    }
  }

  const preview = prepared?.preview

  return (
    <Modal open={open} onOpenChange={(nextOpen) => {
      if (!nextOpen && running) return
      onOpenChange(nextOpen)
    }}>
      <ModalContent size="3xl" fixed hideClose>
        <ModalHeader>
          <ModalTitle>{t('fileImport.title')}</ModalTitle>
        </ModalHeader>
        <ModalBody className="gap-5">
          <div className="flex items-center gap-3 rounded-xl border bg-card p-4 shadow-soft">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <FileSpreadsheet className="size-5" />
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">{file?.name}</p>
              <p className="text-xs text-muted-foreground">
                {file ? t('fileImport.fileSize', { size: (file.size / 1024).toFixed(1) }) : ''}
              </p>
            </div>
          </div>

          {loading ? (
            <div className="flex min-h-56 items-center justify-center gap-2 text-sm text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" />
              {t('fileImport.preparingPreview')}
            </div>
          ) : null}

          {error ? (
            <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
              <AlertTriangle className="mt-0.5 size-4 shrink-0" />
              <span>{error}</span>
            </div>
          ) : null}

          {preview && !loading ? (
            <>
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                {[
                  [t('fileImport.format'), preview.format.toUpperCase()],
                  [t('fileImport.rows'), preview.row_count.toLocaleString()],
                  [t('fileImport.fields'), preview.field_count.toLocaleString()],
                  [t('fileImport.sheet'), preview.selected_sheet || t('fileImport.notApplicable')],
                ].map(([label, value]) => (
                  <div key={label} className="rounded-xl border bg-card p-3">
                    <p className="text-xs text-muted-foreground">{label}</p>
                    <p className="mt-1 truncate text-sm font-medium">{value}</p>
                  </div>
                ))}
              </div>

              {preview.sheets.length > 1 ? (
                <label className="grid gap-1.5 text-sm sm:max-w-xs">
                  <span className="font-medium">{t('fileImport.selectSheet')}</span>
                  <Select size="lg" value={selectedSheet} onChange={(event) => setSelectedSheet(event.target.value)} disabled={running}>
                    {preview.sheets.filter((sheet) => !sheet.hidden).map((sheet) => (
                      <option key={sheet.name} value={sheet.name}>
                        {sheet.name} ({sheet.row_count.toLocaleString()} {t('fileImport.rows').toLocaleLowerCase()})
                      </option>
                    ))}
                  </Select>
                </label>
              ) : null}

              {target.kind === 'new_table' ? (
                <label className="grid gap-1.5 text-sm sm:max-w-md">
                  <span className="font-medium">{t('fileImport.newTableTitle')}</span>
                  <TextInput
                    size="lg"
                    value={tableTitle}
                    onChange={(event) => setTableTitle(event.target.value)}
                    disabled={running}
                    placeholder={t('fileImport.newTableTitlePlaceholder')}
                    data-testid="table-import-new-title"
                  />
                </label>
              ) : null}

              {target.kind === 'existing_table' ? (
                <label className="grid gap-1.5 text-sm sm:max-w-xs">
                  <span className="font-medium">{t('fileImport.writeMode')}</span>
                  <Select size="lg" value={writeMode} onChange={(event) => setWriteMode(event.target.value as DataImportWriteMode)} disabled={running}>
                    <option value="append">{t('fileImport.append')}</option>
                    <option value="replace">{t('fileImport.replace')}</option>
                  </Select>
                </label>
              ) : null}

              <section className="grid gap-2">
                <h3 className="text-sm font-medium">{t('fileImport.fieldMapping')}</h3>
                <div className="max-h-64 overflow-auto rounded-xl border">
                  <table className="w-full border-collapse text-sm">
                    <thead className="sticky top-0 bg-muted/95 text-left text-xs text-muted-foreground backdrop-blur">
                      <tr>
                        <th className="border-b px-3 py-2 font-medium">{t('fileImport.fieldName')}</th>
                        <th className="w-48 border-b px-3 py-2 font-medium">{t('fileImport.fieldType')}</th>
                        <th className="border-b px-3 py-2 font-medium">{t('fileImport.sample')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {preview.fields.map((field, index) => (
                        <tr key={`${field.index}-${field.title}`} className="border-b last:border-b-0">
                          <td className="px-3 py-2 font-medium">{field.title}</td>
                          <td className="px-3 py-2">
                            <Select
                              value={fieldTypes[index] || field.type}
                              onChange={(event) => setFieldTypes((current) => current.map((value, currentIndex) => (
                                currentIndex === index ? event.target.value as DataImportField['type'] : value
                              )))}
                              disabled={running}
                              aria-label={t('fileImport.fieldTypeFor', { field: field.title })}
                            >
                              {IMPORT_FIELD_TYPES.map((type) => (
                                <option key={type} value={type}>{t(`fileImport.types.${type}`)}</option>
                              ))}
                            </Select>
                          </td>
                          <td className="max-w-80 truncate px-3 py-2 text-muted-foreground">
                            {field.sample_values.slice(0, 3).map(displayCellValue).filter(Boolean).join(', ') || '—'}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>

              {preview.warnings.length ? (
                <section className="grid gap-2">
                  <h3 className="text-sm font-medium">{t('fileImport.warnings')}</h3>
                  <div className="grid gap-2">
                    {preview.warnings.map((warning, index) => (
                      <div key={`${warning.code}-${index}`} className="flex items-start gap-2 rounded-lg border bg-muted/30 p-3 text-xs text-muted-foreground">
                        <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-amber-600" />
                        <span>{warning.message}</span>
                      </div>
                    ))}
                  </div>
                </section>
              ) : null}

            </>
          ) : null}
        </ModalBody>
        <ModalFooter>
          <Button size="md" variant="ghost" disabled={running} onClick={() => onOpenChange(false)}>
            {t('fileImport.cancel')}
          </Button>
          <Button variant="primary" size="md" onClick={() => void handleImport()} disabled={!prepared || loading || running || Boolean(error) || (target.kind === 'new_table' && !tableTitle.trim())}>
            {running ? <LoaderCircle className="size-4 animate-spin" /> : null}
            {running ? t('fileImport.importing') : t('fileImport.import')}
          </Button>
        </ModalFooter>
        <ModalCloseButton disabled={running} />
      </ModalContent>
    </Modal>
  )
}
