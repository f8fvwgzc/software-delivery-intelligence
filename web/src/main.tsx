import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { App } from '@/app/app'

import './index.css'

// Long-task and request-budget instrumentation; dropped from production builds.
if (import.meta.env.DEV) {
  void import('@/lib/perf/dev-perf').then((module) => module.installDevPerf())
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
