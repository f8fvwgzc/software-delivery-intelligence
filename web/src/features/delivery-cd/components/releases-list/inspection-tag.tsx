import { useState } from 'react'

import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'

import type { Release } from '../../api/types'
import { inspectionSummary } from '../../lib/inspection'
import { InspectionSheet } from '../inspection-report'

/** The release's last image inspection as a tag; it opens the report. Nothing when the image was never inspected. */
export function InspectionTag({ release }: { release: Release }) {
  const [open, setOpen] = useState(false)
  if (!release.inspection) return null
  const { label, tone } = inspectionSummary(release.inspection)
  return (
    <>
      <Button
        variant="ghost"
        size="xs"
        className="h-auto px-0.5 py-0"
        onClick={() => setOpen(true)}
        aria-label={`Open the inspection of ${release.version}: ${label}`}
      >
        <Tag tone={tone} dot>
          {label}
        </Tag>
      </Button>
      <InspectionSheet release={release} open={open} onOpenChange={setOpen} />
    </>
  )
}
