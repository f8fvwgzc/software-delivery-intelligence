import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { projectKeys } from '@/features/projects/api/queries'
import { api, unwrap } from '@/lib/api/client'

interface AnswerVariables {
  questionId: string
  answer: string
}

/**
 * `POST /questions/{id}/answer`. An answer can change what is inferred,
 * so the whole project family is refetched, not only the questions.
 */
export function useAnswerQuestion(projectId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ questionId, answer }: AnswerVariables) =>
      unwrap(
        api.POST('/api/v1/questions/{id}/answer', {
          params: { path: { id: questionId } },
          body: { answer },
        }),
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: projectKeys.detail(projectId) })
      queryClient.invalidateQueries({ queryKey: projectKeys.lists() })
      toast.success('Answer recorded')
    },
  })
}
