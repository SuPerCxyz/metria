import { JSDOM } from 'jsdom'

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost/',
})
const { window } = dom

globalThis.window = window
globalThis.document = window.document
Object.defineProperty(globalThis, 'navigator', {
  value: window.navigator,
  configurable: true,
})
globalThis.HTMLElement = window.HTMLElement
globalThis.HTMLInputElement = window.HTMLInputElement
globalThis.HTMLButtonElement = window.HTMLButtonElement
globalThis.HTMLSelectElement = window.HTMLSelectElement
globalThis.HTMLTextAreaElement = window.HTMLTextAreaElement
globalThis.Element = window.Element
globalThis.SVGElement = window.SVGElement
globalThis.Node = window.Node
globalThis.NodeFilter = window.NodeFilter
globalThis.DocumentFragment = window.DocumentFragment
globalThis.ShadowRoot = window.ShadowRoot
globalThis.CustomEvent = window.CustomEvent
globalThis.Event = window.Event
globalThis.DOMRect = window.DOMRect
globalThis.localStorage = window.localStorage
globalThis.getComputedStyle = window.getComputedStyle.bind(window)
globalThis.requestAnimationFrame = (callback) => window.setTimeout(() => callback(Date.now()), 0)
globalThis.cancelAnimationFrame = (id) => window.clearTimeout(id)
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true

window.matchMedia = window.matchMedia || (() => ({
  matches: false,
  media: '',
  onchange: null,
  addListener() {},
  removeListener() {},
  addEventListener() {},
  removeEventListener() {},
  dispatchEvent() { return false },
}))
window.Element.prototype.scrollIntoView = () => {}

// Chart.js 主题色在真实页面由 CSS 变量提供；测试环境注入同语义的占位值。
for (const [name, value] of Object.entries({
  '--color-white': '#ffffff',
  '--color-gray-100': '#f3f4f6',
  '--color-gray-200': '#e5e7eb',
  '--color-gray-400': '#9ca3af',
  '--color-gray-500': '#6b7280',
  '--color-gray-600': '#4b5563',
  '--color-gray-700': '#374151',
  '--color-gray-800': '#1f2937',
})) {
  window.document.documentElement.style.setProperty(name, value)
}
