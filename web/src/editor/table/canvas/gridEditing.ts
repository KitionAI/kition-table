/**
 * Editing on the new grid (grid plan F5, F9, F10, F11, F12, F13, F18, F19, F21; card tg4b): the
 * cell editors, the clipboard, clearing, the drags (`gestures.ts`), the cells' clickable parts,
 * attachment pictures and the AI fields' overlay, over a `GridInteraction` and a `GridData`.
 *
 * - **Typing (platform UI-K6).** A textarea sits on the active cell and holds the focus whenever
 *   the grid has it, invisible until a cell is edited. Keys go through it to the grid
 *   (`GridInteraction.handleKey`) while nothing is edited; a typed character or Enter, F2 or a
 *   double click opens the cell's editor. A text cell's editor is the textarea itself, shown on
 *   the cell, so an input method's composition starts and stays in one focused element.
 * - **Editors by field type.** Text, long text, numbers, links (urls) and other text fields edit
 *   in the textarea (Enter commits and moves down, Shift+Enter a new line in long text, Tab
 *   commits and moves sideways, Escape cancels). Dates and choices open a popup the editor draws
 *   (`onPopup`: the date editor and the choice list, `GridCellEditors.tsx`). A checkbox toggles
 *   on Enter or a click; a rating takes a typed digit or a click on its symbol.
 * - **Clipboard.** Copy and cut put the selection's cells on the clipboard as tab-separated text.
 *   Cmd+C and Cmd+X on a Mac, Ctrl+C and Ctrl+X elsewhere (`mac`, as the kit editor reports the
 *   platform) are the grid's own keys: they write through `navigator.clipboard` in place of the
 *   browser's copy (WebKit enables its copy only for selected text, and the grid's textarea holds
 *   none); a copy event from elsewhere (a menu) sets the clipboard on the event. Paste writes the text from the active cell on, past the last record as new records, in one
 *   `transact`; Delete and Backspace clear the selection's cells in one `transact`.
 * - **Pictures.** The view decodes the pictures the document holds (and `data:` urls) through its
 *   source; the pictures at web addresses the bands name (and `blob:` urls, or what
 *   `resolveFileURL` makes of any other url) are loaded here, as the old grid loaded them, and
 *   drawn where the engine placed them. A click on one asks the editor to preview the cell's
 *   attachments.
 * - **Attachments.** When the data can upload (`GridData.upload`), the active attachment cell
 *   shows an add button at its left (as the old grid did): a click on it picks files, and files
 *   pasted on an attachment cell are added the same way; each file is uploaded and the cell's
 *   attachments get them in one `records.update`.
 * - **AI fields.** A hovered AI cell shows a regenerate button at its top right; cells being
 *   generated (`setGenerating`) show a spinner over them.
 *
 * Every write goes through the engine's commands and then `onWritten` with what it changed (by
 * default the view arranges again; `syncAfterWrite` moves only what the write touched), so undo
 * takes back exactly one action. A clear of more than `CLEAR_IN_ENGINE` records is expanded by
 * the engine (`GridData.clear`), so the page never reads every record of a column to clear it.
 */
import type { DataAttachment, DataField, DataRecordValue } from '../../types/dataDocument'
import { coerceValue, displayValue, extractClipboardFiles, normalizeAttachmentValue } from '../lib/tableEditorShared'
import type { Area } from './axes'
import type { ImageEntry } from './bandBatch'
import { clearOps, coveredCells, emptyValues, fieldDropIndex, fillOps, moveFieldOps, moveRecordOps, pastedRowCount, pasteOps, recordsFrom, serializeCells, writable } from './editOps'
import { type DragActions, installDrags } from './gestures'
import type { GridData, GridRecord, TableOp } from './gridData'
import type { Written } from './gridSync'
import { fieldColumnCount, type GridHit, HitKind } from './hitTest'
import type { ClientRect, GridHandlers, GridInteraction, OverlayFrame } from './interaction'
import { type CellBounds, type CellRef, cellSelection, type GridSelection, sameCell, sameSelection } from './selection'

/** A popup editor the editor draws over a cell (a date or a choice). */
export type CellPopup = {
  kind: 'date' | 'select'
  field: DataField
  recordId: number
  value: DataRecordValue
  /** The cell's client rectangle (null while it is scrolled out of view). */
  anchor: ClientRect | null
  /** The character that opened it, when typing did. */
  text: string | null
  commit(value: DataRecordValue): void
  /** What the popup would commit now (a multiple choice's picks): a press elsewhere commits it. */
  draft(value: DataRecordValue): void
  cancel(): void
}

/** What editing hands to the editor that holds the grid. Every handler is optional. */
export type EditingHandlers = {
  /** A popup editor opens, moves or closes (null). */
  onPopup?(popup: CellPopup | null): void
  /** A click on an attachment: the cell's attachments and which one. */
  onPreviewAttachments?(event: { recordId: number; fieldId: number; items: DataAttachment[]; index: number }): void
  onButton?(event: { recordId: number; fieldId: number }): void
  onOpenLink?(event: { recordId: number; fieldId: number; index: number }): void
  /** An AI cell's regenerate button. */
  onRegenerate?(event: { recordId: number; fieldId: number }): void
  /** After a write, with what it changed: by default the view arranges again. */
  onWritten?(written: Written): Promise<void> | void
  onError?(error: unknown): void
  /** A url the page can load for an attachment's stored url (the host's); by default only `http(s):`, `data:` and `blob:` urls load. */
  resolveFileURL?(url: string): string
}

/** The cells a selection's copy covers before it is read on demand instead of ahead. */
const COPY_AHEAD_CELLS = 20_000
/** The records a clear covers beyond which the engine expands it (`GridData.clear`). */
export const CLEAR_IN_ENGINE = 1_000
/** The add button of the active attachment cell: its side and its left inset (the old grid's). */
export const ADD_BUTTON = 20
export const ADD_BUTTON_INSET = 6
/** Urls a picture loads from without the host. */
const SELF_CONTAINED_URL = /^(https?:|data:|blob:)/i
/** Pictures the view's source decodes: the document's blobs and `data:` urls. */
const HELD_URL = /^(kition-blob:|data:)/i
/** The AI regenerate button: its side and its inset from the cell's top right corner. */
export const AI_BUTTON = 18
export const AI_BUTTON_INSET = 4

