import { WifiOffIcon } from 'lucide-react'
import { useSyncExternalStore } from 'react'

function subscribe(onChange: () => void) {
  window.addEventListener('online', onChange)
  window.addEventListener('offline', onChange)
  return () => {
    window.removeEventListener('online', onChange)
    window.removeEventListener('offline', onChange)
  }
}

const getSnapshot = () => navigator.onLine
const getServerSnapshot = () => true

/** True while the browser reports a connection. */
export function useOnline() {
  return useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
}

/**
 * One line under the header while the browser is offline. Panels that
 * failed to load show their own offline copy and refetch by themselves
 * when the connection returns; this bar explains the pause globally.
 */
export function OfflineBanner() {
  const online = useOnline()
  if (online) return null

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex items-center gap-2 border-b bg-warning/8 px-4 py-1.5 text-xs text-warning md:px-6"
    >
      <WifiOffIcon className="size-3.5 shrink-0" aria-hidden="true" />
      <span>You are offline. Lists stop updating and changes cannot be saved until the connection is back.</span>
    </div>
  )
}
