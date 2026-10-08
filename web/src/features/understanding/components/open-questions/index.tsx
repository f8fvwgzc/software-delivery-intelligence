import { MessageCircleQuestionIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'

import type { Question } from '../../api/queries'
import { AnsweredQuestion } from './answered-question'
import { QuestionForm } from './question-form'

interface OpenQuestionsProps {
  open: Question[]
  answered: Question[]
  projectId: string
}

/** What the platform could not know on its own. Open ones first; answered ones kept as the record. */
export function OpenQuestions({ open, answered, projectId }: OpenQuestionsProps) {
  if (open.length === 0 && answered.length === 0) {
    return (
      <EmptyState
        icon={MessageCircleQuestionIcon}
        title="Nothing to ask"
        description="The scan found what it needed in the repository."
        className="py-8"
      />
    )
  }

  return (
    <div className="flex flex-col gap-5">
      {open.length > 0 ? (
        <div className="grid gap-4 md:grid-cols-2">
          {open.map((question) => (
            <QuestionForm key={question.id} question={question} projectId={projectId} />
          ))}
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">Every question has an answer.</p>
      )}

      {answered.length > 0 ? (
        <div className="flex flex-col gap-1">
          <h3 className="flex items-baseline gap-2 text-xs font-medium text-muted-foreground">
            Answered
            <span className="tabular-nums">{answered.length}</span>
          </h3>
          <ul className="flex flex-col">
            {answered.map((question) => (
              <AnsweredQuestion key={question.id} question={question} />
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  )
}

export { OpenQuestionsSkeleton } from './questions-skeleton'
