/** Both documents live at the repository root and state what SDI stores (CONTRACT.md §19). */
const REPOSITORY_URL = 'https://github.com/f8fvwgzc/software-delivery-intelligence/blob/main'

const LINKS = [
  { label: 'What we store', href: `${REPOSITORY_URL}/TRUST.md` },
  { label: 'Privacy', href: `${REPOSITORY_URL}/PRIVACY.md` },
] as const

/** Sign-in is where people think about trust; the answers are one click away. */
export function TrustLinks() {
  return (
    <nav aria-label="Trust documents" className="flex items-center justify-center gap-3 text-xs text-muted-foreground">
      {LINKS.map((link, index) => (
        <span key={link.href} className="flex items-center gap-3">
          {index > 0 ? <span aria-hidden>·</span> : null}
          <a href={link.href} target="_blank" rel="noreferrer" className="underline-offset-4 hover:text-foreground hover:underline">
            {link.label}
          </a>
        </span>
      ))}
    </nav>
  )
}
