import { toast } from '@kition/sdk/editor'
import type { TableHost } from './editor'
import { engineAIHost } from './aiFields'
import type { EngineTableDataSource } from './dataSource'
import { engineFileImport } from './fileImport'
import type { HostServices } from './hostRequests'

const SELF_CONTAINED_URL = /^(https?:|data:|blob:)/i

/** Shows a host action's refusal, or its message, as a toast. */
async function act(run: () => Promise<{ done: boolean; message?: string }>, failed: string) {
  try {
    const answer = await run()
    if (answer.message) toast(answer.done ? 'success' : 'error', answer.message)
    return answer
  } catch (error) {
    toast('error', failed, error instanceof Error ? error.message : undefined)
    return { done: false }
  }
}

/** The host's own messages, translated (`kit.*` of the `table` strings). */
export type HostMessages = { designFailed: string; workflowFailed: string; aiUnavailable: string; aiSetUp: string; settingsFailed: string }

/**
 * An AI field run the host refused as `unavailable` (no model set up, or not signed in): one
 * notice for the run, with the host's reason and, when the host serves `action:settings.open`, a
 * button that asks it to open where the person sets up AI. The AI field controls and a table from
 * files show the same notice.
 */
export function aiUnavailable(services: Pick<HostServices, 'acts' | 'action'>, messages: HostMessages) {
  return (reason: string) => {
    const setUp = services.acts('settings.open')
      ? { label: messages.aiSetUp, onClick: () => void act(() => services.action('settings.open', { section: 'ai' }), messages.settingsFailed) }
      : undefined
    toast.error(messages.aiUnavailable, { id: 'table-ai-unavailable', description: reason, ...(setUp ? { action: setUp } : {}) })
  }
}

/**
 * What the table editor asks of its app, in a kit: toasts through @kition/sdk/editor; attachments by URL only
 * where a URL loads by itself (`http(s):`, `data:`, `blob:`), while attachments held in the file
 * go through the data source (`attachment.get`, `attachment.add`); CSV, TSV, and XLSX import
 * through the engine. What needs the host is offered only when the host lists it (Kit UI protocol
 * section 7j): AI field runs with `generate`, design from a record with `action:design.fromRecord`,
 * and a button's workflow with `action:workflow.run`. Without them the editor hides those controls.
 * Export writes a new file beside the document through the host (`exportTable`).
 */
export function kitTableHost(
  locale: string,
  source: EngineTableDataSource,
  services: HostServices,
  messages: HostMessages,
  exportTable: NonNullable<TableHost['exportTable']>,
): TableHost {
  return {
    exportTable,
    resolveFileURL: (url) => (SELF_CONTAINED_URL.test(url) ? url : ''),
    notify: {
      success: (message, options) => toast('success', message, options?.description),
      error: (message, options) => toast('error', message, options?.description),
      dismiss: () => undefined,
    },
    locale: () => locale,
    fileImport: engineFileImport(source),
    ...(services.generates ? { ai: engineAIHost(source, services, aiUnavailable(services, messages)) } : {}),
    ...(services.acts('design.fromRecord')
      ? {
          requestDesignFromRecord: ({ title, fields, record }) => {
            const linked = record ? { tableId: record.tableId, recordId: record.recordId, field: record.field } : undefined
            void act(() => services.action('design.fromRecord', { title, fields, ...(linked ? { record: linked } : {}) }), messages.designFailed)
          },
        }
      : {}),
    ...(services.acts('workflow.run')
      ? {
          runWorkflow: ({ workflowId, tableId, recordId }) =>
            act(() => services.action('workflow.run', { workflowId, tableId, recordId }), messages.workflowFailed),
        }
      : {}),
  }
}
