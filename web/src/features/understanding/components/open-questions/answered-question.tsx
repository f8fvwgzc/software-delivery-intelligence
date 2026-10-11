import { CheckIcon, FileCogIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data/relative-time'

import type { Question } from '../../api/queries'
import { isAnsweredByConfig } from '../../lib/labels'

/**
 * A question that has been answered: prompt, answer, when. Nothing to do.
 * Answers that came from `.sdi.yml` say so instead of a timestamp.
 */
export function AnsweredQuestion({ question }: { question: Question }) {
  const byConfig = isAnsweredByConfig(question)

  return (
    <li className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-baseline gap-x-3 border-b border-border/60 py-2 last:border-0">
      {byConfig ? (
        <FileCogIcon className="size-3.5 translate-y-0.5 text-muted-foreground" aria-label="Answered by configuration" />
      ) : (
        <CheckIcon className="size-3.5 translate-y-0.5 text-success" aria-label="Answered" />
      )}
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-xs text-muted-foreground">{question.prompt}</span>
        <span className="text-sm">{question.answer}</span>
      </div>
      {byConfig ? (
        <span className="font-mono text-2xs text-muted-foreground">answered by .sdi.yml</span>
      ) : (
        <RelativeTime value={question.answeredAt} className="font-mono text-2xs text-muted-foreground" />
      )}
    </li>
  )
}
