import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { NumericSummary } from '../hooks/useGridSummary'

export function TableEditorFooter({
  shownCount,
  numericSummaries,
  selectedRecordCount,
  end,
}: {
  /** How many records the view shows. */
  shownCount: number
  numericSummaries: NumericSummary[]
  selectedRecordCount: number
  /** After the hint: the host's document state, when it draws no status bar. */
  end?: ReactNode
}) {
  const { t } = useTranslation('table')

  return (
    <div className="data-inline-footer">
      <span>
        {t('footer.recordsCount', { count: shownCount })}
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
