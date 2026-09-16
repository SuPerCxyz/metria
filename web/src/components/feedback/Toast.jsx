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
    timers.current.set(id, window.setTimeout(() => dismiss(id), 3000))
  }, [dismiss])

  useEffect(() => () => {
    timers.current.forEach((timer) => window.clearTimeout(timer))
  }, [])

  return (
    <ToastContext.Provider value={{ notify }}>
      {children}
      <div className="pointer-events-none fixed right-4 top-20 z-[100] flex w-[min(24rem,calc(100vw-2rem))] flex-col gap-3" aria-label="通知" aria-live="polite" aria-atomic="false">
        {toasts.map((toast) => {
          const failed = toast.type === 'error'
          return (
            <div
              key={toast.id}
              role={failed ? 'alert' : 'status'}
              className="pointer-events-auto flex items-start gap-2.5 rounded-xl border border-gray-200 bg-white py-3 pl-3.5 pr-3 text-sm text-gray-700 shadow-lg dark:border-gray-700 dark:bg-gray-800 dark:text-gray-200"
            >
              <svg
                aria-hidden="true"
                className={`mt-0.5 h-4 w-4 shrink-0 ${failed ? 'text-rose-500' : 'text-emerald-500'}`}
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                {failed ? (
                  <>
                    <circle cx="12" cy="12" r="9" />
                    <path d="M12 8v4.5" />
                    <path d="M12 16h.01" />
                  </>
                ) : (
                  <path d="M20 6 9 17l-5-5" />
                )}
              </svg>
              <span className="min-w-0 flex-1 break-words">{toast.message}</span>
              <button
                type="button"
                onClick={() => dismiss(toast.id)}
                className="-mt-0.5 shrink-0 text-gray-400 transition hover:text-gray-600 dark:hover:text-gray-200"
                aria-label="关闭通知"
              >
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
