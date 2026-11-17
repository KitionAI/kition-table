import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowDownAZIcon as ArrowDownAZ, ArrowLeftFromLineIcon as ArrowLeftFromLine, ArrowRightFromLineIcon as ArrowRightFromLine, CopyIcon as Copy, DownloadIcon as Download, EyeOffIcon as EyeOff, FunnelIcon as Filter, GroupIcon as Group, PencilIcon as Pencil, PinOffIcon as PinOff, SendIcon as Send, SparklesIcon as Sparkles, TrashIcon as Trash2 } from '@kition/tessera-icons/react'
import type { DataField } from '../../types/dataDocument'
import { cn } from '../../../lib/cn'

type Position = { x: number; y: number }

export function TableColumnHeaderMenu({
  menu,
  field,
  busy,
  onClose,
  onEditField,
  onDuplicateField,
  onRunAIBatch,
  onDownloadAllFiles,
  onAddColumnToChat,
  onInsertField,
  onAddFilterForField,
  onAddSortForField,
  onSetGroupField,
  onFreezeUpTo,
  onHideField,
  onDeleteField,
}: {
  menu: Position | null
  field: DataField | null
  busy: boolean
  onClose: () => void
  onEditField: (field: DataField) => void
  onDuplicateField: (field: DataField) => void
  /** Runs the AI field over its records; without it (no model in the app), no run shows. */
  onRunAIBatch?: (field: DataField) => void
  onDownloadAllFiles: (field: DataField) => void
  onAddColumnToChat: (field: DataField) => void
  onInsertField: (side: 'before' | 'after', anchor: DataField) => void
  onAddFilterForField: (field: DataField) => void
  onAddSortForField: (field: DataField) => void
  onSetGroupField: (field: DataField) => void
  onFreezeUpTo: (field: DataField) => void
  onHideField: (field: DataField) => void
  onDeleteField: (field: DataField) => void
}) {
  const { t } = useTranslation('table')
  const rootRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    if (!menu) return
    function closeOnOutside(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) onClose()
    }
    // In the capture phase, before the editor's keys: the Escape that closes the menu is not the
    // editor's too (in Focus it would leave Focus as well).
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== 'Escape') return
      event.preventDefault()
      onClose()
    }
    window.addEventListener('mousedown', closeOnOutside)
    window.addEventListener('keydown', closeOnEscape, true)
    window.addEventListener('scroll', onClose, true)
    return () => {
      window.removeEventListener('mousedown', closeOnOutside)
      window.removeEventListener('keydown', closeOnEscape, true)
      window.removeEventListener('scroll', onClose, true)
    }
  }, [menu, onClose])

  if (!menu || !field) return null

  const aiEnabled = Boolean(field.ai_config?.enabled)
  const isAttachment = field.type === 'attachment'
  const isPrimary = Boolean(field.is_primary)

  const left = typeof window === 'undefined'
    ? menu.x
    : Math.min(menu.x, Math.max(12, window.innerWidth - 260))
  const top = typeof window === 'undefined'
    ? menu.y
    : Math.min(menu.y, Math.max(12, window.innerHeight - 460))

  function closeAndRun(action: () => void) {
    onClose()
    action()
  }

  return (
    <div
      ref={rootRef}
      className="data-inline-record-menu data-inline-column-menu"
      style={{ left, top }}
      onClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
      data-field-id={field.id}
      data-testid="ktable-column-header-menu"
    >
      <div className="data-inline-record-menu-section">
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onEditField(field))}
        >
          <Pencil className="size-4" />
          {t('columnMenu.editField')}
        </button>
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onDuplicateField(field))}
        >
          <Copy className="size-4" />
          {t('columnMenu.duplicateField')}
        </button>
      </div>

      {aiEnabled || isAttachment ? (
        <div className="data-inline-record-menu-section">
          {aiEnabled && onRunAIBatch ? (
            <button
              type="button"
              className="data-inline-record-menu-item"
              disabled={busy}
              onClick={() => closeAndRun(() => onRunAIBatch(field))}
            >
              <Sparkles className="size-4" />
              {t('columnMenu.generate')}
            </button>
          ) : null}
          {isAttachment ? (
            <button
              type="button"
              className="data-inline-record-menu-item"
              disabled={busy}
              onClick={() => closeAndRun(() => onDownloadAllFiles(field))}
            >
              <Download className="size-4" />
              {t('columnMenu.downloadAllFiles')}
            </button>
          ) : null}
          {aiEnabled ? (
            <button
              type="button"
              className="data-inline-record-menu-item"
              disabled={busy}
              onClick={() => closeAndRun(() => onAddColumnToChat(field))}
            >
              <Send className="size-4" />
              {t('columnMenu.addToChat')}
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="data-inline-record-menu-section">
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onInsertField('before', field))}
        >
          <ArrowLeftFromLine className="size-4" />
          {t('columnMenu.insertLeft')}
        </button>
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onInsertField('after', field))}
        >
          <ArrowRightFromLine className="size-4" />
          {t('columnMenu.insertRight')}
        </button>
      </div>

      <div className="data-inline-record-menu-section">
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onAddFilterForField(field))}
        >
          <Filter className="size-4" />
          {t('columnMenu.filterByField')}
        </button>
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onAddSortForField(field))}
        >
          <ArrowDownAZ className="size-4" />
          {t('columnMenu.sortByField')}
        </button>
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onSetGroupField(field))}
        >
          <Group className="size-4" />
          {t('columnMenu.groupByField')}
        </button>
        <button
          type="button"
          className="data-inline-record-menu-item"
          disabled={busy}
          onClick={() => closeAndRun(() => onFreezeUpTo(field))}
        >
          <PinOff className="size-4" />
          {t('columnMenu.freezeUpToField')}
        </button>
      </div>

      <div className="data-inline-record-menu-section">
        <button
          type="button"
          className={cn('data-inline-record-menu-item', isPrimary && 'is-disabled')}
          disabled={busy || isPrimary}
          onClick={() => closeAndRun(() => onHideField(field))}
        >
          <EyeOff className="size-4" />
          {t('columnMenu.hideField')}
        </button>
        <button
          type="button"
          className={cn('data-inline-record-menu-item is-danger', isPrimary && 'is-disabled')}
          disabled={busy || isPrimary}
          onClick={() => closeAndRun(() => onDeleteField(field))}
        >
          <Trash2 className="size-4" />
          {t('columnMenu.deleteField')}
        </button>
      </div>
    </div>
  )
}
