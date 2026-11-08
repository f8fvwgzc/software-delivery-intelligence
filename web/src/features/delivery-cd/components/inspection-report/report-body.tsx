import type { ReactNode } from 'react'

import { Tag } from '@/components/system/tag'

import type { InspectionReport } from '../../api/types'
import { SEVERITIES, severityTone } from '../../lib/inspection'
import { BinariesList } from './binaries-list'
import { ComponentsSummary } from './components-summary'
import { FindingsList } from './findings-list'

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{title}</h3>
      {children}
    </section>
  )
}

/** The whole report: what was read, what was found, what ships, and what could not be read. */
export function ReportBody({ report }: { report: InspectionReport }) {
  const counts = SEVERITIES.map((severity) => [severity, report.findings.filter((f) => f.severity === severity).length] as const).filter(
    ([, n]) => n > 0,
  )
  return (
    <div className="flex flex-col gap-5">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
        <dt className="text-muted-foreground">Image</dt>
        <dd className="truncate font-mono" title={report.image}>
          {report.image}
        </dd>
        <dt className="text-muted-foreground">Digest</dt>
        <dd className="truncate font-mono" title={report.digest}>
          {report.digest || 'unknown'}
        </dd>
        <dt className="text-muted-foreground">Platform</dt>
        <dd>{report.platform || 'unknown'}</dd>
        {report.os ? (
          <>
            <dt className="text-muted-foreground">Distribution</dt>
            <dd>{report.os.prettyName || `${report.os.id} ${report.os.versionId}`}</dd>
          </>
        ) : null}
        <dt className="text-muted-foreground">Read</dt>
        <dd className="tabular-nums">
          {report.layersRead} of {report.layers} layers, {report.filesSeen.toLocaleString()} files
        </dd>
      </dl>

      <Section title="Findings">
        {counts.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {counts.map(([severity, n]) => (
              <Tag key={severity} tone={severityTone(severity)} dot>
                {n} {severity}
              </Tag>
            ))}
          </div>
        ) : null}
        <FindingsList findings={report.findings} />
      </Section>

      <Section title="Binaries">
        <BinariesList binaries={report.binaries} />
      </Section>

      <Section title="What ships">
        <ComponentsSummary components={report.components} />
      </Section>

      {report.tools && report.tools.length > 0 ? (
        <Section title="Second opinion (REA)">
          <ul className="flex flex-col gap-2 text-xs">
            {report.tools.map((run) => (
              <li key={`${run.command}-${run.path}`} className="flex flex-col gap-1">
                <span className="flex items-center gap-1.5">
                  <Tag tone={run.ok ? 'success' : 'warning'} dot>
                    {run.ok ? 'ok' : 'failed'}
                  </Tag>
                  <code className="font-mono">{run.command}</code>
                  <code className="truncate font-mono text-muted-foreground">{run.path}</code>
                </span>
                <pre className="max-h-32 overflow-auto rounded-md bg-muted/40 p-2 font-mono text-2xs whitespace-pre-wrap">{run.summary}</pre>
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      {report.limitations.length > 0 ? (
        <Section title="Not read">
          <ul className="list-disc pl-4 text-xs text-muted-foreground">
            {report.limitations.map((limitation) => (
              <li key={limitation}>{limitation}</li>
            ))}
          </ul>
        </Section>
      ) : null}
    </div>
  )
}
