-- Indexes matching the list queries' filters and ORDER BY (production
-- hardening). ListExecutions pages by updated_at (CONTRACT.md §21), which
-- the number-ordered indexes of 0003 could not serve; ListApprovals filters
-- by execution without a status.

CREATE INDEX IF NOT EXISTS executions_workspace_updated_idx
    ON executions (workspace_id, updated_at DESC, number DESC);
CREATE INDEX IF NOT EXISTS executions_workspace_project_updated_idx
    ON executions (workspace_id, project_id, updated_at DESC, number DESC);
CREATE INDEX IF NOT EXISTS approvals_workspace_requested_idx
    ON approvals (workspace_id, requested_at DESC);
CREATE INDEX IF NOT EXISTS approvals_workspace_execution_idx
    ON approvals (workspace_id, execution_id, requested_at DESC);