/** The fields a text editor edits (a document link opens on a click instead, as the old grid's did). */
const TEXT_TYPES = new Set(['text', 'long_text', 'number', 'url'])
/** Rows taller than this (two 20 px lines and their padding) edit long text in the cell. */
const TALL_ROW = 48

/** The attachment add button's area on the viewport, for a cell at `area` (centred on its height). */
export function addButtonArea(area: Area): Area {
  return { x: area.x + ADD_BUTTON_INSET, y: area.y + (area.height - ADD_BUTTON) / 2, width: ADD_BUTTON, height: ADD_BUTTON }
}

/** The AI regenerate button's area on the viewport, for a cell at `area`. */
export function aiButtonArea(area: Area): Area {
  return { x: area.x + area.width - AI_BUTTON - AI_BUTTON_INSET, y: area.y + AI_BUTTON_INSET, width: AI_BUTTON, height: AI_BUTTON }
}

const inside = (area: Area, x: number, y: number) => x >= area.x && x <= area.x + area.width && y >= area.y && y <= area.y + area.height

/** The text a text editor starts from: the value as the person typed it. */
export function editText(field: DataField, value: DataRecordValue): string {
  if (value == null) return ''
  if (field.type === 'url' && typeof value === 'object' && !Array.isArray(value)) {
    const link = value as { href?: unknown; url?: unknown }
    return String(link.href ?? link.url ?? '')
  }
  return displayValue(value)
}

type Session = {
  kind: 'text' | 'popup'
  cell: CellRef
  recordId: number
  field: DataField
  original: string
  popup?: CellPopup
  draft?: DataRecordValue
  /** An Enter, F2 or double click edit whose value is still being read: it answers the value's text. */
  loading?: Promise<string>
  /** The value's text that arrived during a composition: it goes before the text once the composition ends. */
  before?: string
}

/**
 * The text and caret of an Enter, F2 or double click edit once the cell's value arrives, when keys
 * were typed before it did (regressions/issue-cell-editor-early-typing). Such an edit shows the
 * value with the caret at its end, so typing adds to the value; what was typed before the value
 * arrived goes after it, as it would have had the value been there, and the caret stays where it
 * was in what was typed.
 */
export function withValueBefore(value: string, typed: string, caret: { start: number; end: number }): { text: string; start: number; end: number } {
  return { text: value + typed, start: value.length + caret.start, end: value.length + caret.end }
}

/**
 * Writes `text` to the system clipboard as plain text, as a clipboard item whose text may come
 * later, so the write starts at once even while the text is read (WebKit refuses a write that
 * starts after the key press; the Sheet kit's copy does the same). A browser without such items,
 * or that refuses one, gets the text by `writeText`.
 */
export function writeClipboard(clipboard: Clipboard, text: string | Promise<string>): Promise<void> {
  const value = Promise.resolve(text)
  if (typeof ClipboardItem !== 'undefined' && typeof clipboard.write === 'function') {
    const item = new ClipboardItem({ 'text/plain': value.then((plain) => new Blob([plain], { type: 'text/plain' })) })
    return clipboard.write([item]).catch(async () => clipboard.writeText(await value))
  }
  return value.then((plain) => clipboard.writeText(plain))
}

export class GridEditing {
  private grid: GridInteraction | null = null
  /** The focused textarea on the active cell; a text cell's editor when shown. */
  readonly input: HTMLTextAreaElement
  private session: Session | null = null
  private composing = false
  private copy: { key: string; text: string } | null = null
  /** A copy key was just handled: a copy event the same key press still makes is that copy's. */
  private keyCopy = false
  private copyTimer = 0
  private copyRun = 0
  private generating = new Set<string>()
  private overAIButton = false
  private overAddButton = false
  /** The file picker of the add button, and the cell it adds to. */
  private picker: HTMLInputElement | null = null
  private picking: { recordId: number; field: DataField } | null = null
  private spinFrame = 0
  /** The pictures being loaded or loaded, by image key. */
  private readonly images = new Map<number, 'loading' | 'done'>()
  private readonly cleanup: Array<() => void> = []
  /** The writes and reads in progress (tests wait for them). */
  busy = 0

  constructor(
    readonly data: GridData,
    private readonly handlers: EditingHandlers = {},
    /**
     * `readOnly`: a table this person may only view: no editor opens, nothing is written, nothing
     * drags. `mac`: the platform is a Mac (the kit editor's `isMac()`): Cmd copies, not Ctrl.
     */
    private readonly options: { readOnly?: boolean; mac?: boolean } = {},
  ) {
    const input = document.createElement('textarea')
    input.dataset.ktableGridInput = 'true'
    input.setAttribute('autocomplete', 'off')
    input.setAttribute('autocapitalize', 'off')
    input.setAttribute('autocorrect', 'off')
    input.spellcheck = false
    input.rows = 1
    this.input = input
    this.hide()
  }

  /** The handlers the grid's interaction calls for editing (merge them into its `GridHandlers`). */
  get gridHandlers(): Pick<GridHandlers, 'onEdit' | 'onDelete' | 'onCellPart'> {
    return {
      onEdit: (cell, text) => this.edit(cell, text),
      onDelete: (selection) => void this.clear(selection),
      onCellPart: (hit) => void this.cellPart(hit),
    }
  }

  get interaction(): GridInteraction {
    if (!this.grid) throw new Error('GridEditing is not attached')
    return this.grid
  }

  /** Whether a cell editor is open. */
  get editing(): boolean {
    return this.session != null
  }

  /** The copy text is read for the current selection (copying is then synchronous). */
  get copyReady(): boolean {
    return this.copy?.key === this.copyKey()
  }

