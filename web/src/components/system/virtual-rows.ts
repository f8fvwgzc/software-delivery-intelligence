import { useVirtualizer, useWindowVirtualizer, type VirtualItem, type Virtualizer } from '@tanstack/react-virtual'
import { useLayoutEffect, useRef, useState, type RefObject } from 'react'

/**
 * Lists below this many rows render every row: the DOM is cheap, the
 * stagger animation and CSS counters keep working, and nothing needs a
 * measured scroll container. Above it rows are windowed.
 */
export const VIRTUAL_THRESHOLD = 200

export interface VirtualRows {
  /** True when only a window of the rows is rendered. */
  virtual: boolean
  /** The rows to render; every row when not virtual. */
  items: VirtualItem[]
  /** Height of the whole list, for the spacer that keeps the scrollbar honest. */
  totalSize: number
  /** Pixels the window starts at, relative to the list element. */
  offsetTop: number
  /** Pixels left below the window, so the list keeps its full height. */
  offsetBottom: number
  /** Attach to each rendered row (`data-index` set) when heights vary. */
  measure: (element: HTMLElement | null) => void
  scrollToIndex: (index: number, options?: { align?: 'start' | 'center' | 'end' | 'auto' }) => void
}

interface WindowRowsOptions {
  count: number
  /** Estimated row height in px; exact when `fixed`. */
  estimateSize: number
  /** The list element, to measure where it starts in the document. */
  listRef: RefObject<HTMLElement | null>
  overscan?: number
  /** Rows above this threshold are windowed. */
  threshold?: number
  /** Forces windowing on or off regardless of the threshold. */
  enabled?: boolean
}

const EVERY_ROW = (count: number, size: number): VirtualItem[] =>
  Array.from({ length: count }, (_, index) => ({ index, key: index, start: index * size, end: (index + 1) * size, size, lane: 0 }))

function adapt(virtualizer: Virtualizer<HTMLElement, Element> | Virtualizer<Window, Element>, scrollMargin: number): Omit<VirtualRows, 'virtual'> {
  const items = virtualizer.getVirtualItems()
  const totalSize = virtualizer.getTotalSize()
  const last = items[items.length - 1]
  return {
    items,
    totalSize,
    offsetTop: items.length > 0 ? items[0].start - scrollMargin : 0,
    offsetBottom: last ? Math.max(0, totalSize - (last.end - scrollMargin)) : 0,
    measure: virtualizer.measureElement,
    scrollToIndex: (index, options) => virtualizer.scrollToIndex(index, options),
  }
}

/**
 * Windows the rows of a list that scrolls with the page (a table, the code
 * viewer). Below the threshold every row is returned and nothing is
 * measured, so small lists cost nothing.
 */
export function useWindowRows({ count, estimateSize, listRef, overscan = 12, threshold = VIRTUAL_THRESHOLD, enabled }: WindowRowsOptions): VirtualRows {
  const virtual = enabled ?? count > threshold
  const [scrollMargin, setScrollMargin] = useState(0)

  useLayoutEffect(() => {
    if (!virtual) return
    const element = listRef.current
    if (!element) return
    const update = () => {
      const top = element.getBoundingClientRect().top + window.scrollY
      setScrollMargin((previous) => (Math.abs(previous - top) < 1 ? previous : top))
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(document.body)
    return () => observer.disconnect()
  }, [virtual, listRef])

  // The compiler lint flags TanStack Virtual's unstable helpers; this hook is not compiled and returns fresh values each render.
  // oxlint-disable-next-line react/incompatible-library
  const virtualizer = useWindowVirtualizer({
    count: virtual ? count : 0,
    estimateSize: () => estimateSize,
    overscan,
    scrollMargin,
  })

  if (!virtual) {
    return {
      virtual: false,
      items: EVERY_ROW(count, estimateSize),
      totalSize: count * estimateSize,
      offsetTop: 0,
      offsetBottom: 0,
      measure: () => undefined,
      scrollToIndex: (index) => {
        listRef.current?.children[index]?.scrollIntoView({ block: 'center' })
      },
    }
  }
  return { virtual: true, ...adapt(virtualizer, scrollMargin) }
}

interface ScrollRowsOptions {
  count: number
  estimateSize: number
  /** The element that scrolls. */
  scrollRef: RefObject<HTMLElement | null>
  overscan?: number
  threshold?: number
  enabled?: boolean
}

/** Windows the rows of a list inside its own scroll container (the event log). */
export function useScrollRows({ count, estimateSize, scrollRef, overscan = 16, threshold = VIRTUAL_THRESHOLD, enabled }: ScrollRowsOptions): VirtualRows {
  const virtual = enabled ?? count > threshold
  // oxlint-disable-next-line react/incompatible-library
  const virtualizer = useVirtualizer({
    count: virtual ? count : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimateSize,
    overscan,
  })
  const lastCount = useRef(count)
  lastCount.current = count

  if (!virtual) {
    return {
      virtual: false,
      items: EVERY_ROW(count, estimateSize),
      totalSize: count * estimateSize,
      offsetTop: 0,
      offsetBottom: 0,
      measure: () => undefined,
      scrollToIndex: (index, options) => {
        const element = scrollRef.current
        if (!element) return
        if (options?.align === 'end' || index >= lastCount.current - 1) element.scrollTop = element.scrollHeight
      },
    }
  }
  return { virtual: true, ...adapt(virtualizer, 0) }
}
