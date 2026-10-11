import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'

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
const FIT_PADDING = 0.06
/** Fit never enlarges a small diagram past 1:1. */
const FIT_MAX_ZOOM = 1
const IDENTITY: Viewport = { x: 0, y: 0, k: 1 }

function clampZoom(k: number) {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k))
}

/** The viewport that shows the whole drawing centred in the container. */
function fitTo(size: Size, bounds: Box): Viewport {
  if (size.width === 0 || size.height === 0 || bounds.w === 0 || bounds.h === 0) return IDENTITY
  const usableW = size.width * (1 - FIT_PADDING * 2)
  const usableH = size.height * (1 - FIT_PADDING * 2)
  const k = clampZoom(Math.min(usableW / bounds.w, usableH / bounds.h, FIT_MAX_ZOOM))
  return {
    k,
    x: (size.width - bounds.w * k) / 2 - bounds.x * k,
    y: (size.height - bounds.h * k) / 2 - bounds.y * k,
  }
}

/**
 * Pan and zoom of the canvas as a translate + scale on the content group.
 * Until the user pans or zooms, the viewport is the fit of the drawing to
 * the container, so it follows resizes and re-layouts by itself; a manual
 * viewport sticks until fit-to-view clears it or the drawing changes. The
 * wheel only zooms with ⌘/ctrl (the page keeps scrolling otherwise) and
 * dragging the background pans.
 */
export function useViewport(containerRef: RefObject<HTMLElement | null>, bounds: Box) {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 })
  const [manual, setManual] = useState<Viewport | null>(null)
  const [seenBounds, setSeenBounds] = useState(bounds)
  const drag = useRef<{ pointerId: number; startX: number; startY: number; origin: Viewport } | null>(null)
  /** Set once the pointer travels far enough that the gesture is a pan, not a click. */
  const moved = useRef(false)
  const [dragging, setDragging] = useState(false)

  // A new drawing (regenerate, other diagram) starts fitted again.
  if (seenBounds !== bounds) {
    setSeenBounds(bounds)
    setManual(null)
  }

  const fitted = useMemo(() => fitTo(size, bounds), [size, bounds])
  const viewport = manual ?? fitted

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

  const fitView = useCallback(() => setManual(null), [])

  /** Zooms around a point given in container pixels. */
  const zoomAround = useCallback(
    (factor: number, px: number, py: number) => {
      setManual((current) => {
        const base = current ?? fitted
        const k = clampZoom(base.k * factor)
        const ratio = k / base.k
        return { k, x: px - (px - base.x) * ratio, y: py - (py - base.y) * ratio }
      })
    },
    [fitted],
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
    const state = { pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, origin: viewport }
    drag.current = state
    moved.current = false
    setDragging(true)

    const onMove = (move: PointerEvent) => {
      if (move.pointerId !== state.pointerId) return
      const dx = move.clientX - state.startX
      const dy = move.clientY - state.startY
      if (Math.abs(dx) > 4 || Math.abs(dy) > 4) moved.current = true
      if (moved.current) setManual({ k: state.origin.k, x: state.origin.x + dx, y: state.origin.y + dy })
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
    dragging,
    fitView,
    zoomBy,
    wasPan,
    pointerHandlers: { onPointerDown },
  }
}
