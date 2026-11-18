import { useEffect, useMemo, useRef, useState } from 'react'
import { I18nextProvider } from 'react-i18next'
import type { i18n } from 'i18next'
import { ConfirmProvider, EditorFrame, FrameChrome, ICON_STROKE, KitEditorProvider, TesseraIcons, createKitStrings, useKitEditor, useKitEditorState, useKitSession, type HostConnection } from '@kition/kit-editor'
import { TableDataSourceProvider, TableHostProvider } from './editor'
import { TableEditor } from './editor/TableEditor'
import { DashboardEditor } from './editor/DashboardEditor'
import { createWorkerTableDataSource } from './dataSource'
import { describe, engineManifest } from './engine'
import { hostServices } from './hostRequests'
import { CATALOGS } from './strings'
import { kitTableHost } from './tableHost'
import { exportTableThroughHost, type TableExportFormat } from './exportFile'
import { TableTabs } from './TableTabs'
import { resolveShown, shownOfView, type Shown } from './shown'

/** Undo and Redo keys belong to the table editor, which applies what they change; the shell's buttons still run them. */
const editorKeys = (command: string) => command === 'edit.undo' || command === 'edit.redo'

/** The frame has no command tabs: the table editor draws its own toolbar, and the bar under the frame's top lists the tables and dashboards. */
const NO_TABS = [] as const

/** The theme's `.dark` class follows the host's color scheme (kit-editor sets `data-color-scheme`). */
function useDarkClass() {
  useEffect(() => {
    const root = document.documentElement
    const apply = () => root.classList.toggle('dark', root.dataset.colorScheme === 'dark')
    apply()
    const observer = new MutationObserver(apply)
    observer.observe(root, { attributes: true, attributeFilter: ['data-color-scheme'] })
    return () => observer.disconnect()
  }, [])
}

