import { KeyValue } from '@/components/shared/data/key-value'
import { StatusBadge } from '@/components/shared/data/status-badge'
import type { components } from '@/lib/api/schema'

import type { IntelligenceSettings } from '../../api/queries'
import { effectiveReasoner } from '../../lib/settings'

type GithubAccount = components['schemas']['GithubAccount']

interface EnvironmentBlockProps {
  intelligence: IntelligenceSettings
  /** Undefined while the account query is still loading or failed; the row then says so. */
  account?: GithubAccount
  accountError?: boolean
}

function Configured({ ok, detail }: { ok: boolean; detail?: string }) {
  return (
    <span className="inline-flex items-center gap-2">
      {detail ? <span className="font-mono text-xs">{detail}</span> : null}
      <StatusBadge tone={ok ? 'success' : 'warning'}>{ok ? 'Configured' : 'Not configured'}</StatusBadge>
    </span>
  )
}

/** Read-only: what the services were started with. Changed in the environment, not here. */
export function EnvironmentBlock({ intelligence, account, accountError = false }: EnvironmentBlockProps) {
  const reasoner = effectiveReasoner(intelligence)
  const fallsBack = reasoner !== intelligence.reasoner

  return (
    <div className="flex flex-col gap-3 rounded-lg p-4 ring-1 ring-border">
      <KeyValue
        items={[
          {
            label: 'GitHub token',
            value: account ? (
              <Configured ok={account.tokenConfigured} detail={account.tokenConfigured ? account.login : undefined} />
            ) : (
              <span className="text-xs text-muted-foreground">{accountError ? 'Could not check' : 'Checking'}</span>
            ),
          },
          { label: 'Anthropic API key', value: <Configured ok={intelligence.apiKeyConfigured} /> },
          {
            label: 'Reasoner in use',
            value: (
              <span className="inline-flex items-center gap-2">
                <span className="font-mono text-xs">{reasoner}</span>
                {fallsBack ? <span className="text-xs text-warning">fallback</span> : null}
              </span>
            ),
          },
        ]}
      />
      <p className="text-xs text-muted-foreground">
        Set in the services' environment (CONTRACT.md §3). The page reflects it; it cannot change it.
      </p>
    </div>
  )
}