  attach(interaction: GridInteraction) {
    this.grid = interaction
    const view = interaction.view
    const input = this.input
    view.element.append(input)
    const on = <K extends keyof HTMLElementEventMap>(target: HTMLElement, type: K, listener: (event: HTMLElementEventMap[K]) => void, capture = false) => {
      target.addEventListener(type, listener, capture)
      this.cleanup.push(() => target.removeEventListener(type, listener, capture))
    }
    // The grid's focus is the textarea's: keys and the clipboard arrive there.
    on(view.scroller, 'focus', () => this.focus())
    // A press on the grid closes an open editor first (committing it).
    on(view.scroller, 'pointerdown', () => {
      if (this.session) this.commit()
    }, true)
    on(view.scroller, 'click', (event) => this.onClick(event))
    on(input, 'keydown', (event) => this.onKey(event))
    on(input, 'compositionstart', () => {
      this.composing = true
      if (!this.session) this.startTyping('', true)
    })
    on(input, 'compositionend', (event) => {
      this.composing = false
      if (!this.session && event.data) this.startTyping(event.data, false)
      else if (this.session?.before != null) this.putValueBefore(this.session.before)
    })
    on(input, 'input', () => {
      // Text that reached the hidden textarea without a key the grid saw (a dead key, dictation).
      if (!this.session && !this.composing && input.value) this.startTyping(input.value, false)
      // Long text in a tall row keeps its lines centred as they grow.
      else if (this.session?.kind === 'text' && this.session.field.type === 'long_text') this.place()
    })
    on(input, 'copy', (event) => this.onCopy(event, false))
    on(input, 'cut', (event) => this.onCopy(event, true))
    // A paste while the grid has the focus (it reaches the window from the textarea, or is sent there).
    const onWindowPaste = (event: ClipboardEvent) => {
      if (event.target === input || document.activeElement === input) this.onPaste(event)
    }
    window.addEventListener('paste', onWindowPaste)
    this.cleanup.push(() => window.removeEventListener('paste', onWindowPaste))
    this.cleanup.push(interaction.onChange(() => this.onSelection()))
    this.cleanup.push(interaction.onGeometry(() => this.place()))
    this.cleanup.push(view.onImages((images) => this.loadImages(images)))
    if (!this.options.readOnly) this.cleanup.push(installDrags(interaction, this.dragActions()))
    this.cleanup.push(interaction.addPainter((frame) => this.paintAI(frame), 120))
    this.cleanup.push(interaction.addPainter((frame) => this.paintAdd(frame), 121))
    this.cleanup.push(
      interaction.addGesture({
        down: (hit) => this.addButtonAt(hit) != null,
        up: (hit) => {
          const at = this.addButtonAt(hit)
          if (at) this.pickFiles(at.recordId, at.field)
        },
        cursor: (hit) => {
          const over = this.addButtonAt(hit) != null
          if (over !== this.overAddButton) {
            this.overAddButton = over
            this.interaction.invalidateOverlay()
          }
          return over ? 'pointer' : null
        },
      }),
    )
    this.cleanup.push(
      interaction.addGesture({
        down: (hit) => this.aiButtonAt(hit) != null,
        up: (hit) => {
          const at = this.aiButtonAt(hit)
          if (at) this.handlers.onRegenerate?.(at)
        },
        cursor: (hit) => {
          // The grid redraws its overlay when the hovered cell changes; the button's own hover is drawn here.
          const over = this.aiButtonAt(hit) != null
          if (over !== this.overAIButton) {
            this.overAIButton = over
            this.interaction.invalidateOverlay()
          }
          return over ? 'pointer' : null
        },
      }),
    )
    this.place()
  }

  destroy() {
    cancelAnimationFrame(this.spinFrame)
    window.clearTimeout(this.copyTimer)
    for (const undo of this.cleanup) undo()
    this.input.remove()
    this.picker?.remove()
  }

  /** Puts the focus on the grid (its textarea). */
  focus() {
    if (this.session?.kind === 'popup') return
    this.input.focus({ preventScroll: true })
  }

  // ---- geometry --------------------------------------------------------------------------

  private hide() {
    // Invisible, but focusable and on the active cell (an input method's window shows there).
    this.input.style.cssText =
      'position:absolute;left:0;top:0;width:1px;height:1px;margin:0;padding:0;border:0;opacity:0;pointer-events:none;resize:none;overflow:hidden;z-index:2;caret-color:transparent;background:transparent;outline:none;white-space:pre-wrap'
  }

  /** Places the textarea on the active cell (shown as the editor while a text cell is edited) and a popup on its cell. */
  private place() {
    const grid = this.grid
    if (!grid) return
    const session = this.session
    const cell = session?.cell ?? grid.active
    const area = cell ? grid.cellScreenArea(cell) : null
    const style = this.input.style
    if (session?.kind === 'text') {
      const brand = grid.style?.border ?? 'currentColor'
      const long = session.field.type === 'long_text'
      if (!area) {
        style.visibility = 'hidden'
        return
      }
      style.visibility = 'visible'
      // One line centred in the row. Long text in a row taller than two lines edits in the cell,
      // its lines centred as the grid draws them; in a shorter row it opens a taller box from the top.
      const tall = long && area.height > TALL_ROW
      const height = long && !tall ? Math.max(area.height, 96) : area.height
      const padY = long ? 6 : Math.max(0, (area.height - 20) / 2 - 2)
      style.cssText = [
        'position:absolute',
        `left:${area.x}px`,
        `top:${area.y}px`,
        `width:${area.width}px`,
        `height:${height}px`,
        'margin:0',
        `padding:${padY}px 8px`,
        `border:2px solid ${brand}`,
        'border-radius:2px',
        'box-sizing:border-box',
        'box-shadow:none',
        'outline:none',
        'resize:none',
        `overflow:${tall ? 'auto' : 'hidden'}`,
        'z-index:2',
        'opacity:1',
        'pointer-events:auto',
        'cursor:text',
        'background:hsl(var(--background, 0 0% 100%))',
        'color:hsl(var(--foreground, 0 0% 10%))',
        'font:13px/20px Inter, system-ui, sans-serif',
        'text-align:center',
        'white-space:pre-wrap',
      ].join(';')
      if (tall) {
        const padding = `${this.centredPadding(area.height)}px`
        style.paddingTop = padding
        style.paddingBottom = padding
      }
      return
    }
    if (session?.kind === 'popup' && session.popup) {
      session.popup = { ...session.popup, anchor: area ? this.clientRect(area) : null }
      this.handlers.onPopup?.(session.popup)
    }
    if (area) {
      style.left = `${area.x}px`
      style.top = `${area.y}px`
      style.width = `${Math.max(1, area.width)}px`
      style.height = `${Math.max(1, area.height)}px`
    }
  }

  /** The top and bottom padding that centres the text's lines (20 px each, the editor's line) in a row `height` tall. */
  private centredPadding(height: number): number {
    // The text's own height, as the editor lays it out: measured with no vertical padding in a box of no height.
    const style = this.input.style
    const [top, bottom, boxHeight] = [style.paddingTop, style.paddingBottom, style.height]
    style.paddingTop = '0px'
    style.paddingBottom = '0px'
    style.height = '0px'
    const lines = Math.max(1, Math.round(this.input.scrollHeight / 20))
    style.paddingTop = top
    style.paddingBottom = bottom
    style.height = boxHeight
    return Math.max(2, (height - 4 - lines * 20) / 2)
  }

