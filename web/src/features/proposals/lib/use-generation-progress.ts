import { useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useState } from 'react'

import { usePlatformEvents, type PlatformEvent } from '@/lib/live/platform-events'

import { proposalKeys } from '../api/keys'

/** The steps GenerateProposal reports over the platform stream, in order (`data.step`). */
export const GENERATION_STEPS = ['retrieving_context', 'ranking', 'generating_artifacts', 'done'] as const
export type GenerationStep = (typeof GENERATION_STEPS)[number]

export const GENERATION_STEP_LABEL: Record<GenerationStep, string> = {
  retrieving_context: 'Retrieving context',
  ranking: 'Ranking options',
  generating_artifacts: 'Generating files',
  done: 'Done',
}

/** How long the finished rail stays on screen before it goes. */
const DONE_LINGER_MS = 2_500

function isGenerationStep(value: string | undefined): value is GenerationStep {
  return (GENERATION_STEPS as readonly string[]).includes(value ?? '')
}

/**
 * The generation step of one proposal, read from the platform stream:
 * `progress` events move it forward, `updated` with `step: done` ends it
 * and refetches the proposal (so the artifact counts and the files panel
 * fill in). Null while nothing is in flight.
 */
export function useGenerationProgress(proposalId: string): GenerationStep | null {
  const queryClient = useQueryClient()
  const [step, setStep] = useState<GenerationStep | null>(null)

  const listener = useCallback(
    (event: PlatformEvent) => {
      if (event.entity !== 'proposal' || event.entityId !== proposalId) return
      const next = event.data.step
      if (event.action === 'progress' && isGenerationStep(next)) {
        setStep(next)
        return
      }
      if (event.action === 'updated' && next === 'done') {
        setStep('done')
        void queryClient.invalidateQueries({ queryKey: proposalKeys.detail(proposalId) })
      }
    },
    [proposalId, queryClient],
  )
  usePlatformEvents(listener)

  useEffect(() => {
    if (step !== 'done') return
    const timer = setTimeout(() => setStep(null), DONE_LINGER_MS)
    return () => clearTimeout(timer)
  }, [step])

  return step
}
