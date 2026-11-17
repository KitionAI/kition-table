import type { FilterValue } from '../../types'
import { TextInput } from '@kition/kit-editor'

export function ValueDate({
  value,
  onChange,
}: {
  value: FilterValue
  onChange: (next: FilterValue) => void
}) {
  const text = typeof value === 'string' ? value : ''
  return (
    <TextInput
      type="date"
      value={text}
      onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
      className="data-inline-filter-value-input"
    />
  )
}
