import { ArrowLeftIcon } from 'lucide-react'
import { Link, useParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { Markdown } from '@/components/shared/data/markdown'
import { EmptyState } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'

// The two documents at the repository root (CONTRACT.md §19), bundled as
// text so the app serves them itself instead of linking to a hosting site.
import privacy from '../../../../../PRIVACY.md?raw'
import trust from '../../../../../TRUST.md?raw'

const DOCS: Record<string, { title: string; body: string }> = {
  trust: { title: 'What we store', body: trust },
  privacy: { title: 'Privacy', body: privacy },
}

/** `/docs/trust` and `/docs/privacy`: readable before signing in, so the login page can point at them. */
export default function TrustDocPage() {
  const { doc = '' } = useParams()
  const document = DOCS[doc]

  return (
    <div className="min-h-svh bg-background">
      <div className="mx-auto flex w-full max-w-[80ch] flex-col gap-4 px-4 py-6 md:px-6 md:py-8">
        <nav aria-label="Trust documents" className="flex flex-wrap items-center gap-2">
          <Button asChild variant="ghost" size="sm">
            <Link to={routes.login()}>
              <ArrowLeftIcon data-icon="inline-start" />
              Back
            </Link>
          </Button>
          <span className="ml-auto flex items-center gap-3 text-xs text-muted-foreground">
            {Object.entries(DOCS).map(([key, entry]) => (
              <Link
                key={key}
                to={routes.trustDoc(key as 'trust' | 'privacy')}
                aria-current={key === doc ? 'page' : undefined}
                className="underline-offset-4 hover:text-foreground hover:underline aria-[current=page]:text-foreground"
              >
                {entry.title}
              </Link>
            ))}
          </span>
        </nav>
        {document ? (
          <Markdown>{document.body}</Markdown>
        ) : (
          <EmptyState
            title="No such document"
            description="The trust documents are “What we store” and “Privacy”."
            action={
              <Button asChild variant="outline" size="sm">
                <Link to={routes.trustDoc('trust')}>Open “What we store”</Link>
              </Button>
            }
          />
        )}
      </div>
    </div>
  )
}
