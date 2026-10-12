/**
 * Turns a stream of "this family changed" signals into at most one flush
 * per `intervalMs` (250 ms → 4 per second), however fast the events come.
 *
 * - The first signal after a quiet period flushes at once (leading edge),
 *   so a single event still refreshes the page promptly.
 * - Signals during the interval are merged into one flush at its end
 *   (trailing edge); a family is listed once no matter how often it fired.
 * - While `isHidden()` is true nothing flushes: the pending set waits for
 *   `resume()`, which the owner calls when the document is visible again.
 *
 * Pure and framework-free so the stress page can drive it at 100 events
 * per second and count the flushes.
 */

export type Family = readonly string[]

export interface CoalescerOptions {
  onFlush: (families: Family[]) => void
  /** Minimum time between two flushes. */
  intervalMs?: number
  /** Deferred while true; `resume()` releases the backlog. */
  isHidden?: () => boolean
  now?: () => number
}

export interface InvalidationCoalescer {
  add: (family: Family) => void
  /** Flushes the pending families now, if any (and not hidden). */
  flush: () => void
  /** Call when the document becomes visible: flushes what was held back. */
  resume: () => void
  dispose: () => void
  readonly stats: { readonly added: number; readonly flushes: number; readonly pending: number }
}

export const DEFAULT_INTERVAL_MS = 250

export function createInvalidationCoalescer({
  onFlush,
  intervalMs = DEFAULT_INTERVAL_MS,
  isHidden = () => false,
  now = () => performance.now(),
}: CoalescerOptions): InvalidationCoalescer {
  const pending = new Map<string, Family>()
  let timer: ReturnType<typeof setTimeout> | undefined
  let lastFlushAt = -Infinity
  let disposed = false
  const stats = { added: 0, flushes: 0, pending: 0 }

  const flush = () => {
    if (timer) clearTimeout(timer)
    timer = undefined
    if (disposed || pending.size === 0 || isHidden()) return
    const families = [...pending.values()]
    pending.clear()
    stats.pending = 0
    stats.flushes += 1
    lastFlushAt = now()
    onFlush(families)
  }

  const schedule = () => {
    if (timer || isHidden()) return
    const elapsed = now() - lastFlushAt
    if (elapsed >= intervalMs) {
      flush()
      return
    }
    timer = setTimeout(flush, intervalMs - elapsed)
  }

  return {
    add(family) {
      if (disposed) return
      stats.added += 1
      pending.set(family.join('/'), family)
      stats.pending = pending.size
      schedule()
    },
    flush,
    resume() {
      if (pending.size > 0) schedule()
    },
    dispose() {
      disposed = true
      if (timer) clearTimeout(timer)
      timer = undefined
      pending.clear()
    },
    stats,
  }
}