  private clientRect(area: Area): ClientRect {
    const box = this.interaction.view.scroller.getBoundingClientRect()
    return { x: box.left + area.x, y: box.top + area.y, width: area.width, height: area.height }
  }

  // ---- what a cell is -----------------------------------------------------------------------

  /** The record and field of a cell, or null for a cell that is not a record's field. */
  private cellOf(cell: CellRef): { recordId: number; field: DataField } | null {
    const grid = this.interaction
    const recordId = grid.rows?.recordId(cell.row)
    const fieldId = grid.view.currentAxes?.columns[cell.col]?.fieldId
    const field = fieldId != null ? this.data.field(fieldId) : undefined
    return recordId != null && field ? { recordId, field } : null
  }

  private async value(recordId: number, field: DataField): Promise<DataRecordValue> {
    const [record] = await this.data.records([recordId], [field.name])
    return (record?.values[field.name] ?? null) as DataRecordValue
  }

  // ---- editing --------------------------------------------------------------------------------

  /**
   * A click on a date cell opens its calendar, and on a document link opens the document, as the
   * old grid did (a date edits on one click; a document link is read here and opens); other cells
   * need a double click, Enter or a key.
   */
  private onClick(event: MouseEvent) {
    if (this.session || event.button !== 0 || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) return
    const grid = this.interaction
    const hit = grid.hitAt(event.clientX, event.clientY)
    const active = grid.active
    const selection = grid.selection
    if (hit.region !== 'cell' || hit.part || !active || active.row !== hit.row || active.col !== hit.col) return
    if (selection.kind !== 'cells' || !sameCell(selection.anchor, selection.focus)) return
    const at = this.cellOf(active)
    if (at?.field.type === 'document_link') this.handlers.onOpenLink?.({ recordId: at.recordId, fieldId: at.field.id, index: 0 })
    else if (at && (at.field.type === 'date' || at.field.type === 'datetime')) this.edit(active, null)
  }

  /** Enter, F2, a double click (`text` null) or a typed character on a cell. */
  edit(cell: CellRef, text: string | null) {
    if (this.session || this.options.readOnly) return
    const at = this.cellOf(cell)
    if (!at || !writable(at.field)) return
    const { recordId, field } = at
    switch (field.type) {
      case 'checkbox':
        if (text === null) void this.toggle(recordId, field)
        return
      case 'rating':
        if (text && /^[0-9]$/.test(text)) void this.write(() => this.data.update(recordId, { [field.name]: Number(text) || null }), cells([recordId]))
        return
      case 'date':
      case 'datetime':
      case 'single_select':
      case 'multi_select':
        void this.openPopup(cell, recordId, field, text)
        return
      case 'attachment':
        if (text === null) void this.preview(recordId, field, 0)
        return
    }
    if (!TEXT_TYPES.has(field.type)) return
    this.openText(cell, recordId, field, text)
  }

  /** Typing reached the textarea while no editor was open (an input method, a dead key). */
  private startTyping(text: string, composing: boolean) {
    const active = this.grid?.active
    if (!active || this.options.readOnly) {
      this.input.value = ''
      return
    }
    const at = this.cellOf(active)
    if (!at || !writable(at.field)) {
      this.input.value = ''
      return
    }
    if (TEXT_TYPES.has(at.field.type)) {
      // The composition (or the text) stays in the textarea, which becomes the editor.
      this.openText(active, at.recordId, at.field, composing ? null : text, true)
      return
    }
    if (!composing) {
      this.input.value = ''
      this.edit(active, text)
    }
  }

  private openText(cell: CellRef, recordId: number, field: DataField, text: string | null, keep = false) {
    const grid = this.interaction
    const session: Session = { kind: 'text', cell, recordId, field, original: '' }
    this.session = session
    grid.setEditing(true)
    if (!keep) this.input.value = text ?? ''
    this.place()
    this.focus()
    if (text !== null || keep) {
      // Typing replaces the cell: what was there counts as the original for the change check.
      void this.value(recordId, field).then((value) => {
        if (this.session === session) session.original = editText(field, value)
      })
      return
    }
    // Enter, F2 or a double click: the editor shows at once and reads the cell's value after. The
    // value goes before whatever was typed meanwhile (`withValueBefore`), never over it.
    this.busy += 1
    const loading = this.value(recordId, field).then((value) => editText(field, value))
    session.loading = loading
    void loading
      .then(
        (original) => {
          if (this.session !== session) return
          session.original = original
          session.loading = undefined
          // Not in the middle of an input method's composition: changing the text would end it.
          if (this.composing) session.before = original
          else this.putValueBefore(original)
        },
        (error: unknown) => {
          if (this.session === session) session.loading = undefined
          if (this.handlers.onError) this.handlers.onError(error)
          else console.error('grid: a cell could not be read', error)
        },
      )
      .finally(() => {
        this.busy -= 1
      })
  }

  /** Puts the cell's value (its text) before what was typed in the open editor. */
  private putValueBefore(value: string) {
    const session = this.session
    if (session) session.before = undefined
    const input = this.input
    const typed = input.value
    const next = withValueBefore(value, typed, { start: input.selectionStart ?? typed.length, end: input.selectionEnd ?? typed.length })
    input.value = next.text
    input.setSelectionRange(next.start, next.end)
  }

  private async openPopup(cell: CellRef, recordId: number, field: DataField, text: string | null) {
    this.interaction.setEditing(true)
    this.session = { kind: 'popup', cell, recordId, field, original: '' }
    this.busy += 1
    try {
      const value = await this.value(recordId, field)
      const session = this.session
      if (session?.recordId !== recordId || session.field.id !== field.id) return
      session.original = JSON.stringify(value ?? null)
      const area = this.interaction.cellScreenArea(cell)
      session.popup = {
        kind: field.type === 'date' || field.type === 'datetime' ? 'date' : 'select',
        field,
        recordId,
        value,
        anchor: area ? this.clientRect(area) : null,
        text,
        commit: (next) => this.commitValue(next),
        draft: (next) => {
          if (this.session === session) session.draft = next
        },
        cancel: () => this.close(),
      }
      this.input.blur()
      this.handlers.onPopup?.(session.popup)
    } catch (error) {
      this.close()
      if (this.handlers.onError) this.handlers.onError(error)
      else console.error('grid: a cell could not be read', error)
    } finally {
      this.busy -= 1
    }
  }

