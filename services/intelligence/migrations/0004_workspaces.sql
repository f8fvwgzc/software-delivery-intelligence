-- Tenancy (CONTRACT.md §18): every table carries the workspace it belongs
-- to and every query is scoped by it. Rows that predate workspaces go to the
-- bootstrap workspace 00000000-0000-7000-8000-00000000dead, which is what a
-- single-tenant dev install keeps using.
--
-- The default is only there to backfill; it is dropped right after so a
-- write without a workspace fails instead of landing in bootstrap.

ALTER TABLE proposals
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE proposals ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS proposals_workspace_idx ON proposals (workspace_id, project_id, created_at DESC);

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
UPDATE messages m SET workspace_id = p.workspace_id
    FROM proposals p WHERE p.id = m.proposal_id AND m.workspace_id <> p.workspace_id;
ALTER TABLE messages ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS messages_workspace_idx ON messages (workspace_id, proposal_id, created_at);

ALTER TABLE decisions
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE decisions ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS decisions_workspace_idx ON decisions (workspace_id, status, created_at DESC);

ALTER TABLE documents
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE documents ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS documents_workspace_idx ON documents (workspace_id, project_id, generated_at DESC);

ALTER TABLE diagrams
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE diagrams ALTER COLUMN workspace_id DROP DEFAULT;
CREATE INDEX IF NOT EXISTS diagrams_workspace_idx ON diagrams (workspace_id, project_id, created_at DESC);

-- Settings become one row per workspace: the old single row (id = 1) is the
-- bootstrap workspace's. The `id` column and its CHECK go away.
ALTER TABLE settings
    ADD COLUMN IF NOT EXISTS workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE settings DROP CONSTRAINT IF EXISTS settings_id_check;
ALTER TABLE settings DROP CONSTRAINT IF EXISTS settings_pkey;
ALTER TABLE settings DROP COLUMN IF EXISTS id;
ALTER TABLE settings ADD PRIMARY KEY (workspace_id);
ALTER TABLE settings ALTER COLUMN workspace_id DROP DEFAULT;
