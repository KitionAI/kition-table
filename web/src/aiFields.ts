import type { RunDataAIFieldBatchResponse, RunDataAIFieldCellResponse, TableHost } from './editor'
import { toBase64, type EngineTableDataSource } from './dataSource'
import type { GeneratePart, GenerateResult, HostServices } from './hostRequests'

/** A prompt part as the engine builds it: text, a blob held in the file (`hash`), or an image URL. */
type PreparedPart = { type: 'text'; text: string } | { type: 'image'; hash?: string; url?: string; mime?: string }

/** What `table.ai.prepare` answers: one model request per record to run, and the records it skips. */
type Prepared = {
  fieldId: number
  output: 'text' | 'image'
  /** The field's model preference, a hint for the host. */
  model?: string
  requests: Array<{
    recordId: number
    messages: Array<{ role: 'system' | 'user'; parts: PreparedPart[] }>
    /** For image fields: how many images (`count`) and their size, as the field's config says. */
    options?: { maxTokens?: number; temperature?: number; size?: string; count?: number; [key: string]: unknown }
  }>
  skipped: Array<{ recordId: number; reason?: string; message?: string }>
}

/** One record's outcome, for `table.ai.apply`. */
type Outcome =
  | { recordId: number; text: string }
  | { recordId: number; images: Array<{ mime: string; content: string }> }
  | { recordId: number; error: string }

type RunParams = {
  tableId: number
  fieldId: number
  record_ids: number[]
  /** The run's action and config, as the editor's AI controls give them; without them the field's own. */
  action?: string
  config?: unknown
  limit?: number
}

/** Model calls at once in a batch. */
const CONCURRENCY = 3

function aborted(signal?: AbortSignal) {
  if (signal?.aborted) throw new DOMException('AI generation canceled', 'AbortError')
}

async function mapLimited<T, R>(items: T[], limit: number, run: (item: T) => Promise<R>): Promise<R[]> {
  const results = new Array<R>(items.length)
  let next = 0
  const worker = async () => {
    while (next < items.length) {
      const index = next++
      results[index] = await run(items[index])
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker))
  return results
}

/**
 * AI fields over the engine and the host's model (Kit UI protocol section 7j): the engine builds
 * each record's prompt (`table.ai.prepare`) and writes the answers (`table.ai.apply`, one Undo
 * step); the host runs the model (`host/generate`, purpose `ai-field`) with the person's own
 * settings, so the kit never holds a key or picks the provider. Images held in the file go to the
 * host as bytes, external ones as their `http(s):` URL.
 */
export function engineAIHost(
  source: EngineTableDataSource,
  services: HostServices,
  /** The host refused a model call as `unavailable` (no model set up, or not signed in): its message. */
  onUnavailable?: (message: string) => void,
): NonNullable<TableHost['ai']> {
  const run = engineAIRun(source, services, onUnavailable)
  return {
    // The host runs the person's model; the kit names no model of its own.
    resolveRuntimeModel: async () => undefined,
    loadModelOptions: async () => [],
    async runCell({ tableId, recordId, fieldId, action, config }, options) {
      const answer = await run<RunDataAIFieldCellResponse | RunDataAIFieldBatchResponse>(
        { tableId, fieldId, record_ids: [recordId], action, config },
        options?.signal,
      )
      if ('record' in answer) return answer
      const item = answer.items.find((entry) => entry.record.id === recordId)
      if (!item) {
        const error = answer.errors?.find((entry) => entry.record_id === recordId)
        throw new Error(error?.message || 'AI generation failed')
      }
      return { record: item.record, field: answer.field, state: item.state }
    },
    runBatch: ({ tableId, fieldId, record_ids, limit, action, config }, options) =>
      run<RunDataAIFieldBatchResponse>({ tableId, fieldId, record_ids, limit, action, config }, options?.signal),
  }
}

/**
 * One AI field run over the engine and the host's model: prepares, generates, and applies it, and
 * answers what `table.ai.apply` answers. `engineAIHost` runs the editor's AI controls with it, and
 * a table from files (`fromFiles.ts`) fills its AI fields with it.
 */
export function engineAIRun(
  source: EngineTableDataSource,
  services: Pick<HostServices, 'generate'>,
  onUnavailable?: (message: string) => void,
): <T>(params: RunParams, signal?: AbortSignal) => Promise<T> {
  async function part(item: PreparedPart): Promise<GeneratePart | null> {
    if (item.type === 'text') return item
    if (item.hash && source.attachments) {
      const blob = await source.attachments.read({ documentId: 0, hash: item.hash })
      const bytes = blob.data.buffer.slice(blob.data.byteOffset, blob.data.byteOffset + blob.data.byteLength) as ArrayBuffer
      return { type: 'image', mime: item.mime || blob.mimeType || 'application/octet-stream', bytes }
    }
    if (item.url && /^https?:/i.test(item.url)) return { type: 'image', url: item.url }
    return null
  }

  async function generate(prepared: Prepared, request: Prepared['requests'][number], signal?: AbortSignal): Promise<Outcome> {
    try {
      aborted(signal)
      const messages = await Promise.all(
        request.messages.map(async (message) => ({
          role: message.role,
          parts: (await Promise.all(message.parts.map(part))).filter((item): item is GeneratePart => item != null),
        })),
      )
      aborted(signal)
      const ask = (): Promise<GenerateResult> =>
        services.generate({
          purpose: 'ai-field',
          output: prepared.output,
          ...(prepared.model ? { model: prepared.model } : {}),
          messages,
          ...(request.options ? { options: request.options } : {}),
        })
      if (prepared.output === 'image') {
        // One `host/generate` answers one image; an image field asks for `count` of them.
        const count = Math.max(1, Math.min(5, Number(request.options?.count) || 1))
        const images: Array<{ mime: string; content: string }> = []
        for (let index = 0; index < count; index += 1) {
          aborted(signal)
          const answer = await ask()
          if (answer.image) images.push({ mime: answer.image.mime, content: toBase64(new Uint8Array(answer.image.bytes)) })
        }
        if (images.length === 0) return { recordId: request.recordId, error: 'The model answered no image.' }
        return { recordId: request.recordId, images }
      }
      const answer = await ask()
      return { recordId: request.recordId, text: answer.text ?? '' }
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') throw error
      const message = error instanceof Error ? error.message : 'AI generation failed'
      if ((error as { data?: { reason?: unknown } } | null)?.data?.reason === 'unavailable') onUnavailable?.(message)
      return { recordId: request.recordId, error: message }
    }
  }

  /** Prepares, generates, and applies one run; answers what `table.ai.apply` answers. */
  async function run<T>(params: RunParams, signal?: AbortSignal): Promise<T> {
    const { tableId, fieldId, action } = params
    const prepared = await source.command<Prepared>('ai.prepare', {
      tableId,
      fieldId,
      record_ids: params.record_ids,
      ...(params.limit != null ? { limit: params.limit } : {}),
      action,
      config: params.config,
    })
    const results = await mapLimited(prepared.requests, CONCURRENCY, (request) => generate(prepared, request, signal))
    aborted(signal)
    return source.command<T>('ai.apply', { tableId, fieldId, action, results })
  }

  return run
}