export function App({ host, i18n }: { host: HostConnection; i18n: i18n }) {
  useDarkClass()
  const t = useMemo(() => createKitStrings(CATALOGS, host.init.locale), [host])
  const manifest = useMemo(engineManifest, [])
  const session = useKitSession(host, manifest, describe)
  const editor = useKitEditorState(session, host, t, 'table')
  const path = host.init.document.path
  const readOnly = host.readOnly === true || session.state?.readOnly != null

  // The data source reads the session through a ref, so the editor keeps one source.
  const latest = useRef(session)
  latest.current = session
  const source = useMemo(
    () =>
      createWorkerTableDataSource(
        {
          run: (command, params, refresh) => latest.current.run(command, params, null, { refresh }),
          invoke: async <T,>(method: string, ...args: unknown[]) => {
            const open = latest.current.doc
            if (!open) throw new Error('The table is not open.')
            return open.invoke<T>(method, ...args)
          },
          revision: () => latest.current.doc?.info.revision ?? 0,
        },
        path,
      ),
    [path],
  )
  // The document opened again from its file (the file changed on disk, Load the new version) is
  // a new engine: the editor reads it again.
  const doc = session.doc
  const opened = useRef(doc)
  useEffect(() => {
    if (!doc) return
    if (opened.current && opened.current !== doc) source.reopened(doc.info.revision)
    opened.current = doc
  }, [doc, source])
  const revision = session.doc?.info.revision
  useEffect(() => {
    if (revision != null) source.observe(revision)
  }, [source, revision])
  // Export: the engine answers the file's bytes, the host writes them as a new file.
  const exportTable = useMemo(
    () => (format: TableExportFormat, table: { id: number; title: string }) =>
      exportTableThroughHost(
        async (command, params) => {
          const answer = await latest.current.run(command, params, null, { refresh: 'none' })
          if (answer == null) throw new Error('The table is not open.')
          return answer
        },
        host,
        format,
        table,
        latest.current.state?.tables?.length ?? 1,
      ),
    [host],
  )
  const tableHost = useMemo(() => {
    const strings = i18n.getFixedT(null, 'table')
    return kitTableHost(
      host.init.locale,
      source,
      hostServices(host),
      {
        designFailed: strings('kit.designFailed'),
        workflowFailed: strings('kit.workflowFailed'),
        aiUnavailable: strings('kit.aiUnavailable'),
        aiSetUp: strings('kit.aiSetUp'),
        settingsFailed: strings('kit.settingsFailed'),
      },
      exportTable,
    )
  }, [host, i18n, source, exportTable])

  // The table or dashboard shown: the one chosen (first the host's `document.view`, then each
  // `document/reveal` and tab), while the document has it, or its first table.
  const tables = session.state?.tables ?? []
  const dashboards = session.state?.dashboards ?? []
  const [chosen, setChosen] = useState<Shown | null>(() => shownOfView(host.view))
  useEffect(
    () =>
      host.onReveal((view) => {
        const named = shownOfView(view)
        if (named) setChosen(named)
      }),
    [host],
  )
  const shown = resolveShown(chosen, tables, dashboards)

  return (
    <I18nextProvider i18n={i18n}>
      {/* The editor's providers wrap the shell too: the bar's tabs import and add tables. */}
      <ConfirmProvider>
        <TableDataSourceProvider value={source}>
          <TableHostProvider value={tableHost}>
            <KitEditorProvider editor={editor}>
              <EditorFrame
                tabs={NO_TABS}
                title={
                  <span className="table-kit-title" data-testid="table-title">
                    {host.init.document.name}
                  </span>
                }
                hiddenCommands={editorKeys}
                // The editor draws its own footer (the record count): the frame draws no status bar under it.
                statusBar={false}
              >
                {/* The tables and dashboards bar is the kit's own: Focus shows the table alone. */}
                {session.doc ? (
                  <FrameChrome>
                    <div className="table-kit-tabs">
                      <TableTabs tables={tables} dashboards={dashboards} shown={shown} onSelect={setChosen} source={source} readOnly={readOnly} documentPath={path} />
                    </div>
                  </FrameChrome>
                ) : null}
                <div className="table-kit-body" data-testid="table-body">
                  {session.doc && shown?.kind === 'dashboard' ? (
                    <DashboardEditor key={shown.id} dashboardId={shown.id} documentPath={path} topbarEnd={<DocumentState />} />
                  ) : session.doc ? (
                    <TableEditor documentPath={path} markerContent="" pinnedTableId={shown?.kind === 'table' ? shown.id : undefined} readOnly={readOnly} footerEnd={<DocumentState />} />
                  ) : session.failure ? (
                    <div className="kui-message" data-testid="table-failed">
                      {t('openFailed')}: {session.failure}
                    </div>
                  ) : (
                    <div className="kui-message kui-muted">{t('opening')}</div>
                  )}
                </div>
              </EditorFrame>
            </KitEditorProvider>
          </TableHostProvider>
        </TableDataSourceProvider>
      </ConfirmProvider>
    </I18nextProvider>
  )
}

/**
 * What the frame's status bar shows of the document (unsaved, edited by the agent or by the app),
 * in the editor's own footer: the frame draws no status bar under it.
 */
function DocumentState() {
  const { session, t, testId } = useKitEditor()
  return (
    <>
      {session.dirty ? <span data-testid={`${testId}-unsaved`}>{t('unsaved')}</span> : null}
      {session.editedBy?.by === 'agent' ? (
        <span className="kui-agent-edited" data-testid={`${testId}-agent-edited`}>
          <TesseraIcons.SparklesIcon size={12} strokeWidth={ICON_STROKE} aria-hidden="true" />
          {t('agentEdited')}
        </span>
      ) : session.editedBy?.by === 'system' ? (
        // The host's own change (Kit UI protocol 2.7): the app, named by its feature on hover.
        <span className="kui-host-edited" data-testid={`${testId}-host-edited`} title={session.editedBy.originLabel}>
          {t('hostEdited')}
        </span>
      ) : null}
    </>
  )
}