  /** Commits the open editor (a text editor's text; a popup keeps what it committed). */
  commit() {
    const session = this.session
    if (!session) return
    if (session.kind === 'popup') {
      if (session.draft !== undefined) this.commitValue(session.draft)
      else this.close()
      return
    }
    const typed = this.input.value
    const { recordId, field, original, loading, before } = session
    this.close()
    if (loading) {
      // Committed before the cell's value arrived: what was typed goes after it once it does.
      void loading.then((value) => this.store(recordId, field, value, value + typed), () => {})
      return
    }
    this.store(recordId, field, original, before != null ? before + typed : typed)
  }

  /** Writes `text` to a text cell that held `original`, when it changed. */
  private store(recordId: number, field: DataField, original: string, text: string) {
    if (text === original) return
    const value = coerceValue(field, text)
    void this.write(() => this.data.update(recordId, { [field.name]: value }), cells([recordId]))
  }

  /** A popup's value: written when it changed. */
  private commitValue(value: DataRecordValue) {
    const session = this.session
    if (!session) return
    const { recordId, field, original } = session
    this.close()
    const stored = field.type === 'date' || field.type === 'datetime' ? (value === '' ? null : value) : value
    if (JSON.stringify(stored ?? null) === original) return
    void this.write(() => this.data.update(recordId, { [field.name]: stored }), cells([recordId]))
  }

  /** Closes the open editor without writing. */
  close() {
    const session = this.session
    if (!session) return
    this.session = null
    this.interaction.setEditing(false)
    if (session.kind === 'popup') this.handlers.onPopup?.(null)
    this.input.value = ''
    this.hide()
    this.place()
    this.focus()
  }

  private onKey(event: KeyboardEvent) {
    const grid = this.interaction
    const session = this.session
    if (event.isComposing || this.composing || event.keyCode === 229) return
    if (!session) {
      const action = this.clipboardKey(event)
      if (action && this.copyByKey(action === 'cut')) {
        event.preventDefault()
        return
      }
      if (grid.handleKey(event)) event.preventDefault()
      return
    }
    if (session.kind !== 'text') return
    const long = session.field.type === 'long_text'
    switch (event.key) {
      case 'Enter':
        if (long && (event.shiftKey || event.altKey)) return
        event.preventDefault()
        this.commit()
        grid.moveActive(0, event.shiftKey ? -1 : 1)
        return
      case 'Tab':
        event.preventDefault()
        this.commit()
        grid.moveActive(event.shiftKey ? -1 : 1, 0)
        return
      case 'Escape':
        event.preventDefault()
        this.close()
        return
    }
  }

  // ---- writes ------------------------------------------------------------------------------

  private async write(run: () => Promise<unknown>, written: Written) {
    this.busy += 1
    try {
      await run()
      await (this.handlers.onWritten ? this.handlers.onWritten(written) : this.interaction.view.reopen())
      this.copy = null
      this.onSelection()
    } catch (error) {
      if (this.handlers.onError) this.handlers.onError(error)
      else console.error('grid: a write failed', error)
    } finally {
      this.busy -= 1
    }
  }

  private transact(ops: () => Promise<TableOp[]>, written: Written) {
    return this.write(async () => {
      const made = await ops()
      if (made.length) await this.data.transact(made)
    }, written)
  }

  private async toggle(recordId: number, field: DataField) {
    const value = await this.value(recordId, field)
    await this.write(() => this.data.update(recordId, { [field.name]: !value }), cells([recordId]))
  }

  /** The fields of shown columns `cols`. */
  private fieldsOf(cols: number[]): DataField[] {
    const axes = this.interaction.view.currentAxes
    return cols.map((col) => axes?.columns[col]?.fieldId).map((id) => (id != null ? this.data.field(id) : undefined)).filter((field): field is DataField => field != null)
  }

  /**
   * Delete or Backspace: the selection's cells emptied in one transaction; past `CLEAR_IN_ENGINE`
   * records the engine finds the records and writes it.
   */
  clear(selection: GridSelection) {
    const grid = this.interaction
    const rows = grid.rows
    if (this.session || !rows || this.options.readOnly) return Promise.resolve()
    const { recordIds, cols } = coveredCells(selection, rows, grid.columnCount)
    const fields = this.fieldsOf(cols)
    if (!recordIds.length || !fields.length) return Promise.resolve()
    if (recordIds.length > CLEAR_IN_ENGINE) {
      const values = emptyValues(fields)
      if (!Object.keys(values).length) return Promise.resolve()
      return this.write(() => this.data.clear(recordIds, values), cells(recordIds))
    }
    return this.transact(async () => clearOps(this.data.tableId, await this.data.records(recordIds, []), fields), cells(recordIds))
  }

  // ---- clipboard ---------------------------------------------------------------------------

  private copyKey(): string {
    const grid = this.grid
    return grid ? `${grid.view.currentAxes?.epoch}:${JSON.stringify(grid.selection)}` : ''
  }

  /** The selection's cells as the clipboard text. */
  async copyText(selection: GridSelection = this.interaction.selection): Promise<string> {
    const grid = this.interaction
    const rows = grid.rows
    if (!rows) return ''
    const { recordIds, cols } = coveredCells(selection, rows, grid.columnCount)
    const fields = this.fieldsOf(cols)
    if (!recordIds.length || !fields.length) return ''
    const records = await this.data.records(recordIds, fields.map((field) => field.name))
    return serializeCells(records, fields)
  }

  /** A new selection: its copy text is read ahead when it is small, so a copy can set the clipboard at once. */
  private onSelection() {
    window.clearTimeout(this.copyTimer)
    if (!this.session) this.place()
    const grid = this.grid
    const rows = grid?.rows
    if (!grid || !rows) return
    const selection = grid.selection
    const { recordIds, cols } = coveredCells(selection, rows, grid.columnCount)
    if (!recordIds.length || !cols.length || recordIds.length * cols.length > COPY_AHEAD_CELLS) return
    const key = this.copyKey()
    if (this.copy?.key === key) return
    const run = ++this.copyRun
    this.copyTimer = window.setTimeout(() => {
      void this.copyText(selection).then((text) => {
        if (run === this.copyRun) this.copy = { key, text }
      })
    }, 30)
  }

