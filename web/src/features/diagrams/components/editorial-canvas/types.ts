import type { Ref } from 'react'

import type { DiagramSpec } from '../../api/queries'
import type { Direction } from '../../lib/direction'

export type { Direction }

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
  /** Picks the renderer: `db_schema` → ER, `sequence` → sequence, anything else → shaped graph. */
  kind: string
  direction: Direction
  title: string
  /** Whether a node's path opens the code browser; actors and externals have none. */
  onOpenPath?: (path: string) => void
  ref?: Ref<CanvasHandle>
  className?: string
}

/** What the shell hands every renderer: hover state and the node interactions. */
export interface SceneProps {
  markerPrefix: string
  /** The node under the pointer or focus, or null. */
  hovered: string | null
  /** Nodes and edges lit by the hover; null when nothing is hovered. */
  connected: { nodes: ReadonlySet<string>; edges: ReadonlySet<string> } | null
  onHover: (id: string | null) => void
  onOpen?: (path: string) => void
  /** Reports whether the pointer gesture that just ended was a pan. */
  wasPan: () => boolean
}
