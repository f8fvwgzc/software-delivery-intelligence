-- Tenancy (CONTRACT.md §18): every table carries the workspace it belongs
-- to and every query is scoped by it. Rows that predate workspaces go to the
-- bootstrap workspace 00000000-0000-7000-8000-00000000dead, which is what a
-- single-tenant dev install keeps using. Policies and identities become
-- per-workspace: the §5.2 / §5.3 seed is copied for a new workspace the
-- first time it is used (db::policies::seed, db::identities::seed).
--
-- The default is only there to backfill; it is dropped right after so a
-- write without a workspace fails instead of landing in bootstrap.

ALTER TABLE identities
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE identities DROP CONSTRAINT IF EXISTS identities_name_key;
ALTER TABLE identities ADD CONSTRAINT identities_workspace_name_key UNIQUE (workspace_id, name);
ALTER TABLE identities ALTER COLUMN workspace_id DROP DEFAULT;

ALTER TABLE grants
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
UPDATE grants g SET workspace_id = i.workspace_id
    FROM identities i WHERE i.id = g.identity_id AND g.workspace_id <> i.workspace_id;
ALTER TABLE grants ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS grants_workspace_idx ON grants (workspace_id, identity_id);

ALTER TABLE policies
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE policies DROP CONSTRAINT IF EXISTS policies_pkey;
ALTER TABLE policies ADD PRIMARY KEY (workspace_id, action_type);
ALTER TABLE policies ALTER COLUMN workspace_id DROP DEFAULT;

ALTER TABLE executions
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead',
    -- x-sdi-user of the caller that created the run; background work on the
    -- run continues as the workspace's system principal.
    ADD COLUMN IF NOT EXISTS created_by   text NOT NULL DEFAULT '';
ALTER TABLE executions ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS executions_workspace_idx ON executions (workspace_id, number DESC);
CREATE INDEX IF NOT EXISTS executions_workspace_project_idx ON executions (workspace_id, project_id, number DESC);

ALTER TABLE steps
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
UPDATE steps s SET workspace_id = e.workspace_id
    FROM executions e WHERE e.id = s.execution_id AND s.workspace_id <> e.workspace_id;
ALTER TABLE steps ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS steps_workspace_idx ON steps (workspace_id, execution_id, position);

ALTER TABLE events
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
UPDATE events ev SET workspace_id = e.workspace_id
    FROM executions e WHERE e.id = ev.execution_id AND ev.workspace_id <> e.workspace_id;
ALTER TABLE events ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS events_workspace_idx ON events (workspace_id, execution_id, seq);

ALTER TABLE approvals
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
UPDATE approvals a SET workspace_id = e.workspace_id
    FROM executions e WHERE e.id = a.execution_id AND a.workspace_id <> e.workspace_id;
ALTER TABLE approvals ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS approvals_workspace_idx ON approvals (workspace_id, status, requested_at DESC);
