import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { proposalQuery } from '../api/queries'

/** Breadcrumb label for /proposals/:proposalId — the proposal title. */
export default function ProposalCrumb() {
  const { proposalId = '' } = useParams()
  const { data } = useSuspenseQuery(proposalQuery(proposalId))
  return <>{data.title}</>
}
