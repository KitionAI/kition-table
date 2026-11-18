import { useEffect, useRef, useState } from 'react'
import { useTableDataSource } from '../ports/TableDataSource'
import type { DataDocument, DataRecord } from '../../types/dataDocument'
import { parseMarker } from '../lib/tableEditorShared'

export function useTableEditorData({
  documentPath,
  markerContent,
  pinnedTableId,
  onRecordsLoaded,
}: {
  documentPath: string
  markerContent: string
  pinnedTableId?: number
  onRecordsLoaded?: () => void
}) {
  const dataSource = useTableDataSource()
  const pendingRecordsRef = useRef('')
  const [document, setDocument] = useState<DataDocument | null>(null)
  const [resolvedActiveTableId, setResolvedActiveTableId] = useState<number | null>(null)
  const [resolvedActiveViewId, setResolvedActiveViewId] = useState<number | null>(null)
  const [records, setRecords] = useState<DataRecord[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  async function loadDocument() {
    setLoading(true)
    setError('')
    try {
      const markerId = parseMarker(markerContent)
      let nextDocument: DataDocument | null = null
      if (markerId) {
        try {
          nextDocument = await dataSource.documents.get({ documentId: markerId })
        } catch {
          nextDocument = null
        }
      }
      if (!nextDocument) {
        try {
          nextDocument = await dataSource.documents.open({ path: documentPath })
        } catch {
          const result = await dataSource.documents.list({ query: documentPath })
          nextDocument = result.items.find((item) => item.path === documentPath) || null
        }
      }
      setDocument(nextDocument)
      setResolvedActiveTableId((current) => {
        const tables = nextDocument?.tables ?? []
        if (pinnedTableId != null) {
          return tables.some((table) => table.id === pinnedTableId) ? pinnedTableId : null
        }
        if (current && tables.some((table) => table.id === current)) {
          return current
        }
        return tables[0]?.id ?? null
      })
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to load table')
    } finally {
      setLoading(false)
    }
  }

  async function refreshDocument(preferredTableId = resolvedActiveTableId, preferredViewId = resolvedActiveViewId) {
    if (!document) return
    const nextDocument = await dataSource.documents.get({ documentId: document.id })
    setDocument(nextDocument)
    const tables = nextDocument?.tables ?? []
    const resolvedId = pinnedTableId != null
      ? (tables.some((table) => table.id === pinnedTableId) ? pinnedTableId : null)
      : (preferredTableId && tables.some((table) => table.id === preferredTableId)
          ? preferredTableId
          : tables[0]?.id ?? null)
    setResolvedActiveTableId(resolvedId)
    const nextTable = resolvedId
      ? tables.find((table) => table.id === resolvedId)
      : (pinnedTableId != null ? undefined : tables[0])
    setResolvedActiveViewId(
      preferredViewId && nextTable?.views?.some((view) => view.id === preferredViewId)
        ? preferredViewId
        : nextTable?.views?.[0]?.id ?? null,
    )
  }

  /** Reads every record of the active table (the kanban, calendar and gallery show them all). */
  async function loadRecords() {
    if (!document || !resolvedActiveTableId) {
      setRecords([])
      return
    }
    const readKey = `${document.id}:${resolvedActiveTableId}`
    if (pendingRecordsRef.current === readKey) return
    pendingRecordsRef.current = readKey
    try {
      const result = await dataSource.records.list({ documentId: document.id, tableId: resolvedActiveTableId })
      setRecords(result.items)
      onRecordsLoaded?.()
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : 'Failed to load records')
    } finally {
      if (pendingRecordsRef.current === readKey) {
        pendingRecordsRef.current = ''
      }
    }
  }

  useEffect(() => {
    void loadDocument()
  }, [documentPath, markerContent, pinnedTableId])

  return {
    document,
    setDocument,
    activeTableId: resolvedActiveTableId,
    setActiveTableId: setResolvedActiveTableId,
    activeViewId: resolvedActiveViewId,
    setActiveViewId: setResolvedActiveViewId,
    records,
    setRecords,
    loading,
    setLoading,
    error,
    setError,
    loadDocument,
    refreshDocument,
    loadRecords,
  }
}
