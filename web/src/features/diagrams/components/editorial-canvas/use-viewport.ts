import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from 'react'

import type { Box } from './types'

export interface Viewport {
  x: number
  y: number
  k: number
}

interface Size {
  width: number
  height: number
}

const MIN_ZOOM = 0.2
const MAX_ZOOM = 2.5
/** Padding around the drawing inside the pane, in pane pixels. */
const FIT_PAD = 16
/** Fit never enlarges a small diagram past 1:1. */
const FIT_MAX_ZOOM = 1

function clampZoom(k: number) {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k))
}

/**
 * Pan and zoom of the canvas as a translate + scale on the content group.
 * The drawing is fitted to the pane's WIDTH (never past 1:1) and the pane
 * scrolls vertically when the drawing is taller than it, so a tall
 * top-down diagram stays readable instead of shrinking to fit its height.
 * Until the user zooms or pans, the viewport follows resizes and
 * re-layouts by itself; a manual viewport sticks until fit-to-view clears
 * it or the drawing changes. The wheel only zooms with ⌘/ctrl (it scrolls
 * otherwise) and dragging the background pans sideways.
 */
export function useViewport(containerRef: RefObject<HTMLElement | null>, bounds: Box) {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 })
  const [manual, setManual] = useState<{ k: number; x: number } | null>(null)
  const [seenBounds, setSeenBounds] = useState(bounds)
  const drag = useRef<{ pointerId: number; startX: number; origin: number } | null>(null)
  /** Set once the pointer travels far enough that the gesture is a pan, not a click. */
  const moved = useRef(false)
  const [dragging, setDragging] = useState(false)
  /** Scroll position to apply after a zoom re-renders the taller/shorter canvas. */
  const pendingScroll = useRef<number | null>(null)

  // A new drawing (regenerate, other diagram) starts fitted again.
  if (seenBounds !== bounds) {
    setSeenBounds(bounds)
    setManual(null)
  }

  const fitted = useMemo(() => {
    if (size.width === 0 || bounds.w === 0) return { k: 1, x: 0 }
    const k = clampZoom(Math.min((size.width - FIT_PAD * 2) / bounds.w, FIT_MAX_ZOOM))
    return { k, x: (size.width - bounds.w * k) / 2 - bounds.x * k }
  }, [size.width, bounds])
  const { k, x } = manual ?? fitted

  /** The drawing's height in pane pixels; the pane scrolls when it is taller than the pane. */
  const drawingHeight = bounds.h * k + FIT_PAD * 2
  const canvasHeight = Math.max(size.height, drawingHeight)
  // A short drawing sits centred; a tall one starts at the top.
  const y = (drawingHeight <= size.height ? (size.height - bounds.h * k) / 2 : FIT_PAD) - bounds.y * k
  const viewport: Viewport = { k, x, y }

  useEffect(() => {
    const element = containerRef.current
    if (!element) return
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect
      if (rect) setSize({ width: rect.width, height: rect.height })
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [containerRef])

  useLayoutEffect(() => {
    const element = containerRef.current
    if (element && pendingScroll.current !== null) {
      element.scrollTop = pendingScroll.current
      pendingScroll.current = null
    }
  })

  const fitView = useCallback(() => {
    setManual(null)
    pendingScroll.current = 0
  }, [])

  /** Zooms around a point given in pane pixels (relative to the visible area). */
  const zoomAround = useCallback(
    (factor: number, px: number, py: number) => {
      const element = containerRef.current
      setManual((current) => {
        const base = current ?? fitted
        const nextK = clampZoom(base.k * factor)
        const ratio = nextK / base.k
        if (element) pendingScroll.current = Math.max(0, (element.scrollTop + py) * ratio - py)
        return { k: nextK, x: px - (px - base.x) * ratio }
      })
    },
    [containerRef, fitted],
  )

  const zoomBy = useCallback(
    (factor: number) => zoomAround(factor, size.width / 2, size.height / 2),
    [zoomAround, size.width, size.height],
  )

  // Wheel must be a non-passive native listener to be allowed to preventDefault.
  useEffect(() => {
    const element = containerRef.current
    if (!element) return
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return
      event.preventDefault()
      const rect = element.getBoundingClientRect()
      zoomAround(Math.exp(-event.deltaY * 0.01), event.clientX - rect.left, event.clientY - rect.top)
    }
    element.addEventListener('wheel', onWheel, { passive: false })
    return () => element.removeEventListener('wheel', onWheel)
  }, [containerRef, zoomAround])

  // Pointer capture is deliberately not used: capturing on the <svg> would
  // redirect the pointerup and so the click away from the node under the
  // pointer. Window listeners track the drag instead.
  const endDrag = useRef<(() => void) | null>(null)
  useEffect(() => () => endDrag.current?.(), [])

  const onPointerDown = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (event.button !== 0 || drag.current) return
    const state = { pointerId: event.pointerId, startX: event.clientX, origin: x }
    const originK = k
    drag.current = state
    moved.current = false
    setDragging(true)

    const onMove = (move: PointerEvent) => {
      if (move.pointerId !== state.pointerId) return
      const dx = move.clientX - state.startX
      if (Math.abs(dx) > 4) moved.current = true
      if (moved.current) setManual({ k: originK, x: state.origin + dx })
    }
    const onUp = (up: PointerEvent) => {
      if (up.pointerId !== state.pointerId) return
      endDrag.current?.()
    }
    endDrag.current = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
      endDrag.current = null
      drag.current = null
      setDragging(false)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)
  }

  /** True when the gesture that just ended was a pan; a node click then does nothing. */
  const wasPan = useCallback(() => moved.current, [])

  return {
    viewport,
    /** Height the <svg> takes inside the scrolling pane. */
    canvasHeight,
    dragging,
    fitView,
    zoomBy,
    wasPan,
    pointerHandlers: { onPointerDown },
  }
}
