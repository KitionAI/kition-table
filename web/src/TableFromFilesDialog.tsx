import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, Modal, ModalContent, ModalDescription, ModalFooter, ModalHeader, ModalTitle, toast } from '@kition/sdk/editor'
import { FromFilesError, MAX_FILES, type FromFilesProgress, type FromFilesResult } from './fromFiles'

/** Makes a table from files and a request (`tableFromFiles`). */
export type FromFilesRun = (
  files: File[],
  request: string,
  options: { onProgress: (progress: FromFilesProgress) => void; signal: AbortSignal },
) => Promise<Pick<FromFilesResult, 'tableId' | 'fallback' | 'unfilled'>>

/** The same file chosen again: the same name, size and modification time. */
const sameFile = (a: File, b: File) => a.name === b.name && a.size === b.size && a.lastModified === b.lastModified

/** What choosing files left out: files past `MAX_FILES`, and files chosen already. */
type Left = { truncated: number; duplicates: number }

/**
 * A table from files: the person chooses files (or drops them here) and says what they want from
 * them; the table is made with one record a file, and its AI fields are filled while the dialog
 * shows how far it is. The same file chosen again (same name, size and modification time) is kept
 * once; files past `MAX_FILES` are left out; the dialog says both. Cancel, Escape, closing the
 * document for editing and the dialog going away all stop the run, which leaves no table behind.
 * After the run, a notice says when the plain design was used after the host refused the design,
 * and how many cells AI could not fill.
 */
