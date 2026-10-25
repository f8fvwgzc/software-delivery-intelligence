import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { gitopsQueries } from '../api/queries'

/** Breadcrumb for `/applications/:applicationId`: its name. Shares the page's query, so it costs no request of its own. */
export default function ApplicationCrumb() {
  const { applicationId = '' } = useParams()
  const { data: application } = useSuspenseQuery(gitopsQueries.application(applicationId))

  return <span className="inline-block max-w-64 truncate align-bottom">{application.name}</span>
}
