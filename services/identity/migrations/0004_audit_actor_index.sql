-- ResolveUsers names the people behind stored actor ids. Someone who has left
-- a workspace is still named in its history through the audit trail, and that
-- lookup is by (workspace, actor): without this index it would walk every
-- audit row of the workspace.

CREATE INDEX IF NOT EXISTS audit_workspace_user_idx ON audit (workspace_id, user_id);
