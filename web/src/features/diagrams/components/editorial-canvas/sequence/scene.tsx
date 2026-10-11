import type { KeyboardEvent } from 'react'

import { NODE_KIND_LABEL } from '../../../lib/labels'
import { DASH } from '../graph/edge-path'
import { SAMPLE_HEIGHT, SAMPLE_WIDTH, type LegendItem } from '../legend'
import { HOVER_COLOR, LINE_COLOR, LINE_OPACITY, markerUrl } from '../markers'
import { FILL_ALPHA, STROKE_ALPHA, paletteTint } from '../palette'
import type { SceneProps } from '../types'
import {
  ACTIVATION_WIDTH,
  CAPTION_FONT,
  HEAD_HEIGHT,
  HEAD_RADIUS,
  MESSAGE_FONT,
  PARTICIPANT_FONT,
  SELF_WIDTH,
  type Activation,
  type Message,
  type Participant,
  type SequenceLayout,
} from './layout'

interface SequenceSceneProps extends SceneProps {
  layout: SequenceLayout
}

interface ParticipantProps {
  item: Participant
  emphasised: boolean
  dimmed: boolean
  onHover: (id: string | null) => void
  onOpen?: (path: string) => void
  wasPan: () => boolean
}

/** A box across the top in the participant's tint; an actor gets a head above it. */
function ParticipantBox({ item, emphasised, dimmed, onHover, onOpen, wasPan }: ParticipantProps) {
  const { node, box, tint, lines, caption, actor } = item
  const interactive = Boolean(onOpen && node.path)
  const lineHeight = 16
  const textHeight = lines.length * lineHeight + (caption ? 14 : 0)
  const top = (box.h - textHeight) / 2

  const open = () => {
    if (interactive && node.path && !wasPan()) onOpen?.(node.path)
  }
  const onKeyDown = (event: KeyboardEvent<SVGGElement>) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      if (interactive && node.path) onOpen?.(node.path)
    }
  }
  const kindLabel = NODE_KIND_LABEL[node.kind] ?? node.kind
  const description = [kindLabel, node.caption?.trim() ?? '', node.path ? `opens ${node.path}` : ''].filter(Boolean).join(', ')

  return (
    <g
      transform={`translate(${box.x} ${box.y})`}
      data-dim
      style={{ opacity: dimmed ? 0.35 : 1, cursor: interactive ? 'pointer' : 'default' }}
      className="group outline-none transition-opacity duration-150 motion-reduce:transition-none"
      role={interactive ? 'link' : undefined}
      tabIndex={interactive ? 0 : undefined}
      aria-label={`${node.label}: ${description}`}
      onPointerEnter={() => onHover(node.id)}
      onPointerLeave={() => onHover(null)}
      onFocus={() => onHover(node.id)}
      onBlur={() => onHover(null)}
      onClick={open}
      onKeyDown={onKeyDown}
    >
      {node.description ? <title>{node.description}</title> : null}
      {actor ? (
        <circle
          cx={box.w / 2}
          cy={-HEAD_HEIGHT / 2 + 1}
          r={HEAD_RADIUS}
          style={{ fill: tint.hue, fillOpacity: FILL_ALPHA, stroke: tint.hue, strokeOpacity: emphasised ? 1 : STROKE_ALPHA, strokeWidth: 1 }}
        />
      ) : null}
      <rect
        width={box.w}
        height={box.h}
        rx={6}
        style={{
          fill: tint.hue,
          fillOpacity: FILL_ALPHA,
          stroke: tint.hue,
          strokeOpacity: emphasised ? 1 : STROKE_ALPHA,
          strokeWidth: emphasised ? 1.5 : 1,
        }}
      />
      <text x={box.w / 2} textAnchor="middle" style={{ fill: tint.fg, fontSize: PARTICIPANT_FONT.size, fontWeight: PARTICIPANT_FONT.weight }}>
        {lines.map((line, index) => (
          <tspan key={index} x={box.w / 2} y={top + index * lineHeight + lineHeight / 2} dominantBaseline="central">
            {line}
          </tspan>
        ))}
      </text>
      {caption ? (
        <text
          x={box.w / 2}
          y={top + lines.length * lineHeight + 7}
          textAnchor="middle"
          dominantBaseline="central"
          style={{ fill: tint.fg, fillOpacity: 0.75, fontSize: CAPTION_FONT.size, fontFamily: 'var(--font-mono)' }}
        >
          {caption}
        </text>
      ) : null}
      {interactive ? (
        <rect
          width={box.w}
          height={box.h}
          rx={6}
          className="pointer-events-none fill-none stroke-transparent [stroke-width:2] group-focus-visible:stroke-ring"
        />
      ) : null}
    </g>
  )
}

interface MessageProps {
  item: Message
  markerPrefix: string
  emphasised: boolean
  dimmed: boolean
}

