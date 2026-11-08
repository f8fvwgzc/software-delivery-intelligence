import { BadgeCheckIcon, ChevronDownIcon, CircleDashedIcon } from 'lucide-react'
import { useState } from 'react'

import { Person } from '@/components/shared/data/person'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { Row } from '@/components/system/row'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { cn } from '@/lib/utils'

import type { Release } from '../../api/types'
import { releaseSource, shortSha } from '../../lib/labels'
import { CommitLink } from '../commit-link'
import { InspectionTag } from './inspection-tag'
import { ReleaseNotes } from './release-notes'

interface ReleaseRowProps {
  release: Release
  repoUrl?: string
}

/** One release: version and tag, the commit, where it came from, what its image inspection found, where it runs now, and its notes on demand. */
export function ReleaseRow({ release, repoUrl }: ReleaseRowProps) {
  const [open, setOpen] = useState(false)
  // The tag defaults to the short sha, which the row already shows: only a tag of its own is worth a chip.
  const ownTag = release.tag && release.tag !== release.version && release.tag !== shortSha(release.commitSha)

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <Row
        leading={
          release.verified ? (
            <BadgeCheckIcon role="img" aria-label="Image verified in the registry" className="text-success" />
          ) : (
            <CircleDashedIcon role="img" aria-label="Image not verified in the registry yet" className="text-muted-foreground" />
          )
        }
        title={
          <span className="inline-flex min-w-0 items-center gap-2 align-bottom">
            <span className="truncate font-mono font-medium" title={release.version}>
              {release.version}
            </span>
            {ownTag ? (
              <Tag mono variant="outline" title={`${release.image}:${release.tag}`}>
                {release.tag}
              </Tag>
            ) : null}
          </span>
        }
        meta={
          <>
            <CommitLink sha={release.commitSha} repoUrl={repoUrl} />
            <span>{releaseSource(release.source)}</span>
            {release.createdBy ? <Person value={release.createdBy} /> : null}
            <RelativeTime value={release.createdAt} />
          </>
        }
        trailing={
          <>
            <InspectionTag release={release} />
            {release.deployedTo.map((name) => (
              <Tag key={name} tone="success" dot title={`Runs in ${name}`}>
                {name}
              </Tag>
            ))}
            <CollapsibleTrigger asChild>
              <Button variant="ghost" size="icon-xs" aria-label={`${open ? 'Hide' : 'Show'} the notes of ${release.version}`}>
                <ChevronDownIcon className={cn('transition-transform duration-(--motion-fast)', open && 'rotate-180')} />
              </Button>
            </CollapsibleTrigger>
          </>
        }
      />
      <CollapsibleContent className="border-t border-dashed bg-muted/30 px-3 py-2.5 pl-10">
        {/* The content only mounts while open, so a collapsed row never asks for its notes. */}
        <ReleaseNotes release={release} />
      </CollapsibleContent>
    </Collapsible>
  )
}
