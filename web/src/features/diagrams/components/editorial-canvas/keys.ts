import type { DiagramEdge } from '../../api/queries'

/** The id an edge is drawn and highlighted under: its own, or its position when the spec left it empty. */
export function edgeKey(edge: DiagramEdge, index: number): string {
  return edge.id || `e${index}`
}
