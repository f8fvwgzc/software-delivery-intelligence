import { KeyRoundIcon } from 'lucide-react'

import { EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Empty } from '@/components/ui/empty'

import { TOKEN_ENV_VAR, TOKEN_FALLBACK_COMMAND } from '../../lib/labels'

/** Shown when the knowledge service has no token: the exact variable and the dev fallback. */
export function TokenMissing() {
  return (
    <Empty className="items-start p-0 text-left">
      <EmptyHeader className="items-start">
        <EmptyMedia variant="icon">
          <KeyRoundIcon />
        </EmptyMedia>
        <EmptyTitle>GitHub token not configured</EmptyTitle>
        <EmptyDescription className="text-left">
          The knowledge service reads <code className="rounded-sm bg-muted px-1 py-0.5 text-2xs">{TOKEN_ENV_VAR}</code> at
          start. Set it to a token with the <code className="rounded-sm bg-muted px-1 py-0.5 text-2xs">repo</code> scope
          and restart the service.
        </EmptyDescription>
        <EmptyDescription className="text-left">
          For local development, <code className="rounded-sm bg-muted px-1 py-0.5 text-2xs">scripts/dev.sh</code> exports{' '}
          <code className="rounded-sm bg-muted px-1 py-0.5 text-2xs">{TOKEN_FALLBACK_COMMAND}</code> automatically when the
          GitHub CLI is signed in.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  )
}
