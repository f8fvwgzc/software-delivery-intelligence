-- Tenancy (CONTRACT.md §18) and data controls (§19).
--
-- Every tenant table gains `workspace_id uuid NOT NULL` with an index, and
-- every repository function takes the workspace, so ids alone never address
-- a row. Rows that exist before this migration belong to the bootstrap
-- workspace `00000000-0000-7000-8000-00000000dead`: the single-tenant
-- installation that preceded workspaces (dev mode's personal workspace maps
-- to it). The defaults are dropped again so no insert can forget the column.

-- Project-owned tables.
ALTER TABLE projects         ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE scans            ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE facts            ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE questions        ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE understandings   ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE graph_nodes      ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE graph_edges      ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE dependencies     ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE dependabot       ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE pipelines        ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE pipeline_runs    ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE deployments      ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE chunks           ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE project_profiles ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE maturity         ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE project_config   ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE repo_trees       ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE repo_files       ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';
ALTER TABLE pattern_usage    ADD COLUMN workspace_id uuid NOT NULL DEFAULT '00000000-0000-7000-8000-00000000dead';

ALTER TABLE projects         ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE scans            ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE facts            ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE questions        ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE understandings   ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE graph_nodes      ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE graph_edges      ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE dependencies     ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE dependabot       ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE pipelines        ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE pipeline_runs    ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE deployments      ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE chunks           ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE project_profiles ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE maturity         ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE project_config   ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE repo_trees       ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE repo_files       ALTER COLUMN workspace_id DROP DEFAULT;
ALTER TABLE pattern_usage    ALTER COLUMN workspace_id DROP DEFAULT;

CREATE INDEX projects_workspace_idx         ON projects(workspace_id, created_at DESC);
CREATE INDEX scans_workspace_idx            ON scans(workspace_id);
CREATE INDEX facts_workspace_idx            ON facts(workspace_id);
CREATE INDEX questions_workspace_idx        ON questions(workspace_id);
CREATE INDEX understandings_workspace_idx   ON understandings(workspace_id);
CREATE INDEX graph_nodes_workspace_idx      ON graph_nodes(workspace_id);
CREATE INDEX graph_edges_workspace_idx      ON graph_edges(workspace_id);
CREATE INDEX dependencies_workspace_idx     ON dependencies(workspace_id);
CREATE INDEX dependabot_workspace_idx       ON dependabot(workspace_id);
CREATE INDEX pipelines_workspace_idx        ON pipelines(workspace_id);
CREATE INDEX pipeline_runs_workspace_idx    ON pipeline_runs(workspace_id);
CREATE INDEX deployments_workspace_idx      ON deployments(workspace_id);
CREATE INDEX chunks_workspace_idx           ON chunks(workspace_id, kind);
CREATE INDEX project_profiles_workspace_idx ON project_profiles(workspace_id);
CREATE INDEX maturity_workspace_idx         ON maturity(workspace_id);
CREATE INDEX project_config_workspace_idx   ON project_config(workspace_id);
CREATE INDEX repo_trees_workspace_idx       ON repo_trees(workspace_id);
CREATE INDEX repo_files_workspace_idx       ON repo_files(workspace_id);
CREATE INDEX pattern_usage_workspace_idx    ON pattern_usage(workspace_id);

-- The same repository may be connected in a personal workspace and in the
-- organisation's; uniqueness is per workspace now.
ALTER TABLE projects DROP CONSTRAINT projects_repo_full_name_key;
ALTER TABLE projects ADD CONSTRAINT projects_workspace_repo_key UNIQUE (workspace_id, repo_full_name);

-- Patterns: the catalog stays global (`workspace_id IS NULL`,
-- `scope = public_template`); learned patterns belong to the workspace
-- that learned them. Learned ids are `learned:<workspace>:<key>` from now
-- on; the bootstrap workspace keeps its `learned:<key>` rows.
ALTER TABLE patterns ADD COLUMN workspace_id uuid;
UPDATE patterns SET workspace_id = '00000000-0000-7000-8000-00000000dead' WHERE source = 'learned';
UPDATE patterns SET scope = 'public_template' WHERE source = 'catalog';
CREATE INDEX patterns_workspace_idx ON patterns(workspace_id);

-- Settings become per workspace, seeded lazily on first read.
CREATE TABLE workspace_settings (
    workspace_id        uuid PRIMARY KEY,
    scan_max_files      integer NOT NULL DEFAULT 150,
    scan_max_file_bytes integer NOT NULL DEFAULT 200000,
    scan_concurrency    integer NOT NULL DEFAULT 6,
    pipeline_runs_limit integer NOT NULL DEFAULT 50,
    cache_github        boolean NOT NULL DEFAULT true,
    updated_at          timestamptz NOT NULL DEFAULT now()
);
INSERT INTO workspace_settings (workspace_id, scan_max_files, scan_max_file_bytes, scan_concurrency, pipeline_runs_limit, cache_github, updated_at)
SELECT '00000000-0000-7000-8000-00000000dead', scan_max_files, scan_max_file_bytes, scan_concurrency, pipeline_runs_limit, cache_github, updated_at
FROM settings WHERE id = 1;
DROP TABLE settings;

-- Data controls (§19): with `code_cache = session` a cached file carries
-- its expiry; a background sweeper deletes expired rows every ten minutes
-- and reads treat an expired row as a miss. NULL means persistent.
ALTER TABLE repo_files ADD COLUMN expires_at timestamptz;
CREATE INDEX repo_files_expires_idx ON repo_files(expires_at) WHERE expires_at IS NOT NULL;