  /** Cmd+C or Cmd+X on a Mac, Ctrl+C or Ctrl+X elsewhere, without Shift or Alt; else null. */
  private clipboardKey(event: KeyboardEvent): 'copy' | 'cut' | null {
    const mac = this.options.mac === true
    if (event.shiftKey || event.altKey || (mac ? !event.metaKey || event.ctrlKey : !event.ctrlKey || event.metaKey)) return null
    // The letter, or the key's place on a keyboard whose layout types another script.
    const letter = /^[a-z]$/i.test(event.key) ? event.key.toLowerCase() : event.code === 'KeyC' ? 'c' : event.code === 'KeyX' ? 'x' : ''
    return letter === 'c' ? 'copy' : letter === 'x' ? 'cut' : null
  }

  /**
   * A copy key on the grid: the selection's text goes through `navigator.clipboard`, written while
   * the key press is the person's (WebKit refuses a write after it). Answers whether it took the
   * key; without a selection or an async clipboard it leaves the key to the browser, whose copy
   * event (`onCopy`) then carries the text.
   */
  private copyByKey(cut: boolean): boolean {
    const clipboard = typeof navigator !== 'undefined' ? navigator.clipboard : undefined
    const selection = this.interaction.selection
    if (!clipboard || selection.kind === 'none') return false
    this.keyCopy = true
    window.setTimeout(() => {
      this.keyCopy = false
    }, 0)
    const ready = this.copy?.key === this.copyKey() ? this.copy.text : null
    void writeClipboard(clipboard, ready ?? this.copyText(selection)).then(
      () => (cut ? this.clear(selection) : undefined),
      (error: unknown) => console.error('grid: copy failed', error),
    )
    return true
  }

  private onCopy(event: ClipboardEvent, cut: boolean) {
    if (this.session) return
    // The key press that made this event wrote the clipboard already (`copyByKey`).
    if (this.keyCopy) {
      event.preventDefault()
      return
    }
    const grid = this.interaction
    const selection = grid.selection
    if (selection.kind === 'none') return
    event.preventDefault()
    if (this.copy?.key === this.copyKey() && event.clipboardData) {
      event.clipboardData.setData('text/plain', this.copy.text)
    } else if (navigator.clipboard) {
      // Not read yet: the clipboard takes the text when it is.
      void writeClipboard(navigator.clipboard, this.copyText(selection)).catch((error: unknown) => console.error('grid: copy failed', error))
    }
    if (cut) void this.clear(selection)
  }

  private onPaste(event: ClipboardEvent) {
    if (this.session || this.options.readOnly) return
    // Files pasted on an attachment cell are added to it (as the old grid did); elsewhere the text is pasted.
    const files = extractClipboardFiles(event.clipboardData)
    const active = this.grid?.active
    const at = files.length && active ? this.cellOf(active) : null
    if (at && at.field.type === 'attachment' && writable(at.field) && this.data.upload) {
      event.preventDefault()
      void this.addFiles(at.recordId, at.field, files)
      return
    }
    const text = event.clipboardData?.getData('text/plain') ?? ''
    if (!text) return
    event.preventDefault()
    void this.paste(text)
  }

  /** Pastes `text` (tab-separated, or one value) from the active cell (a range's top left) on. */
  paste(text: string) {
    if (this.options.readOnly) return Promise.resolve()
    const grid = this.interaction
    const rows = grid.rows
    const selection = grid.selection
    const start = selection.kind === 'cells' ? { row: Math.min(selection.anchor.row, selection.focus.row), col: Math.min(selection.anchor.col, selection.focus.col) } : grid.active
    if (!rows || !start) return Promise.resolve()
    const count = pastedRowCount(text)
    const ids = recordsFrom(rows, start.row, count)
    const cols = Array.from({ length: Math.max(0, grid.columnCount - start.col) }, (_, index) => start.col + index)
    const fields = this.fieldsOf(cols)
    return this.transact(async () => pasteOps(this.data.tableId, text, await this.data.records(ids, []), fields), cells(ids))
  }

  // ---- cells' parts ------------------------------------------------------------------------

  private async cellPart(hit: GridHit) {
    const part = hit.part
    if (!part || hit.row < 0) return
    const at = this.cellOf({ row: hit.row, col: part.col })
    if (!at) return
    const { recordId, field } = at
    switch (part.kind) {
      case HitKind.Checkbox:
        if (writable(field)) await this.toggle(recordId, field)
        return
      case HitKind.Rating: {
        if (!writable(field)) return
        const value = Number(await this.value(recordId, field)) || 0
        const next = part.index + 1
        await this.write(() => this.data.update(recordId, { [field.name]: value === next ? null : next }), cells([recordId]))
        return
      }
      case HitKind.Attachment:
        await this.preview(recordId, field, part.index)
        return
      case HitKind.Button:
        this.handlers.onButton?.({ recordId, fieldId: field.id })
        return
      case HitKind.Link:
        this.handlers.onOpenLink?.({ recordId, fieldId: field.id, index: part.index })
        return
    }
  }

  private async preview(recordId: number, field: DataField, index: number) {
    const items = normalizeAttachmentValue(await this.value(recordId, field))
    if (items.length) this.handlers.onPreviewAttachments?.({ recordId, fieldId: field.id, items, index: Math.min(index, items.length - 1) })
  }

  // ---- drags ---------------------------------------------------------------------------------

