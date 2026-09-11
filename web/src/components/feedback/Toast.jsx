import React, { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react'

const ToastContext = createContext(null)
let nextToastId = 0

export function ToastProvider({ children }) {
  const [toasts, setToasts] = useState([])
  const timers = useRef(new Map())

  const dismiss = useCallback((id) => {
    setToasts((items) => items.filter((item) => item.id !== id))
    const timer = timers.current.get(id)
    if (timer) window.clearTimeout(timer)
    timers.current.delete(id)
  }, [])

  const notify = useCallback((message, type = 'success') => {
    const id = ++nextToastId
    setToasts((items) => [...items.slice(-3), { id, message, type }])
    timers.current.set(id, window.setTimeout(() => dismiss(id), 4500))
  }, [dismiss])

  useEffect(() => () => {
    timers.current.forEach((timer) => window.clearTimeout(timer))
  }, [])

  return (
    <ToastContext.Provider value={{ notify }}>
      {children}
      <div className="pointer-events-none fixed right-4 top-4 z-[100] flex w-[min(24rem,calc(100vw-2rem))] flex-col gap-3" aria-label="通知" aria-live="polite" aria-atomic="false">
        {toasts.map((toast) => {
          const failed = toast.type === 'error'
          return (
            <div
              key={toast.id}
              role={failed ? 'alert' : 'status'}
              className={`pointer-events-auto flex items-start gap-3 rounded-xl border px-4 py-3 text-sm shadow-lg ${failed
                ? 'border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-800/70 dark:bg-rose-950/80 dark:text-rose-200'
                : 'border-emerald-200 bg-emerald-50 text-emerald-700 dark:border-emerald-800/70 dark:bg-emerald-950/80 dark:text-emerald-200'}`}
            >
              <span className="min-w-0 flex-1 break-words">{toast.message}</span>
              <button type="button" onClick={() => dismiss(toast.id)} className="shrink-0 text-current opacity-70 hover:opacity-100" aria-label="关闭通知">
                ×
              </button>
            </div>
          )
        })}
      </div>
    </ToastContext.Provider>
  )
}

export function useToast() {
  const context = useContext(ToastContext)
  if (!context) throw new Error('useToast must be used inside ToastProvider')
  return context
}
