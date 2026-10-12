-- Indexes matching every list query's filter and ORDER BY (production
-- hardening). Each one is the shape a hot read uses; none changes data.

-- GET /projects/{id}/file: facts detected from one path.
CREATE INDEX IF NOT EXISTS facts_project_path_idx ON facts (project_id, source_path);

-- The `open_questions` counter on every project row, and ListQuestions
-- with only_open: the open ones are the few.
CREATE INDEX IF NOT EXISTS questions_open_idx ON questions (project_id) WHERE answered_at IS NULL;
CREATE INDEX IF NOT EXISTS questions_project_created_idx ON questions (project_id, created_at, key);

-- StartScan's "is one running" check: live scans are the exception.
CREATE INDEX IF NOT EXISTS scans_active_idx ON scans (project_id) WHERE status IN ('queued', 'running');

-- Foreign keys without an index make every node delete scan the edges
-- table (graph sync deletes vanished nodes on each scan).
CREATE INDEX IF NOT EXISTS graph_edges_source_idx ON graph_edges (source_id);
CREATE INDEX IF NOT EXISTS graph_edges_target_idx ON graph_edges (target_id);

-- ListPatterns ordering within a workspace's learned patterns.
CREATE INDEX IF NOT EXISTS patterns_workspace_order_idx ON patterns (workspace_id, source, category, name);

-- ListDependencies and ListPipelines order.
CREATE INDEX IF NOT EXISTS dependencies_project_order_idx ON dependencies (project_id, ecosystem, scope, name);
CREATE INDEX IF NOT EXISTS pipelines_project_path_idx ON pipelines (project_id, path);
