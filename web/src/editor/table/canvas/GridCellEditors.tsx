/**
 * The new grid's popup cell editors (grid plan F9): a date cell's compact calendar and time menu,
 * and a choice cell's list. `GridEditing` asks for one with a `CellPopup` (the field, the value,
 * the cell's client rectangle) and writes what it commits; they sit over the cell in the page,
 * outside the canvas. Primitives and theme are the editor's own (@kition/sdk/editor's popover, the
 * editor's Tailwind tokens), strings its `table` namespace.
 *
 * The date editor keeps today's behaviour: it opens on the calendar; a day commits at once (the
 * time of day kept), the time menu offers half hours (and the cell's own time), Clear empties
 * the cell and Today picks today; there is no native date input. The choice list filters as one
 * types, arrows move through it and Enter picks; a single choice commits on the pick, a multiple
 * choice toggles and commits when the list closes. Escape closes either without writing.
 */
import { Popover, PopoverContent, PopoverTrigger } from '@kition/sdk/editor'
import { CalendarDaysIcon as CalendarDays, CheckIcon, ChevronLeftIcon as ChevronLeft, ChevronRightIcon as ChevronRight, Clock3Icon as Clock3 } from '@kition/tessera/icons/react'
import { type CSSProperties, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../../../lib/cn'
import type { DataRecordValue } from '../../types/dataDocument'
import { fromDateInputValue, toDateInputValue } from '../lib/dateFormatting'
import { normalizeChoiceList } from '../lib/tableEditorShared'
import type { CellPopup } from './gridEditing'

type InputType = 'date' | 'datetime-local'

const HALF_HOURS = Array.from({ length: 48 }, (_, index) => `${String(Math.floor(index / 2)).padStart(2, '0')}:${index % 2 ? '30' : '00'}`)

const pad = (value: number) => String(value).padStart(2, '0')

/** A `YYYY-MM-DD` day or a date-time, as a local `Date`. */
export function parseEditorDate(value: string): Date | null {
  const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value)
  const date = day ? new Date(Number(day[1]), Number(day[2]) - 1, Number(day[3])) : new Date(value)
  return Number.isNaN(date.getTime()) ? null : date
}

const dayKey = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
const sameDay = (a: Date, b: Date) => dayKey(a) === dayKey(b)

/** The 42 days a month's calendar shows, from the Sunday on or before its first. */
export function monthDays(month: Date): Date[] {
  const start = new Date(month.getFullYear(), month.getMonth(), 1)
  start.setDate(1 - start.getDay())
  return Array.from({ length: 42 }, (_, index) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + index))
}

