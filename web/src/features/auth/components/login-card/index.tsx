import { useState } from 'react'

import { Button } from '@/components/ui/button'

import { gatewayLoginUrl } from '../../lib/return-to'
import { GithubMark } from './github-mark'
import { InstallLine } from './install-line'
import { TrustLinks } from './trust-links'

interface LoginCardProps {
  /** Where to land after GitHub; already sanitised. */
  returnTo: string
}

/**
 * Centered utility card (ui-craft recipe-auth): one accent-filled element,
 * the button. Sign-in is a full navigation through the gateway, so the
 * button only has to show that it was pressed.
 */
export function LoginCard({ returnTo }: LoginCardProps) {
  const [leaving, setLeaving] = useState(false)

  const signIn = () => {
    setLeaving(true)
    window.location.assign(gatewayLoginUrl(returnTo))
  }

  return (
    <div className="flex w-full max-w-[360px] flex-col gap-5 rounded-lg bg-background p-5 ring-1 ring-border">
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span aria-hidden className="size-1.5 rounded-full bg-brand" />
          <span className="text-2xs font-medium text-muted-foreground">Software Delivery Intelligence</span>
        </div>
        <h1 className="text-[15px] font-semibold tracking-tight">Sign in with GitHub</h1>
        <p className="text-xs leading-4 text-muted-foreground">
          Identity is GitHub: no passwords, and nobody gets more than GitHub already grants them.
        </p>
      </div>

      <Button type="button" className="w-full" size="lg" loading={leaving} onClick={signIn}>
        <GithubMark data-icon="inline-start" className="size-4" />
        Continue with GitHub
      </Button>

      <div className="flex flex-col gap-2.5 border-t pt-4">
        <InstallLine />
        <TrustLinks />
      </div>
    </div>
  )
}
