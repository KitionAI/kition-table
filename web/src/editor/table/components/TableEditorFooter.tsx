import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { DataField } from '../../types/dataDocument'

type NumericSummary = {
  field: DataField
  sum: number
  average: number
}

export function TableEditorFooter({
  gridTotalRows,
  canUseVirtualGrid,
  recordOffset,
  loadedRecordCount,
  recordTotal,
  numericSummaries,
  selectedRecordCount,
  end,
}: {
  gridTotalRows: number
  canUseVirtualGrid: boolean
  recordOffset: number
  loadedRecordCount: number
  recordTotal: number
  numericSummaries: NumericSummary[]
  selectedRecordCount: number
  /** After the hint: the host's document state, when it draws no status bar. */
  end?: ReactNode
}) {
  const { t } = useTranslation('table')
  const visibleRangeLabel =
    canUseVirtualGrid && recordTotal > loadedRecordCount
      ? ` · ${t('footer.showingRange', {
          start: (recordOffset + 1).toLocaleString(),
          end: Math.min(recordOffset + loadedRecordCount, recordTotal).toLocaleString(),
        })}`
      : ''

  return (
    <div className="data-inline-footer">
      <span>
        {t('footer.recordsCount', { count: gridTotalRows })}{visibleRangeLabel}
      </span>
      {numericSummaries.length ? (
        <div className="data-inline-aggregates">
          {numericSummaries.map((item) => (
            <span key={item.field.id}>
              {t('footer.aggregate', {
                title: item.field.title,
                sum: item.sum.toLocaleString(),
                avg: item.average.toLocaleString(undefined, {
                  maximumFractionDigits: 2,
                }),
              })}
            </span>
          ))}
        </div>
      ) : null}
      {end ? (
        <span className="flex items-center gap-3">
          <span>{selectedRecordCount ? t('footer.selectedCount', { count: selectedRecordCount }) : t('footer.editHint')}</span>
          {end}
        </span>
      ) : selectedRecordCount ? (
        <span>{t('footer.selectedCount', { count: selectedRecordCount })}</span>
      ) : (
        <span>{t('footer.editHint')}</span>
      )}
    </div>
  )
}