/** What a date cell stores for a picked local date-time: the day for a date field, the instant for a date-time one. */
export function storedDate(inputType: InputType, date: Date): string {
  const local = inputType === 'date' ? dayKey(date) : `${dayKey(date)}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  return fromDateInputValue(inputType, local)
}

function DateCellEditor({ popup, locale, style }: { popup: CellPopup; locale: string; style: CSSProperties }) {
  const { t } = useTranslation('table')
  const inputType: InputType = popup.field.type === 'date' ? 'date' : 'datetime-local'
  const value = toDateInputValue(inputType, popup.value)
  const selected = useMemo(() => parseEditorDate(value), [value])
  const today = useMemo(() => new Date(), [])
  const [picker, setPicker] = useState<'date' | 'time' | null>('date')
  const [month, setMonth] = useState(() => selected ?? today)
  const selectedTime = useRef<HTMLButtonElement | null>(null)
  const datePart = value.slice(0, 10)
  const timePart = value.includes('T') ? value.slice(11, 16) : ''
  const times = useMemo(() => (!timePart || HALF_HOURS.includes(timePart) ? HALF_HOURS : [...HALF_HOURS, timePart].sort()), [timePart])
  const weekdays = useMemo(() => Array.from({ length: 7 }, (_, index) => new Intl.DateTimeFormat(locale, { weekday: 'narrow' }).format(new Date(2026, 0, 4 + index))), [locale])
  const monthLabel = useMemo(() => new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'long' }).format(month), [locale, month])

  useEffect(() => {
    if (picker === 'time') selectedTime.current?.scrollIntoView?.({ block: 'center' })
  }, [picker])

  const commit = (next: string) => {
    setPicker(null)
    popup.commit(next === '' ? null : next)
  }
  const chooseDay = (day: Date) => {
    const base = selected ?? today
    const keep = inputType === 'datetime-local'
    commit(storedDate(inputType, new Date(day.getFullYear(), day.getMonth(), day.getDate(), keep ? base.getHours() : 0, keep ? base.getMinutes() : 0, keep ? base.getSeconds() : 0)))
  }
  const chooseTime = (time: string) => {
    const [hours, minutes] = time.split(':').map(Number)
    const next = new Date(selected ?? today)
    next.setHours(hours, minutes, 0, 0)
    commit(storedDate('datetime-local', next))
  }
  // Escape, or a press outside the editor and its menus, closes it without writing.
  const root = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        popup.cancel()
      }
    }
    const onPress = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null
      if (target && (root.current?.contains(target) || target.closest('[data-testid="table-date-calendar"], [data-testid="table-date-time-menu"]'))) return
      popup.cancel()
    }
    document.addEventListener('keydown', onKey, true)
    document.addEventListener('pointerdown', onPress, true)
    return () => {
      document.removeEventListener('keydown', onKey, true)
      document.removeEventListener('pointerdown', onPress, true)
    }
  }, [popup])

  return (
    <div ref={root} style={style} data-testid="table-date-cell-editor" className="flex items-center overflow-hidden rounded-[2px] border-2 border-primary bg-background text-[13px] text-foreground">
      <Popover open={picker === 'date'}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-testid="table-date-trigger"
            onClick={() => setPicker('date')}
            className={cn('flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 px-2 text-left outline-none hover:bg-accent/50', picker === 'date' && 'bg-accent/45')}
          >
            <CalendarDays className="size-3.5 shrink-0 text-muted-foreground" />
            <span className={cn('truncate tabular-nums', !datePart && 'text-muted-foreground')}>{datePart || 'YYYY-MM-DD'}</span>
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          side="bottom"
          sideOffset={4}
          collisionPadding={8}
          data-testid="table-date-calendar"
          className="!w-[282px] rounded-lg border border-border bg-popover p-0 shadow-floating"
          onOpenAutoFocus={(event) => event.preventDefault()}
        >
          <div className="flex items-center justify-between px-3 pb-2 pt-3">
            <span className="text-sm font-semibold text-foreground">{monthLabel}</span>
            <div className="flex items-center gap-1">
              {([
                [-1, 'datePicker.previousMonth', ChevronLeft],
                [1, 'datePicker.nextMonth', ChevronRight],
              ] as const).map(([offset, label, Icon]) => (
                <button
                  key={offset}
                  type="button"
                  aria-label={t(label)}
                  onClick={() => setMonth((current) => new Date(current.getFullYear(), current.getMonth() + offset, 1))}
                  className="grid size-7 cursor-pointer place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  <Icon className="size-4" />
                </button>
              ))}
            </div>
          </div>
          <div className="grid grid-cols-7 px-3 pb-1 text-center text-[11px] font-medium text-muted-foreground">
            {weekdays.map((weekday, index) => (
              <span key={index}>{weekday}</span>
            ))}
          </div>
          <div className="grid grid-cols-7 gap-y-1 px-3 pb-3">
            {monthDays(month).map((day) => {
              const isSelected = selected != null && sameDay(day, selected)
              const isToday = sameDay(day, today)
              return (
                <button
                  key={dayKey(day)}
                  type="button"
                  data-date={dayKey(day)}
                  onPointerDown={(event) => {
                    event.preventDefault()
                    chooseDay(day)
                  }}
                  onClick={(event) => {
                    if (event.detail === 0) chooseDay(day)
                  }}
                  className={cn(
                    'mx-auto grid size-8 cursor-pointer place-items-center rounded-full text-xs tabular-nums',
                    day.getMonth() === month.getMonth() ? 'text-foreground' : 'text-muted-foreground/45',
                    !isSelected && 'hover:bg-accent',
                    isToday && !isSelected && 'text-primary ring-1 ring-inset ring-primary/70',
                    isSelected && 'bg-primary font-semibold text-primary-foreground shadow-sm hover:bg-primary/90',
                  )}
                >
                  {day.getDate()}
                </button>
              )
            })}
          </div>
          <div className="flex items-center justify-between border-t border-border px-3 py-2">
            <button type="button" onClick={() => commit('')} className="cursor-pointer rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-accent hover:text-foreground">
              {t('datePicker.clear')}
            </button>
            <button type="button" onClick={() => chooseDay(new Date())} className="cursor-pointer rounded-md px-2 py-1 text-xs font-semibold text-primary hover:bg-primary/10">
              {t('datePicker.today')}
            </button>
          </div>
        </PopoverContent>
      </Popover>
      {inputType === 'datetime-local' ? (
        <Popover open={picker === 'time'}>
          <PopoverTrigger asChild>
            <button
              type="button"
              data-testid="table-date-time-trigger"
              onClick={() => setPicker('time')}
              className={cn('flex h-full w-[78px] shrink-0 cursor-pointer items-center gap-1 border-l border-border px-2 outline-none hover:bg-accent/50', picker === 'time' && 'bg-accent/45')}
            >
              <Clock3 className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="tabular-nums">{timePart || '00:00'}</span>
            </button>
          </PopoverTrigger>
          <PopoverContent
            align="start"
            side="bottom"
            sideOffset={4}
            collisionPadding={8}
            data-testid="table-date-time-menu"
            className="max-h-[240px] !w-[92px] !overflow-y-auto rounded-lg border border-border bg-popover p-1 shadow-floating"
            onOpenAutoFocus={(event) => event.preventDefault()}
          >
            {times.map((time) => (
              <button
                key={time}
                ref={time === timePart ? selectedTime : undefined}
                type="button"
                onPointerDown={(event) => {
                  event.preventDefault()
                  chooseTime(time)
                }}
                onClick={(event) => {
                  if (event.detail === 0) chooseTime(time)
                }}
                className={cn('flex h-8 w-full cursor-pointer items-center justify-center rounded-md text-xs tabular-nums', time === timePart ? 'bg-primary font-semibold text-primary-foreground' : 'text-foreground hover:bg-accent')}
              >
                {time}
              </button>
            ))}
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  )
}

/** The choices a choice cell holds now. */
function chosen(value: DataRecordValue): string[] {
  if (Array.isArray(value)) return value.map((item) => String(item)).filter(Boolean)
  return value == null || value === '' ? [] : [String(value)]
}

function ChoiceCellEditor({ popup, style }: { popup: CellPopup; style: CSSProperties }) {
  const { t } = useTranslation('table')
  const multiple = popup.field.type === 'multi_select'
  const choices = useMemo(() => normalizeChoiceList(popup.field), [popup.field])
  const [picked, setPicked] = useState(() => chosen(popup.value))
  const [query, setQuery] = useState(popup.text ?? '')
  const [cursor, setCursor] = useState(0)
  const search = useRef<HTMLInputElement | null>(null)
  const shown = useMemo(() => choices.filter((choice) => choice.toLowerCase().includes(query.trim().toLowerCase())), [choices, query])
  useEffect(() => {
    search.current?.focus({ preventScroll: true })
  }, [])
  const done = (next: string[]) => popup.commit(multiple ? next : (next[0] ?? null))
  const pick = (choice: string) => {
    if (!multiple) return done(picked[0] === choice ? [] : [choice])
    const next = picked.includes(choice) ? picked.filter((item) => item !== choice) : [...picked, choice]
    setPicked(next)
    popup.draft(next)
  }
  return (
    <Popover
      open
      onOpenChange={(open) => {
        if (!open) {
          if (multiple) done(picked)
          else popup.cancel()
        }
      }}
    >
      <PopoverTrigger asChild>
        <div style={style} className="rounded-[2px] border-2 border-primary" />
      </PopoverTrigger>
      <PopoverContent
        align="start"
        side="bottom"
        sideOffset={4}
        collisionPadding={8}
        data-testid="table-select-cell-editor"
        className="!w-[240px] rounded-lg border border-border bg-popover p-1 shadow-floating"
        onOpenAutoFocus={(event) => event.preventDefault()}
        onKeyDown={(event) => {
          if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault()
            setCursor((current) => Math.max(0, Math.min(shown.length - 1, current + (event.key === 'ArrowDown' ? 1 : -1))))
          } else if (event.key === 'Enter') {
            event.preventDefault()
            if (shown[cursor]) pick(shown[cursor])
            else if (multiple) done(picked)
          } else if (event.key === 'Escape') {
            event.preventDefault()
            popup.cancel()
          }
        }}
      >
        <input
          ref={search}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value)
            setCursor(0)
          }}
          placeholder={t('cell.searchPlaceholder')}
          className="mb-1 h-8 w-full rounded-md border border-border bg-background px-2 text-[13px] outline-none"
        />
        <div className="max-h-[240px] overflow-y-auto">
          {shown.length === 0 ? <div className="px-2 py-1.5 text-xs text-muted-foreground">{t('cell.noMatches')}</div> : null}
          {shown.map((choice, index) => (
            <button
              key={choice}
              type="button"
              data-choice={choice}
              onPointerDown={(event) => {
                event.preventDefault()
                pick(choice)
              }}
              onClick={(event) => {
                if (event.detail === 0) pick(choice)
              }}
              className={cn('flex h-8 w-full cursor-pointer items-center gap-2 rounded-md px-2 text-left text-[13px]', index === cursor ? 'bg-accent' : 'hover:bg-accent')}
            >
              <span className="flex size-4 shrink-0 items-center justify-center text-primary">{picked.includes(choice) ? <CheckIcon className="size-3.5" /> : null}</span>
              <span className="truncate">{choice}</span>
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  )
}

/** The popup editor `popup` asks for, over its cell (nothing while the cell is out of view). */
export function GridCellPopup({ popup, locale }: { popup: CellPopup | null; locale: string }) {
  if (!popup?.anchor) return null
  const { x, y, width, height } = popup.anchor
  const style: CSSProperties = { position: 'fixed', left: x, top: y, width, height, zIndex: 30 }
  const key = `${popup.recordId}:${popup.field.id}`
  return popup.kind === 'date' ? <DateCellEditor key={key} popup={popup} locale={locale} style={style} /> : <ChoiceCellEditor key={key} popup={popup} style={style} />
}
