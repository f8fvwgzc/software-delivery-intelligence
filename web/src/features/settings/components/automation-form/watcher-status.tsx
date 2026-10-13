import { RelativeTime } from '@/components/shared/data/relative-time'
import { InlineCode } from '@/components/system/code'
import { TagDot } from '@/components/system/tag'

import type { IntelligenceSettings } from '../../api/queries'

type WatcherSettings = Pick<IntelligenceSettings, 'autoReview' | 'prWatchIntervalSeconds' | 'prWatchLastRunAt' | 'prWatchLastError'>

/**
 * The pull request watcher, read-only (CONTRACT.md §23.1): how often it
 * looks and when it last did, or that the operator turned it off; under
 * that, why its last look failed. It only looks at workspaces that review
 * automatically, so with that switch off the line says so.
 */
export function WatcherStatus({ settings }: { settings: WatcherSettings }) {
  const off = settings.prWatchIntervalSeconds <= 0

  return (
    <div role="status" aria-label="Pull request watcher" className="flex min-w-0 flex-col gap-1 text-xs leading-4 text-muted-foreground">
      <p className="flex items-start gap-2">
        <TagDot tone={off || !settings.autoReview ? 'neutral' : 'success'} className="mt-[5px]" />
        {off ? (
          <span>
            Turned off by the operator (<InlineCode>SDI_PR_WATCH_SECONDS=0</InlineCode>)
          </span>
        ) : (
          <span>
            Looks every <span className="tabular-nums">{settings.prWatchIntervalSeconds} s</span>
            {settings.prWatchLastRunAt ? (
              <>
                , last looked <RelativeTime value={settings.prWatchLastRunAt} className="tabular-nums" />
              </>
            ) : (
              ', has not looked yet'
            )}
            {settings.autoReview ? null : '. It only looks while pull requests are reviewed automatically.'}
          </span>
        )}
      </p>
      {settings.prWatchLastError ? (
        <p className="flex items-start gap-2 text-warning">
          <TagDot tone="warning" className="mt-[5px]" />
          <span>Its last look failed: {settings.prWatchLastError}</span>
        </p>
      ) : null}
    </div>
  )
}
