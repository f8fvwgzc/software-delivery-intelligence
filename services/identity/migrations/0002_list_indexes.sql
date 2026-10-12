-- Indexes matching the list queries (production hardening). ListAudit
-- filters by an action prefix (`LIKE 'x%'`), which the plain btree on
-- (workspace_id, at) cannot narrow; the user's newest live session is the
-- read behind every personal-workspace GitHub call.

CREATE INDEX IF NOT EXISTS audit_workspace_action_at_idx
    ON audit (workspace_id, action text_pattern_ops, at DESC);
CREATE INDEX IF NOT EXISTS sessions_user_live_idx
    ON sessions (user_id, created_at DESC) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS workspaces_owner_idx ON workspaces (owner_user_id) WHERE owner_user_id IS NOT NULL;
