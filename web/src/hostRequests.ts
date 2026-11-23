import type { HostConnection } from '@kition/sdk/editor'

/**
 * The host requests of Kit UI protocol 2.2 section 7j that the table asks (Kition platform
 * requests: the kit stays offline, so a model call or a host feature goes to the host). @kition/sdk/editor
 * types them in a later release; until then the editor sends them through the connection's peer
 * with these shapes.
 */

/** A part of a `host/generate` message: text, an image's bytes (transferred), or an `http(s):` image. */
export type GeneratePart =
  | { type: 'text'; text: string }
  | { type: 'image'; mime: string; bytes: ArrayBuffer }
  | { type: 'image'; url: string }

export type GenerateParams = {
  /**
   * What the kit generates for: an AI field's cell, or one of the kit's own purposes (UI-G4): only
   * those the kit uses, `table.schema` for the design of a table from files.
   */
  purpose: 'ai-field' | 'table.schema'
  output: 'text' | 'image'
  /** The document's preference, a hint only: the host runs the person's model settings. */
  model?: string
  messages: Array<{ role: 'system' | 'user'; parts: GeneratePart[] }>
  options?: { maxTokens?: number; temperature?: number; size?: string }
}

export type GenerateResult = {
  text?: string
  image?: { mime: string; bytes: ArrayBuffer }
  /** The model the host ran. */
  model?: string
  usage?: { inputTokens?: number; outputTokens?: number }
}

/** `host/action`'s answer: whether the host ran it, and a short text the kit may show. */
export type ActionResult = { done: boolean; message?: string }

/** The two requests on the peer, typed. */
type GenerationPeer = {
  request(method: 'host/generate', params: GenerateParams, options?: { transfer?: boolean }): Promise<GenerateResult>
  request(method: 'host/action', params: { id: string; params: Record<string, unknown> }): Promise<ActionResult>
}

/** The host actions the table offers a control for, when the host lists them (`action:<id>`). */
export type HostActionId = 'workflow.run' | 'design.fromRecord' | 'settings.open'

/** What this host serves of section 7j, and the requests to ask it. */
export type HostServices = {
  /** The host runs models for kits (capability `generate`). */
  generates: boolean
  /** The host serves the action `id` (capability `action:<id>`). */
  acts(id: HostActionId): boolean
  /**
   * Asks the host's model. The protocol has no way to cancel a request, so `signal` stops the
   * waiting: on abort the answer rejects with an `AbortError` and the host's late answer is dropped.
   */
  generate(params: GenerateParams, signal?: AbortSignal): Promise<GenerateResult>
  action(id: HostActionId, params: Record<string, unknown>): Promise<ActionResult>
}

export function hostServices(host: HostConnection): HostServices {
  const capabilities = new Set(host.init.capabilities ?? [])
  const peer = host.peer as unknown as GenerationPeer
  return {
    generates: capabilities.has('generate'),
    acts: (id) => capabilities.has(`action:${id}`),
    // Image bytes are transferred, not copied.
    generate: (params, signal) => {
      if (signal?.aborted) return Promise.reject(new DOMException('Canceled', 'AbortError'))
      const answer = peer.request('host/generate', params, { transfer: true })
      if (!signal) return answer
      return new Promise<GenerateResult>((resolve, reject) => {
        const abort = () => reject(new DOMException('Canceled', 'AbortError'))
        signal.addEventListener('abort', abort, { once: true })
        answer.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort))
      })
    },
    action: (id, params) => peer.request('host/action', { id, params }),
  }
}
