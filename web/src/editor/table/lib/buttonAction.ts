import type { DataField } from '../../types/dataDocument'

/** What a button field does: open a URL, or run one of the app's workflows on the record. */
export type ButtonAction = { kind: 'open_url'; url?: string } | { kind: 'run_workflow'; workflow_id?: number }

/** A button field's label and action, from its options. */
export function buttonConfig(field: DataField, fallbackLabel: string): { label: string; action: ButtonAction } {
  const options = (field.options ?? {}) as Record<string, unknown>
  const label = typeof options.label === 'string' && options.label ? options.label : field.title || fallbackLabel
  const action = (options.action ?? {}) as Record<string, unknown>
  const kind = action.kind === 'run_workflow' ? 'run_workflow' : 'open_url'
  if (kind === 'open_url') {
    return { label, action: { kind: 'open_url', url: typeof action.url === 'string' ? action.url : undefined } }
  }
  return { label, action: { kind: 'run_workflow', workflow_id: typeof action.workflow_id === 'number' ? action.workflow_id : undefined } }
}
