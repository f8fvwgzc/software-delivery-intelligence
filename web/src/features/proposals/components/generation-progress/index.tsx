import { Rail, RailStep, type RailStatus } from '@/components/system/rail'
import { Tag } from '@/components/system/tag'

import { GENERATION_STEP_LABEL, GENERATION_STEPS, type GenerationStep } from '../../lib/use-generation-progress'

interface GenerationProgressProps {
  step: GenerationStep
}

function statusFor(step: GenerationStep, current: GenerationStep): RailStatus {
  const at = GENERATION_STEPS.indexOf(step)
  const now = GENERATION_STEPS.indexOf(current)
  if (current === 'done') return 'done'
  if (at < now) return 'done'
  if (at === now) return 'running'
  return 'pending'
}

/**
 * The three generation steps on the shared rail while the proposal fills
 * in: the options arrive within seconds, the files follow over the stream.
 */
export function GenerationProgress({ step }: GenerationProgressProps) {
  const finished = step === 'done'
  return (
    <section
      aria-label="Generation progress"
      aria-live="polite"
      className="flex flex-col gap-2 rounded-lg px-3 py-2 ring-1 ring-border"
      data-generation-step={step}
    >
      <div className="flex items-center gap-2 text-xs">
        <Tag tone={finished ? 'success' : 'brand'} dot live={!finished}>
          {finished ? 'Files ready' : 'Generating'}
        </Tag>
        <span className="text-muted-foreground">
          {finished ? 'The generated files are in the proposal.' : 'The options are ranked; the files are being written.'}
        </span>
      </div>
      <Rail className="flex-row flex-wrap gap-x-6 gap-y-1">
        {GENERATION_STEPS.filter((item) => item !== 'done').map((item) => (
          <RailStep
            key={item}
            status={statusFor(item, step)}
            title={<span className="text-xs">{GENERATION_STEP_LABEL[item]}</span>}
            compact
            className="pb-0 after:hidden"
          />
        ))}
      </Rail>
    </section>
  )
}
