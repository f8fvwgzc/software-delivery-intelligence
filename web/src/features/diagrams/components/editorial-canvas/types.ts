import type { Ref } from 'react'

import type { DiagramSpec } from '../../api/queries'

export interface Point {
  x: number
  y: number
}

/** Top-left box in diagram units. */
export interface Box {
  x: number
  y: number
  w: number
  h: number
}

export interface CanvasHandle {
  fitView: () => void
  /** The diagram as a standalone SVG document with tokens resolved. */
  exportSvg: () => string
  /** The diagram rasterised at 2x. */
  exportPng: () => Promise<Blob>
}

export interface EditorialCanvasProps {
  spec: DiagramSpec
  title: string
  /** Whether a node's path opens the code browser; actors and externals have none. */
  onOpenPath?: (path: string) => void
  ref?: Ref<CanvasHandle>
  className?: string
}