export function TableFromFilesDialog({
  open,
  onOpenChange,
  run,
  onCompleted,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  run: FromFilesRun
  onCompleted: (tableId: number) => void
}) {
  const { t } = useTranslation('table')
  const [files, setFiles] = useState<File[]>([])
  const [left, setLeft] = useState<Left>({ truncated: 0, duplicates: 0 })
  const [request, setRequest] = useState('')
  const [progress, setProgress] = useState<FromFilesProgress | null>(null)
  const [error, setError] = useState('')
  const stop = useRef<AbortController | null>(null)
  const input = useRef<HTMLInputElement | null>(null)
  const running = progress != null

  // The dialog opens empty each time, and a run does not outlive it.
  useEffect(() => {
    if (open) return
    stop.current?.abort()
    stop.current = null
    setFiles([])
    setLeft({ truncated: 0, duplicates: 0 })
    setRequest('')
    setProgress(null)
    setError('')
  }, [open])
  // Nor the dialog itself (the document turned read-only, the bar went away).
  useEffect(() => () => stop.current?.abort(), [])

  function add(chosen: FileList | File[] | null) {
    if (!chosen) return
    setError('')
    const next = [...files]
    let duplicates = 0
    let truncated = 0
    for (const file of Array.from(chosen)) {
      if (next.some((kept) => sameFile(kept, file))) duplicates += 1
      else if (next.length >= MAX_FILES) truncated += 1
      else next.push(file)
    }
    setFiles(next)
    setLeft({ truncated, duplicates })
  }

  async function start() {
    const controller = new AbortController()
    stop.current = controller
    setError('')
    setProgress({ stage: 'design' })
    try {
      const made = await run(files, request, { onProgress: (step) => (controller.signal.aborted ? undefined : setProgress(step)), signal: controller.signal })
      if (made.fallback) toast.warning(t('kit.fromFiles.notices.plainDesign'))
      if (made.unfilled.length) {
        const count = made.unfilled.reduce((sum, field) => sum + field.records, 0)
        toast.warning(t('kit.fromFiles.notices.unfilled', { count }), {
          description: t('kit.fromFiles.notices.unfilledFields', { fields: made.unfilled.map((field) => field.field).join(', ') }),
        })
      }
      onCompleted(made.tableId)
      onOpenChange(false)
    } catch (failure) {
      if (failure instanceof DOMException && failure.name === 'AbortError') return
      setError(failure instanceof FromFilesError ? t(`kit.fromFiles.errors.${failure.code}`, failure.params) : t('kit.fromFiles.errors.unexpected'))
    } finally {
      if (stop.current === controller) stop.current = null
      setProgress(null)
    }
  }

  function said(step: FromFilesProgress) {
    if (step.stage === 'design') return t('kit.fromFiles.designing')
    if (step.stage === 'table') return t('kit.fromFiles.makingTable', { title: step.title })
    if (step.stage === 'records') return t('kit.fromFiles.addingRecords', { done: step.done, total: step.total })
    return t('kit.fromFiles.fillingFields', { done: step.done, total: step.total })
  }

  return (
    // Escape and the Cancel button do the same: while a run goes they stop it, otherwise they close.
    <Modal open={open} onOpenChange={(next) => (running ? (next ? null : stop.current?.abort()) : onOpenChange(next))}>
      <ModalContent size="sm" data-testid="table-from-files-dialog">
        <ModalHeader>
          <ModalTitle>{t('kit.fromFiles.title')}</ModalTitle>
          <ModalDescription>{t('kit.fromFiles.description')}</ModalDescription>
        </ModalHeader>
        <div className="flex flex-col gap-3 px-6 pb-2">
          <button
            type="button"
            className="flex min-h-20 flex-col items-center justify-center gap-1 rounded-xl border border-dashed border-border px-3 py-4 text-[13px] text-muted-foreground transition-colors hover:bg-muted disabled:opacity-60"
            disabled={running}
            onClick={() => input.current?.click()}
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => {
              event.preventDefault()
              if (!running) add(event.dataTransfer.files)
            }}
            data-testid="table-from-files-choose"
          >
            <span>{files.length ? t('kit.fromFiles.chosen', { count: files.length }) : t('kit.fromFiles.choose')}</span>
            {files.length ? <span className="max-w-full truncate text-xs">{files.map((file) => file.name).join(', ')}</span> : null}
          </button>
          <input
            ref={input}
            type="file"
            multiple
            className="hidden"
            onChange={(event) => {
              add(event.target.files)
              event.currentTarget.value = ''
            }}
            data-testid="table-from-files-input"
          />
          {left.truncated || left.duplicates ? (
            <p className="text-xs text-muted-foreground" role="status" data-testid="table-from-files-left-out">
              {[
                left.truncated ? t('kit.fromFiles.notices.truncated', { count: left.truncated, max: MAX_FILES }) : '',
                left.duplicates ? t('kit.fromFiles.notices.duplicates', { count: left.duplicates }) : '',
              ]
                .filter(Boolean)
                .join('. ')}
            </p>
          ) : null}
          <textarea
            className="min-h-20 w-full resize-none rounded-lg border border-border bg-background px-3 py-2 text-[13px] outline-none focus:border-ring disabled:opacity-60"
            placeholder={t('kit.fromFiles.requestPlaceholder')}
            aria-label={t('kit.fromFiles.request')}
            value={request}
            disabled={running}
            onChange={(event) => setRequest(event.target.value)}
            data-testid="table-from-files-request"
          />
          {progress ? (
            <p className="text-[13px] text-muted-foreground" role="status" data-testid="table-from-files-progress">
              {said(progress)}
            </p>
          ) : null}
          {error ? (
            <p className="text-[13px] text-destructive" role="alert" data-testid="table-from-files-error">
              {t('kit.fromFiles.failed')}: {error}
            </p>
          ) : null}
        </div>
        <ModalFooter>
          <Button size="md" variant="ghost" onClick={() => (running ? stop.current?.abort() : onOpenChange(false))} data-testid="table-from-files-cancel">
            {t('common:actions.cancel')}
          </Button>
          <Button variant="primary" size="md" disabled={running || files.length === 0} onClick={() => void start()} data-testid="table-from-files-start">
            {t('kit.fromFiles.start')}
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  )
}
