import { ChevronDownIcon, SparklesIcon } from 'lucide-react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Spinner } from '@/components/ui/spinner'

import { useGenerateDocument } from '../../api/mutations'
import { DOCUMENT_KIND_LABEL, DOCUMENT_KINDS, type DocumentKind } from '../../lib/labels'

interface GenerateMenuProps {
  projectId: string
  onGenerated: (documentId: string) => void
}

/** "Generate" dropdown of the eight kinds. One runs at a time. */
export function GenerateMenu({ projectId, onGenerated }: GenerateMenuProps) {
  const generate = useGenerateDocument(projectId)
  const pendingKind: DocumentKind | undefined = generate.isPending ? generate.variables : undefined

  const run = (kind: DocumentKind) => {
    generate.mutate(kind, {
      onSuccess: (document) => {
        onGenerated(document.id)
        toast.success(`${DOCUMENT_KIND_LABEL[kind]} generated`, {
          description: `${document.factsUsed} facts used.`,
        })
      },
    })
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button size="sm" loading={generate.isPending}>
          <SparklesIcon data-icon="inline-start" />
          {pendingKind ? `Generating ${DOCUMENT_KIND_LABEL[pendingKind].toLowerCase()}` : 'Generate'}
          {pendingKind ? null : <ChevronDownIcon data-icon="inline-end" />}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64">
        <DropdownMenuGroup>
          <DropdownMenuLabel className="text-xs text-muted-foreground">From the knowledge model</DropdownMenuLabel>
          {DOCUMENT_KINDS.map((kind) => (
            <DropdownMenuItem key={kind} disabled={generate.isPending} onSelect={() => run(kind)}>
              {pendingKind === kind ? <Spinner /> : null}
              {DOCUMENT_KIND_LABEL[kind]}
            </DropdownMenuItem>
          ))}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
