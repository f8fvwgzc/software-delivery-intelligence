import { useId, useState, type FormEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Textarea } from '@/components/ui/textarea'

import { useAnswerQuestion } from '../../api/mutations'
import type { Question } from '../../api/queries'

interface QuestionFormProps {
  question: Question
  projectId: string
}

/**
 * One open question. Suggested options are radios; free text is offered
 * when the platform accepts it. Picking one clears the other, so exactly
 * one answer is ever sent.
 */
export function QuestionForm({ question, projectId }: QuestionFormProps) {
  const id = useId()
  const [choice, setChoice] = useState('')
  const [text, setText] = useState('')
  const answerQuestion = useAnswerQuestion(projectId)

  const answer = text.trim() || choice
  const hasOptions = question.options.length > 0

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!answer) return
    answerQuestion.mutate({ questionId: question.id, answer })
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-4 rounded-lg p-4 ring-1 ring-border">
      <FieldSet>
        <FieldLegend variant="label">{question.prompt}</FieldLegend>
        <FieldDescription>{question.reason}</FieldDescription>
        <FieldGroup className="gap-3">
          {hasOptions ? (
            <RadioGroup
              value={choice}
              onValueChange={(value) => {
                setChoice(value)
                setText('')
              }}
              aria-label={question.prompt}
            >
              {question.options.map((option, index) => (
                <Field key={option} orientation="horizontal">
                  <RadioGroupItem id={`${id}-${index}`} value={option} />
                  <FieldLabel htmlFor={`${id}-${index}`} className="font-normal">
                    {option}
                  </FieldLabel>
                </Field>
              ))}
            </RadioGroup>
          ) : null}
          {question.allowFreeText || !hasOptions ? (
            <Field>
              <FieldLabel htmlFor={`${id}-text`} className="sr-only">
                Your answer
              </FieldLabel>
              <Textarea
                id={`${id}-text`}
                value={text}
                onChange={(event) => {
                  setText(event.target.value)
                  if (event.target.value) setChoice('')
                }}
                placeholder={hasOptions ? 'Or answer in your own words' : 'Your answer'}
                rows={2}
                className="min-h-14 text-sm"
              />
            </Field>
          ) : null}
        </FieldGroup>
      </FieldSet>
      <div className="flex items-center justify-between gap-3">
        <span className="truncate font-mono text-2xs text-muted-foreground">{question.key}</span>
        <Button type="submit" size="sm" loading={answerQuestion.isPending} disabled={!answer}>
          Answer
        </Button>
      </div>
    </form>
  )
}
