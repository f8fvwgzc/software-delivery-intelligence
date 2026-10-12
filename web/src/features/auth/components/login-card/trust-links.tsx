import { Link } from 'react-router'

import { routes } from '@/app/router/routes'

/** Both documents live at the repository root and state what SDI stores (CONTRACT.md §19); the app renders them itself. */
const LINKS = [
  { label: 'What we store', to: routes.trustDoc('trust') },
  { label: 'Privacy', to: routes.trustDoc('privacy') },
] as const

/** Sign-in is where people think about trust; the answers are one click away. */
export function TrustLinks() {
  return (
    <nav aria-label="Trust documents" className="flex items-center justify-center gap-3 text-xs text-muted-foreground">
      {LINKS.map((link, index) => (
        <span key={link.to} className="flex items-center gap-3">
          {index > 0 ? <span aria-hidden>·</span> : null}
          <Link to={link.to} className="underline-offset-4 hover:text-foreground hover:underline">
            {link.label}
          </Link>
        </span>
      ))}
    </nav>
  )
}