  private dragActions(): DragActions {
    const grid = () => this.interaction
    return {
      // The engine lays out a drag handle only when the view neither sorts nor groups.
      canMoveRows: () => true,
      resize: (_col, fieldId, width) => void this.write(() => this.data.patchFieldWidth(fieldId, width), LAYOUT),
      freeze: (count) => void this.write(() => this.data.setFrozen(count), LAYOUT),
      moveColumns: (cols, drop) => {
        const axes = grid().view.currentAxes
        if (!axes) return
        const shown = axes.columns.slice(0, fieldColumnCount(axes)).map((column) => column.fieldId as number)
        const all = this.data.fields().map((field) => field.id)
        const toIndex = fieldDropIndex(all, shown, cols[0], cols[cols.length - 1], drop)
        if (toIndex == null) return
        const ids = cols.map((col) => shown[col])
        void this.transact(async () => moveFieldOps(this.data.tableId, ids, toIndex), LAYOUT)
      },
      moveRow: (source, target, after) => {
        const rows = grid().rows
        const sourceId = rows?.recordId(source)
        const targetId = rows?.recordId(target)
        if (!rows || sourceId == null || targetId == null) return
        const next = after ? recordsFrom(rows, target + 1, 1)[0] : targetId
        void this.transact(async () => {
          const ids = next == null ? [sourceId] : [sourceId, next]
          const records = await this.data.records(ids, [])
          const key = (id: number | undefined) => records.find((record) => record.id === id)?.row_key ?? null
          const moving = key(sourceId)
          return moving ? moveRecordOps(this.data.tableId, moving, next == null ? null : key(next)) : []
        }, cells([sourceId]))
      },
      fill: (bounds: CellBounds, span) => {
        const rows = grid().rows
        if (!rows) return
        const sources = recordsBetween(rows, bounds.top, bounds.bottom)
        const step = span.down ? 1 : -1
        const targets: number[] = []
        for (let row = span.from; span.down ? row <= span.to : row >= span.to; row += step) {
          const id = rows.recordId(row)
          if (id != null) targets.push(id)
        }
        const fields = this.fieldsOf(Array.from({ length: bounds.right - bounds.left + 1 }, (_, index) => bounds.left + index))
        void this.transact(async () => {
          const read = await this.data.records(sources, fields.map((field) => field.name))
          const byId = new Map(read.map((record) => [record.id, record]))
          const ordered = sources.map((id) => byId.get(id)).filter((record): record is GridRecord => record != null)
          return fillOps(this.data.tableId, ordered, await this.data.records(targets, []), fields, span.down)
        }, cells(targets)).then(() => {
          // The range grows over what was filled.
          const g = grid()
          const top = span.down ? bounds.top : span.to
          const bottom = span.down ? span.to : bounds.bottom
          const next = cellSelection({ row: top, col: bounds.left }, { row: bottom, col: bounds.right })
          if (!sameSelection(next, g.selection)) g.setSelection(next, g.active)
        })
      },
    }
  }

  // ---- pictures ------------------------------------------------------------------------------

  /** Loads the pictures the view's source leaves (not the document's blobs, not `data:` urls). */
  private loadImages(entries: ImageEntry[]) {
    const resources = this.interaction.view.resources
    for (const entry of entries) {
      if (HELD_URL.test(entry.url.trim())) continue
      // Loading, or loaded and still held (the view may drop its pictures, then they come again).
      if (this.images.get(entry.key) === 'loading' || resources.image(entry.key)) continue
      this.images.set(entry.key, 'loading')
      void this.decode(entry.url).then((image) => {
        this.images.set(entry.key, 'done')
        if (!image) return
        const view = this.interaction.view
        view.resources.setImage(entry.key, image)
        view.invalidate()
      })
    }
  }

  /**
   * A picture at the url the page loads it from (the url itself when it is self-contained, else
   * what `resolveFileURL` makes of it), decoded; null when it cannot be had.
   */
  private async decode(url: string): Promise<ImageBitmap | null> {
    try {
      const src = this.handlers.resolveFileURL ? this.handlers.resolveFileURL(url) : SELF_CONTAINED_URL.test(url.trim()) ? url.trim() : ''
      return src ? await loadPicture(src) : null
    } catch {
      return null
    }
  }

  // ---- attachments ---------------------------------------------------------------------------

  /** The active attachment cell whose add button is under `hit`, or null (none without an upload). */
  private addButtonAt(hit: GridHit): { recordId: number; field: DataField } | null {
    const grid = this.interaction
    const active = grid.active
    if (hit.region !== 'cell' || !this.data.upload || !active || hit.row !== active.row || hit.col !== active.col || this.session) return null
    const at = this.cellOf(active)
    if (!at || at.field.type !== 'attachment' || !writable(at.field)) return null
    const area = grid.cellScreenArea(active)
    return area && inside(addButtonArea(area), hit.x, hit.y) ? at : null
  }

  /** Opens the file picker for the attachments of a record's field. */
  private pickFiles(recordId: number, field: DataField) {
    let picker = this.picker
    if (!picker) {
      picker = document.createElement('input')
      picker.type = 'file'
      picker.multiple = true
      picker.dataset.ktableGridFiles = 'true'
      picker.style.display = 'none'
      picker.addEventListener('change', () => {
        const target = this.picking
        const files = Array.from(picker?.files ?? [])
        this.picking = null
        if (picker) picker.value = ''
        if (target && files.length) void this.addFiles(target.recordId, target.field, files)
      })
      this.interaction.view.element.append(picker)
      this.picker = picker
    }
    this.picking = { recordId, field }
    picker.click()
  }

  /** Uploads `files` and adds them to the record's attachments, after the ones it holds, in one `records.update`. */
  private addFiles(recordId: number, field: DataField, files: File[]) {
    const data = this.data
    if (!data.upload || !files.length) return Promise.resolve()
    return this.write(async () => {
      const uploaded = await Promise.all(files.map((file) => data.upload?.(file)))
      const current = normalizeAttachmentValue(await this.value(recordId, field))
      await data.update(recordId, { [field.name]: [...current, ...uploaded.filter((item): item is DataAttachment => item != null)] })
    }, cells([recordId]))
  }

  /** The add button on the active attachment cell, as the old grid drew it (a plus at the cell's left). */
  private paintAdd(frame: OverlayFrame) {
    const grid = this.interaction
    const active = grid.active
    if (!active || !this.data.upload || this.session) return
    const at = this.cellOf(active)
    if (!at || at.field.type !== 'attachment' || !writable(at.field)) return
    const area = grid.cellScreenArea(active)
    if (!area) return
    const { ctx, dpr, style } = frame
    const zone = addButtonArea(area)
    ctx.save()
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.beginPath()
    ctx.rect(area.x, area.y, area.width, area.height)
    ctx.clip()
    ctx.fillStyle = style.rowHead
    ctx.beginPath()
    ctx.roundRect(zone.x, zone.y, zone.width, zone.height, 4)
    ctx.fill()
    ctx.fillStyle = this.overAddButton ? style.selection : style.hover
    ctx.fill()
    ctx.strokeStyle = this.overAddButton ? style.border : style.control
    ctx.lineWidth = 1.5
    ctx.lineCap = 'round'
    const cx = zone.x + zone.width / 2
    const cy = zone.y + zone.height / 2
    const arm = zone.width / 2 - 5
    ctx.beginPath()
    ctx.moveTo(cx - arm, cy)
    ctx.lineTo(cx + arm, cy)
    ctx.moveTo(cx, cy - arm)
    ctx.lineTo(cx, cy + arm)
    ctx.stroke()
    ctx.restore()
  }