/** A horizontal arrow between two lifelines with its label above; a self-message loops out to the right. */
function MessageArrow({ item, markerPrefix, emphasised, dimmed }: MessageProps) {
  const tone = emphasised ? 'hover' : 'line'
  const head = markerUrl(markerPrefix, item.returns ? 'open' : 'filled', tone)
  const stroke = {
    stroke: emphasised ? HOVER_COLOR : LINE_COLOR,
    strokeOpacity: emphasised ? 1 : LINE_OPACITY,
    strokeWidth: emphasised ? 1.5 : 1.2,
    strokeDasharray: item.returns ? DASH : undefined,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
  }
  const labelStyle = {
    fill: emphasised ? 'var(--foreground)' : 'var(--muted-foreground)',
    fontSize: MESSAGE_FONT.size,
    paintOrder: 'stroke' as const,
    stroke: 'var(--background)',
    strokeWidth: 4,
    strokeLinejoin: 'round' as const,
  }
  const x1 = item.from.x
  const x2 = item.to.x
  const y = item.y
  // Leave the activation bar's half width at both ends so heads sit on the bar, not inside it.
  const gap = ACTIVATION_WIDTH / 2
  const dir = x2 >= x1 ? 1 : -1

  return (
    <g data-dim style={{ opacity: dimmed ? 0.18 : 1 }} className="transition-opacity duration-150 motion-reduce:transition-none">
      {item.self ? (
        <>
          <path
            d={`M ${x1 + gap} ${y - 8} H ${x1 + SELF_WIDTH} V ${y + 8} H ${x1 + gap}`}
            fill="none"
            markerEnd={head}
            style={stroke}
          />
          {item.label ? (
            <text x={x1 + SELF_WIDTH + 8} y={y} dominantBaseline="central" style={labelStyle}>
              {item.label}
            </text>
          ) : null}
        </>
      ) : (
        <>
          <path d={`M ${x1 + gap * dir} ${y} H ${x2 - gap * dir}`} fill="none" markerEnd={head} style={stroke} />
          {item.label ? (
            <text x={(x1 + x2) / 2} y={y - 7} textAnchor="middle" style={labelStyle}>
              {item.label}
            </text>
          ) : null}
        </>
      )}
      {item.edge.evidencePath ? <title>{`Evidence: ${item.edge.evidencePath}`}</title> : null}
    </g>
  )
}

function ActivationBar({ item, dimmed }: { item: Activation; dimmed: boolean }) {
  const { participant, from, to } = item
  return (
    <rect
      data-dim
      x={participant.x - ACTIVATION_WIDTH / 2}
      y={from}
      width={ACTIVATION_WIDTH}
      height={Math.max(to - from, 4)}
      style={{
        fill: participant.tint.hue,
        fillOpacity: FILL_ALPHA,
        stroke: participant.tint.hue,
        strokeOpacity: STROKE_ALPHA,
        strokeWidth: 1,
        opacity: dimmed ? 0.35 : 1,
      }}
      className="transition-opacity duration-150 motion-reduce:transition-none"
    />
  )
}

/** Lifelines under the participants, activations, then the messages top-down. */
export function SequenceScene({ layout, markerPrefix, connected, onHover, onOpen, wasPan }: SequenceSceneProps) {
  return (
    <>
      {layout.participants.map((item) => (
        <line
          key={`life-${item.node.id}`}
          x1={item.x}
          x2={item.x}
          y1={layout.lifeline.top}
          y2={layout.lifeline.bottom}
          style={{ stroke: 'var(--foreground)', strokeOpacity: 0.25, strokeWidth: 1, strokeDasharray: '4 4' }}
        />
      ))}
      {layout.activations.map((item, index) => (
        <ActivationBar
          key={`act-${index}`}
          item={item}
          dimmed={connected !== null && !connected.nodes.has(item.participant.node.id)}
        />
      ))}
      {layout.messages.map((item) => (
        <MessageArrow
          key={item.id}
          item={item}
          markerPrefix={markerPrefix}
          emphasised={connected?.edges.has(item.id) ?? false}
          dimmed={connected !== null && !connected.edges.has(item.id)}
        />
      ))}
      {layout.participants.map((item) => (
        <ParticipantBox
          key={item.node.id}
          item={item}
          emphasised={connected?.nodes.has(item.node.id) ?? false}
          dimmed={connected !== null && !connected.nodes.has(item.node.id)}
          onHover={onHover}
          onOpen={onOpen}
          wasPan={wasPan}
        />
      ))}
    </>
  )
}

function lineSample(prefix: string, returns: boolean) {
  return (
    <path
      d={`M 0 ${SAMPLE_HEIGHT / 2} H ${SAMPLE_WIDTH - 2}`}
      fill="none"
      markerEnd={markerUrl(prefix, returns ? 'open' : 'filled', 'line')}
      style={{ stroke: LINE_COLOR, strokeOpacity: LINE_OPACITY, strokeWidth: 1.2, strokeDasharray: returns ? DASH : undefined }}
    />
  )
}

export function sequenceLegend(layout: SequenceLayout, prefix: string): LegendItem[] {
  const items: LegendItem[] = []
  if (layout.hasCalls) items.push({ id: 'call', label: 'Call', sample: lineSample(prefix, false) })
  if (layout.hasReturns) items.push({ id: 'return', label: 'Return', sample: lineSample(prefix, true) })
  if (layout.activations.length > 0) {
    const tint = paletteTint(0)
    items.push({
      id: 'activation',
      label: 'Active until the return',
      sample: (
        <rect
          x={SAMPLE_WIDTH / 2 - ACTIVATION_WIDTH / 2}
          y={1}
          width={ACTIVATION_WIDTH}
          height={SAMPLE_HEIGHT - 2}
          style={{ fill: tint.hue, fillOpacity: FILL_ALPHA, stroke: tint.hue, strokeOpacity: STROKE_ALPHA, strokeWidth: 1 }}
        />
      ),
    })
  }
  return items
}
