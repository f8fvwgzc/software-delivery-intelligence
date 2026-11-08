import { RowList } from '@/components/system/row'
import { Tag } from '@/components/system/tag'

import type { InspectedBinary } from '../../api/types'
import { HARDENING_LABELS, hardeningTone, languageLabel } from '../../lib/inspection'

/** Each binary with its toolchain, its embedded dependencies and its hardening, every state with the bytes it was read from. */
export function BinariesList({ binaries }: { binaries: InspectedBinary[] }) {
  if (binaries.length === 0) {
    return <p className="text-xs text-muted-foreground">No application binary in the image (an interpreted stack, or only the distribution’s own tools).</p>
  }
  return (
    <RowList aria-label="Binaries">
      {binaries.map((binary) => (
        <li key={binary.path} className="flex flex-col gap-1.5 px-3 py-2.5">
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            <code className="min-w-0 truncate font-mono text-xs font-medium" title={binary.path}>
              {binary.path}
            </code>
            <Tag variant="outline">{languageLabel(binary)}</Tag>
            {binary.application ? null : <Tag variant="outline">third party</Tag>}
          </div>
          <p className="text-2xs text-muted-foreground">
            {binary.machine}, {binary.kind.replaceAll('_', ' ')}
            {binary.mainModule ? `, ${binary.mainModule}` : ''}
            {binary.embeddedDependencies ? `, ${binary.dependencyCount} embedded dependencies` : ''}
            {binary.buildSettings?.['vcs.revision'] ? `, built from ${binary.buildSettings['vcs.revision'].slice(0, 7)}` : ''}
          </p>
          <ul className="flex flex-wrap gap-1" aria-label={`Hardening of ${binary.path}`}>
            {binary.hardening
              .filter((row) => row.state !== 'not_applicable' && HARDENING_LABELS[row.property])
              .map((row) => (
                <li key={row.property}>
                  <Tag
                    tone={hardeningTone(row.property, row.state)}
                    title={`${row.state.replaceAll('_', ' ')} (${row.confidence}): ${row.basis}`}
                  >
                    {HARDENING_LABELS[row.property]} {row.state === 'enabled' ? 'on' : row.state.replaceAll('_', ' ')}
                  </Tag>
                </li>
              ))}
          </ul>
        </li>
      ))}
    </RowList>
  )
}
