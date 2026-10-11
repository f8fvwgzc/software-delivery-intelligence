import { useLocation } from 'react-router'

import { readReturnTo } from '../lib/return-to'
import { LoginCard } from '../components/login-card'

/** `/login`, outside the shell. The guard sends signed-in users straight to `returnTo`. */
export default function LoginPage() {
  const location = useLocation()
  const returnTo = readReturnTo(location.search)

  return (
    <main className="flex min-h-svh items-center justify-center bg-sidebar px-4 py-10">
      <LoginCard returnTo={returnTo} />
    </main>
  )
}
