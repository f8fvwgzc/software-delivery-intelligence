import {
  Background,
  BackgroundVariant,
  Controls,
  ReactFlow,
  ReactFlowProvider,
  useNodesInitialized,
  useReactFlow,
  type EdgeTypes,
  type FitViewOptions,
  type NodeTypes,
  type OnNodesChange,
  type ReactFlowState,
  useStore,
} from '@xyflow/react'
import { useEffect, useImperativeHandle } from 'react'

import '@xyflow/react/dist/style.css'
import './flow.css'

import { NODE_KINDS } from '../../lib/labels'
import { EdgeMarkers } from './edge-markers'
import { ArchitectureNodeView } from './node-view'
import { RelationEdge } from './relation-edge'
import type { ArchEdge, ArchNode, ArchitectureCanvasProps } from './types'
import { useFlowElements } from './use-flow-elements'

/** One registration per kind, all drawn by the same view; the kind drives icon and caption. */
const NODE_TYPES: NodeTypes = Object.fromEntries(NODE_KINDS.map((kind) => [kind, ArchitectureNodeView]))
const EDGE_TYPES: EdgeTypes = { relation: RelationEdge }

const FIT_VIEW: FitViewOptions = { padding: 0.15, maxZoom: 1 }

function motionDuration(ms: number) {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : ms
}

const containerWidth = (state: ReactFlowState) => state.width

function CanvasBody({ nodes, edges, selectedId, highlight, onSelect, ref }: ArchitectureCanvasProps) {
  const { positions, flowNodes, flowEdges } = useFlowElements({ nodes, edges, selectedId, highlight })
  const { fitView } = useReactFlow<ArchNode, ArchEdge>()
  const initialized = useNodesInitialized()
  const width = useStore(containerWidth)

  // Re-fit whenever the layout changes (filter, search, history) or the canvas
  // is resized (the details panel opening beside it), once nodes are measured.
  useEffect(() => {
    if (!initialized || positions.size === 0 || width === 0) return
    void fitView({ ...FIT_VIEW, duration: motionDuration(200) })
  }, [initialized, positions, width, fitView])

  useImperativeHandle(
    ref,
    () => ({
      fitView: () => {
        void fitView({ ...FIT_VIEW, duration: motionDuration(250) })
      },
      focusNode: (id) => {
        void fitView({ nodes: [{ id }], padding: 0.6, maxZoom: 1.1, duration: motionDuration(250) })
      },
    }),
    [fitView],
  )

  // Keyboard selection (Enter/Space on a focused node) arrives as a select change.
  const handleNodesChange: OnNodesChange<ArchNode> = (changes) => {
    for (const change of changes) {
      if (change.type !== 'select') continue
      if (change.selected) onSelect(change.id)
      else if (change.id === selectedId) onSelect(null)
    }
  }

  return (
    <ReactFlow<ArchNode, ArchEdge>
      className="arch-flow"
      nodes={flowNodes}
      edges={flowEdges}
      nodeTypes={NODE_TYPES}
      edgeTypes={EDGE_TYPES}
      onNodesChange={handleNodesChange}
      onNodeClick={(_, node) => onSelect(node.id)}
      onPaneClick={() => onSelect(null)}
      fitView
      fitViewOptions={FIT_VIEW}
      minZoom={0.2}
      maxZoom={1.75}
      nodesDraggable={false}
      nodesConnectable={false}
      edgesFocusable={false}
      selectNodesOnDrag={false}
      zoomOnDoubleClick={false}
      // The canvas sits in a scrolling page: the wheel keeps scrolling the
      // page, and zoom comes from pinch or the controls.
      zoomOnScroll={false}
      preventScrolling={false}
      deleteKeyCode={null}
      multiSelectionKeyCode={null}
      selectionKeyCode={null}
      onlyRenderVisibleElements
    >
      <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--border)" />
      <Controls position="bottom-right" showInteractive={false} showFitView={false} aria-label="Zoom" />
      <EdgeMarkers />
    </ReactFlow>
  )
}

/**
 * The React Flow canvas. This module is the heavy chunk: the page lazy-loads
 * it, so React Flow and dagre never land in the main bundle.
 */
export default function ArchitectureCanvas(props: ArchitectureCanvasProps) {
  return (
    <ReactFlowProvider>
      <CanvasBody {...props} />
    </ReactFlowProvider>
  )
}
