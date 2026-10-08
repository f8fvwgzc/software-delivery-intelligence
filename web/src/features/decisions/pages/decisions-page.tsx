import { useSuspenseQueries } from '@tanstack/react-query'
import { useState, useTransition } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Field, FieldLabel } from '@/components/ui/field'
import { Switch } from '@/components/ui/switch'
import { projectPickerQuery } from '@/features/proposals/api/queries'

import { decisionsQuery } from '../api/queries'
import { DecisionList } from '../components/decision-list'
import { DecisionListSkeleton } from '../components/decision-list/skeleton'
import { RecordDecisionDialog } from '../components/record-decision-dialog'

/** /knowledge/decisions — the rules every ranking honours. */
export default function DecisionsPage() {
  const [includeSuperseded, setIncludeSuperseded] = useState(false)
  const [isPending, startTransition] = useTransition()

  return (
    <Page>
      <PageHeader
        title="Decisions"
        description="Standing rules the platform honours when it ranks options. Superseded ones stay in the record."
        actions={
          <>
            <Field orientation="horizontal" className="gap-2">
              <Switch
                id="include-superseded"
                size="sm"
                checked={includeSuperseded}
                aria-busy={isPending || undefined}
                onCheckedChange={(checked) => startTransition(() => setIncludeSuperseded(checked))}
              />
              <FieldLabel htmlFor="include-superseded" className="text-xs font-normal text-muted-foreground">
                Include superseded
              </FieldLabel>
            </Field>
            <RecordDecisionDialog />
          </>
        }
      />
      <QueryBoundary fallback={<DecisionListSkeleton />}>
        <DecisionsBody includeSuperseded={includeSuperseded} />
      </QueryBoundary>
    </Page>
  )
}

function DecisionsBody({ includeSuperseded }: { includeSuperseded: boolean }) {
  const [decisions, projects] = useSuspenseQueries({
    queries: [decisionsQuery({ includeSuperseded }), projectPickerQuery()],
  })
  const names = new Map(projects.data.map((project) => [project.id, project.repoFullName]))
  return <DecisionList decisions={decisions.data} projectNames={names} emptyAction={<RecordDecisionDialog />} />
}