  // ---- AI fields -------------------------------------------------------------------------

  /** The cells being generated (by record and field); a spinner turns over each. */
  setGenerating(cells: Array<{ recordId: number; fieldId: number }>) {
    this.generating = new Set(cells.map((cell) => `${cell.recordId}:${cell.fieldId}`))
    this.interaction.invalidateOverlay()
    this.spin()
  }

  private spin() {
    if (this.spinFrame || !this.generating.size) return
    const tick = () => {
      this.spinFrame = 0
      if (!this.generating.size) return
      this.interaction.invalidateOverlay()
      this.spinFrame = requestAnimationFrame(tick)
    }
    this.spinFrame = requestAnimationFrame(tick)
  }

  private isAI(fieldId: number | null | undefined): boolean {
    const field = fieldId != null ? this.data.field(fieldId) : undefined
    return field?.ai_config?.enabled === true
  }

  /** The AI cell whose regenerate button is under `hit`, or null. */
  private aiButtonAt(hit: GridHit): { recordId: number; fieldId: number } | null {
    if (hit.region !== 'cell' || !this.handlers.onRegenerate) return null
    const grid = this.interaction
    const fieldId = grid.view.currentAxes?.columns[hit.col]?.fieldId
    const recordId = grid.rows?.recordId(hit.row)
    if (fieldId == null || recordId == null || !this.isAI(fieldId)) return null
    const area = grid.cellScreenArea({ row: hit.row, col: hit.col })
    return area && inside(aiButtonArea(area), hit.x, hit.y) ? { recordId, fieldId } : null
  }

  private paintAI(frame: OverlayFrame) {
    const grid = this.interaction
    const { ctx, dpr, style } = frame
    const hover = grid.hover
    const drawButton = (area: Area, over: boolean) => {
      const zone = aiButtonArea(area)
      ctx.save()
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.fillStyle = over ? style.selection : style.hover
      ctx.beginPath()
      ctx.roundRect(zone.x, zone.y, zone.width, zone.height, 4)
      ctx.fill()
      ctx.strokeStyle = over ? style.border : style.control
      ctx.fillStyle = ctx.strokeStyle
      ctx.lineWidth = 1.4
      const cx = zone.x + zone.width / 2
      const cy = zone.y + zone.height / 2
      const r = (zone.width - 8) / 2
      ctx.beginPath()
      ctx.arc(cx, cy, r, Math.PI * 0.2, Math.PI * 1.8)
      ctx.stroke()
      const ex = cx + r * Math.cos(Math.PI * 1.8)
      const ey = cy + r * Math.sin(Math.PI * 1.8)
      ctx.beginPath()
      ctx.moveTo(ex, ey)
      ctx.lineTo(ex + 3, ey - 1)
      ctx.lineTo(ex + 1, ey + 3)
      ctx.closePath()
      ctx.fill()
      ctx.restore()
    }
    if (hover?.region === 'cell' && this.handlers.onRegenerate) {
      const fieldId = frame.axes.columns[hover.col]?.fieldId
      const area = this.isAI(fieldId) ? grid.cellScreenArea({ row: hover.row, col: hover.col }) : null
      if (area) drawButton(area, inside(aiButtonArea(area), hover.x, hover.y))
    }
    if (!this.generating.size || !frame.rows) return
    const rows = frame.rows
    const axes = frame.axes
    const columns = fieldColumnCount(axes)
    const first = Math.max(0, axes.rowAt(frame.viewport.scrollY + axes.headerHeight))
    const last = Math.min(axes.rows - 1, axes.rowAt(frame.viewport.scrollY + frame.viewport.height - 0.001))
    const angle = ((performance.now() / 1000) * Math.PI * 2) % (Math.PI * 2)
    for (let row = first; row <= last; row += 1) {
      const recordId = rows.recordId(row)
      if (recordId == null) continue
      for (let col = 0; col < columns; col += 1) {
        if (!this.generating.has(`${recordId}:${axes.columns[col].fieldId}`)) continue
        const area = grid.cellScreenArea({ row, col })
        if (!area) continue
        ctx.save()
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
        ctx.beginPath()
        ctx.rect(area.x, area.y, area.width, area.height)
        ctx.clip()
        ctx.fillStyle = style.hover
        ctx.fillRect(area.x, area.y, area.width, area.height - 1)
        const radius = Math.min(8, Math.min(area.width, area.height) / 3)
        ctx.translate(area.x + area.width / 2, area.y + area.height / 2)
        ctx.rotate(angle)
        ctx.strokeStyle = style.border
        ctx.lineWidth = 2
        ctx.lineCap = 'round'
        for (let spoke = 0; spoke < 8; spoke += 1) {
          ctx.globalAlpha = (spoke + 1) / 8
          ctx.beginPath()
          ctx.moveTo(0, -radius)
          ctx.lineTo(0, -radius * 0.55)
          ctx.stroke()
          ctx.rotate(Math.PI / 4)
        }
        ctx.restore()
      }
    }
  }
}

/** A write of the cells of the records `ids`. */
const cells = (ids: number[]): Written => ({ kind: 'cells', records: ids })
/** A write of the view's columns. */
const LAYOUT: Written = { kind: 'layout' }

/**
 * A picture at `src`, decoded: with CORS first, so the canvas it is drawn on stays readable, then
 * without (a host that sends no CORS headers; the picture still shows, as the old grid showed it).
 */
async function loadPicture(src: string): Promise<ImageBitmap> {
  const load = async (cors: boolean) => {
    const image = new Image()
    if (cors) image.crossOrigin = 'anonymous'
    image.decoding = 'async'
    image.src = src
    await image.decode()
    return createImageBitmap(image)
  }
  if (/^(data:|blob:)/i.test(src)) return load(false)
  try {
    return await load(true)
  } catch {
    return load(false)
  }
}

/** The records of rows `top`..`bottom`, in display order. */
function recordsBetween(rows: { recordId(row: number): number | null }, top: number, bottom: number): number[] {
  const ids: number[] = []
  for (let row = top; row <= bottom; row += 1) {
    const id = rows.recordId(row)
    if (id != null) ids.push(id)
  }
  return ids
}

