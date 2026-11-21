import { GripHorizontalIcon as GripHorizontal } from '@kition/tessera/icons/react'
import { BarChart, LineChart, PieChart } from '@kition/sdk/editor'

import type { DashboardQueryResult } from '../lib/dashboardQuery'
import type { DataDashboardWidget, DataRecordValue } from '../../types/dataDocument'

const chartColors = ['#5645d4', '#2a9d99', '#dd5b00', '#7b3ff2', '#1aae39', '#ff64c8']

export function DashboardWidgetCard({
  editing,
  result,
  widget,
}: {
  editing: boolean
  result: DashboardQueryResult
  widget: DataDashboardWidget
}) {
  return (
    <section
      className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-border bg-background shadow-sm"
      data-testid={`dashboard-widget-${widget.id}`}
    >
      <header
        className={editing
          ? 'dashboard-drag-handle flex cursor-move items-start gap-2 border-b border-border px-4 py-3'
          : 'flex items-start gap-2 border-b border-border px-4 py-3'}
      >
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold text-foreground">{widget.title}</h2>
          {widget.description ? (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">{widget.description}</p>
          ) : null}
        </div>
        {editing ? <GripHorizontal className="mt-0.5 size-4 shrink-0 text-muted-foreground" /> : null}
      </header>
      <div className="min-h-0 flex-1 p-4">
        <DashboardWidgetContent result={result} widget={widget} />
      </div>
    </section>
  )
}

function DashboardWidgetContent({
  result,
  widget,
}: {
  result: DashboardQueryResult
  widget: DataDashboardWidget
}) {
  if (result.kind === 'metric') {
    return (
      <div className="flex h-full flex-col justify-center">
        <div
          className="text-4xl font-semibold tracking-[-0.04em] text-foreground"
          style={{ color: widget.config?.color }}
        >
          {result.value.toLocaleString()}
        </div>
        <div className="mt-2 text-sm text-muted-foreground">
          {widget.config?.value_label || 'items'}
        </div>
      </div>
    )
  }

  if (result.kind === 'table') {
    return (
      <div className="h-full overflow-auto rounded-lg border border-border">
        <table className="w-full min-w-[520px] border-collapse text-left text-xs">
          <thead className="sticky top-0 z-10 bg-muted/80 backdrop-blur">
            <tr>
              {result.columns.map((column) => (
                <th key={column.name} className="border-b border-border px-3 py-2 font-medium text-foreground">
                  {column.title}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {result.rows.map((row) => (
              <tr key={row.id} className="border-b border-border last:border-b-0 hover:bg-muted/40">
                {result.columns.map((column) => (
                  <td key={column.name} className="max-w-64 truncate px-3 py-2 text-muted-foreground">
                    {formatValue(row.values[column.name] ?? null)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {result.rows.length === 0 ? (
          <div className="flex h-24 items-center justify-center text-sm text-muted-foreground">
            No matching records
          </div>
        ) : null}
      </div>
    )
  }

  if (result.points.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        No data to display
      </div>
    )
  }

  const color = widget.config?.color || '#5645d4'
  if (widget.type === 'pie') {
    return <PieChart label={widget.title} data={result.points} labelField="label" valueField="value" innerRadius={0.64} colors={chartColors} height="fill" />
  }

  if (widget.type === 'line') {
    return <LineChart label={widget.title} data={result.points} xField="label" yField="value" color={color} dots integers height="fill" />
  }

  return <BarChart label={widget.title} data={result.points} xField="label" yField="value" color={color} integers height="fill" />
}

function formatValue(value: DataRecordValue) {
  if (value == null || value === '') return '—'
  if (Array.isArray(value)) {
    return value.map((item) => {
      if (!item || typeof item !== 'object') return item
      if ('display' in item) return item.display
      if ('name' in item) return item.name
      return ''
    }).join(', ')
  }
  if (typeof value === 'object') return value.name
  if (typeof value === 'boolean') return value ? 'Yes' : 'No'
  return String(value)
}
