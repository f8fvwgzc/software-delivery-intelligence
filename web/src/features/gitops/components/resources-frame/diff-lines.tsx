import { cn } from '@/lib/utils'

import type { DiffLine } from '../../lib/diff'

const ROW_CLASS: Record<DiffLine['kind'], string> = {
  add: 'bg-success/8',
  remove: 'bg-destructive/8',
  hunk: 'bg-muted/70 text-muted-foreground',
  meta: 'text-muted-foreground italic',
  context: '',
}

const SIGN: Record<DiffLine['kind'], string> = { add: '+', remove: '-', hunk: '', meta: '', context: ' ' }
const SIGN_CLASS: Record<DiffLine['kind'], string> = {
  add: 'text-success',
  remove: 'text-destructive',
  hunk: '',
  meta: '',
  context: 'text-muted-foreground',
}

const GUTTER = 'w-9 shrink-0 select-none pr-2 text-right tabular-nums text-muted-foreground'

/**
 * A unified diff from the live object to the desired one: both line
 * numbers, the sign, and the line. Added lines are what a sync writes,
 * removed lines what it takes away. The signs and numbers are never part of
 * a copy.
 */
export function DiffLines({ lines, label }: { lines: readonly DiffLine[]; label: string }) {
  return (
    <div className="max-h-96 overflow-auto rounded-md ring-1 ring-border ring-inset" style={{ scrollbarGutter: 'stable' }}>
      <pre className="min-w-max py-1 font-mono text-xs leading-5" tabIndex={0} aria-label={label}>
        {lines.map((line, index) => (
          <div key={index} className={cn('flex pr-4', ROW_CLASS[line.kind])}>
            {line.kind === 'hunk' || line.kind === 'meta' ? (
              <code className="pl-[4.75rem] whitespace-pre">{line.text}</code>
            ) : (
              <>
                <span className={GUTTER} aria-hidden>
                  {line.oldLine ?? ''}
                </span>
                <span className={GUTTER} aria-hidden>
                  {line.newLine ?? ''}
                </span>
                <span className={cn('w-4 shrink-0 select-none', SIGN_CLASS[line.kind])} aria-hidden>
                  {SIGN[line.kind]}
                </span>
                {line.kind === 'add' ? <span className="sr-only">Added: </span> : line.kind === 'remove' ? <span className="sr-only">Removed: </span> : null}
                <code className="whitespace-pre">{line.text || ' '}</code>
              </>
            )}
          </div>
        ))}
      </pre>
    </div>
  )
}
