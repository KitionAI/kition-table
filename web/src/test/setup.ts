// The editor's specs run in jsdom with the editor's own en-US strings, and with the browser APIs
// jsdom lacks (ResizeObserver, IntersectionObserver, matchMedia, scrollTo, Range geometry) stubbed.
import { afterEach, vi } from 'vitest'
import i18next from 'i18next'
import { initReactI18next } from 'react-i18next'
import table from '../../locales/en-US/table.json'
import common from '../../locales/en-US/common.json'

void i18next.use(initReactI18next).init({
  resources: { 'en-US': { table, common } },
  lng: 'en-US',
  fallbackLng: 'en-US',
  defaultNS: 'table',
  ns: ['table', 'common'],
  interpolation: { escapeValue: false },
  returnNull: false,
  react: { useSuspense: false },
})

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

class IntersectionObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.stubGlobal('ResizeObserver', ResizeObserverMock)
vi.stubGlobal('IntersectionObserver', IntersectionObserverMock)

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

Object.defineProperty(window, 'scrollTo', {
  writable: true,
  value: vi.fn(),
})

const emptyDOMRect = {
  x: 0,
  y: 0,
  width: 0,
  height: 0,
  top: 0,
  right: 0,
  bottom: 0,
  left: 0,
  toJSON: () => ({}),
} as DOMRect

Object.defineProperty(Range.prototype, 'getClientRects', {
  configurable: true,
  value: () => [],
})

Object.defineProperty(Range.prototype, 'getBoundingClientRect', {
  configurable: true,
  value: () => emptyDOMRect,
})

afterEach(() => {
  vi.clearAllMocks()
})
