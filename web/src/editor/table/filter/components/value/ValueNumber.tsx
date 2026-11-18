import type { FilterValue } from '../../types'
import { TextInput } from '@kition/kit-editor'

export function ValueNumber({
  value,
  onChange,
}: {
  value: FilterValue
  onChange: (next: FilterValue) => void
}) {
  const text = typeof value === 'number' ? String(value) : ''
  return (
    <TextInput
      type="number"
      inputMode="decimal"
      value={text}
      placeholder="0"
      onChange={(e) => {
        const raw = e.target.value
        if (raw === '') return onChange(null)
        const n = Number(raw)
        onChange(Number.isFinite(n) ? n : null)
      }}
      className="data-inline-filter-value-input"
    />
  )
}
