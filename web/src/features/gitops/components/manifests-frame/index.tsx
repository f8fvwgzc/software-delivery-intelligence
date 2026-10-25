import { FileCodeIcon } from 'lucide-react'
import { useState } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Frame } from '@/components/system/frame'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'

import { ManifestsBody } from './manifests-body'

interface ManifestsFrameProps {
  applicationId: string
  repoUrl?: string
}

function ManifestsSkeleton() {
  return (
    <div role="status" aria-label="Rendering the manifests" className="flex flex-col gap-2 p-3">
      {[48, 64, 40, 72, 56, 36].map((width, index) => (
        <Skeleton key={index} className="h-3" style={{ width: `${width}%` }} />
      ))}
    </div>
  )
}

/**
 * The desired state as the service renders it. Rendering reads the
 * repository at the source's revision, so nothing is asked for until
 * "Show manifests".
 */
export function ManifestsFrame({ applicationId, repoUrl }: ManifestsFrameProps) {
  const [shown, setShown] = useState(false)

  return (
    <Frame title="Manifests" description="The desired state as rendered, one document per resource." padding="none">
      {shown ? (
        <QueryBoundary compact fallback={<ManifestsSkeleton />}>
          <ManifestsBody applicationId={applicationId} repoUrl={repoUrl} />
        </QueryBoundary>
      ) : (
        <div className="flex flex-wrap items-center justify-between gap-3 p-3">
          <p className="max-w-[70ch] text-xs leading-5 text-muted-foreground">
            Rendered from the repository at the source’s revision, in the order a sync applies them, with every Secret value masked.
          </p>
          <Button variant="outline" size="sm" onClick={() => setShown(true)}>
            <FileCodeIcon data-icon="inline-start" />
            Show manifests
          </Button>
        </div>
      )}
    </Frame>
  )
}
