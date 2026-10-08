import { useQueryClient } from '@tanstack/react-query'
import { FileTextIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { RelativeTime } from '@/components/shared/data'
import { EmptyState } from '@/components/shared/feedback'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { documentQuery } from '../../api/queries'
import { DOCUMENT_KIND_SHORT } from '../../lib/labels'

type Document = components['schemas']['Document']

interface DocumentListProps {
  documents: Document[]
  selectedId?: string
  onSelect: (documentId: string) => void
  emptyAction?: ReactNode
}

/** Left column: every generated document, newest first. */
export function DocumentList({ documents, selectedId, onSelect, emptyAction }: DocumentListProps) {
  const queryClient = useQueryClient()

  if (documents.length === 0) {
    return (
      <EmptyState
        icon={FileTextIcon}
        title="Nothing generated yet"
        description="Documents are written from the facts the scan collected, and name how many they used."
        action={emptyAction}
      />
    )
  }

  const sorted = documents.toSorted((a, b) => b.generatedAt.localeCompare(a.generatedAt))

  return (
    <nav aria-label="Documents">
      <ol className="flex flex-col divide-y divide-border/60 rounded-lg bg-card ring-1 ring-border">
        {sorted.map((document) => {
          const selected = document.id === selectedId
          const prefetch = () => queryClient.prefetchQuery(documentQuery(document.id))
          return (
            <li key={document.id}>
              <button
                type="button"
                aria-current={selected ? 'true' : undefined}
                onClick={() => onSelect(document.id)}
                onMouseEnter={prefetch}
                onFocus={prefetch}
                className={cn(
                  'flex w-full flex-col gap-1 px-3 py-2.5 text-left outline-none transition-colors duration-100 first:rounded-t-lg last:rounded-b-lg hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset',
                  selected && 'bg-brand/5 shadow-[inset_2px_0_0_0_var(--brand)]',
                )}
              >
                <span className="flex items-center gap-2">
                  <span className="shrink-0 text-2xs text-muted-foreground">{DOCUMENT_KIND_SHORT[document.kind]}</span>
                  <span className="truncate text-sm font-medium">{document.title}</span>
                </span>
                <span className="flex items-center gap-2 text-2xs text-muted-foreground">
                  <RelativeTime value={document.generatedAt} className="font-mono" />
                  <span aria-hidden="true">·</span>
                  <span className="tabular-nums">{document.factsUsed} facts</span>
                </span>
              </button>
            </li>
          )
        })}
      </ol>
    </nav>
  )
}
