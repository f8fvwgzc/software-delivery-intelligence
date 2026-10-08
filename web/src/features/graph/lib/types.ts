import type { components } from '@/lib/api/schema'

/** Aliases over the generated contract so components never reach into `components['schemas']`. */
export type Graph = components['schemas']['Graph']
export type GraphNode = components['schemas']['GraphNode']
export type GraphEdge = components['schemas']['GraphEdge']
export type NodeKind = components['schemas']['NodeKind']
export type RelationCategory = components['schemas']['RelationCategory']
export type ImpactAnalysis = components['schemas']['ImpactAnalysis']
export type ImpactedNode = components['schemas']['ImpactedNode']

/** Nodes to emphasise after an impact analysis: the origin plus everything it reaches. */
export interface ImpactHighlight {
  originId: string
  ids: ReadonlySet<string>
}
