-- Pull request autopilot (CONTRACT.md §23): the three switches, who wrote a
-- finding's suggestion, the executions the feature creates (commit fixes,
-- merge, turn Dependabot on) and what the watcher last saw per workspace.

-- All off by default: nothing is drafted, committed or merged on its own.
ALTER TABLE settings ADD COLUMN IF NOT EXISTS auto_draft_fixes boolean NOT NULL DEFAULT false;
-- off | dependabot
ALTER TABLE settings ADD COLUMN IF NOT EXISTS auto_merge text NOT NULL DEFAULT 'off';
ALTER TABLE settings ADD COLUMN IF NOT EXISTS merge_requires_review boolean NOT NULL DEFAULT true;

-- rule (the analyzer wrote it) | model (DraftFix asked the workspace's model) | '' (no suggestion).
ALTER TABLE findings ADD COLUMN IF NOT EXISTS suggestion_source text NOT NULL DEFAULT '';
UPDATE findings SET suggestion_source = 'rule' WHERE suggestion <> '' AND suggestion_source = '';
-- The fix execution that carries this suggestion, once one was requested.
ALTER TABLE findings ADD COLUMN IF NOT EXISTS fix_execution_id text NOT NULL DEFAULT '';
-- False when the analysis saw the file as binary, removed or truncated: its
-- lines cannot be replaced. Findings that predate this column count as
-- patchable; the file is read again before anything is drafted or committed.
ALTER TABLE findings ADD COLUMN IF NOT EXISTS patchable boolean NOT NULL DEFAULT true;

-- The latest execution that commits this review's fixes.
ALTER TABLE reviews ADD COLUMN IF NOT EXISTS fix_execution_id text NOT NULL DEFAULT '';

-- One row per execution the autopilot asked for.
CREATE TABLE IF NOT EXISTS autopilot_executions (
    id           uuid        PRIMARY KEY,
    workspace_id uuid        NOT NULL,
    project_id   text        NOT NULL,
    -- fix | merge | dependabot
    kind         text        NOT NULL CHECK (kind IN ('fix', 'merge', 'dependabot')),
    -- 0 for an execution that is not about one pull request (dependabot).
    pr_number    bigint      NOT NULL DEFAULT 0,
    -- The review it came from or belongs to; null when there is none.
    review_id    uuid,
    head_sha     text        NOT NULL DEFAULT '',
    execution_id text        NOT NULL,
    requested_by text        NOT NULL DEFAULT '',
    -- '' while the execution may still be running; succeeded | failed |
    -- cancelled | gone once the execution service said it ended.
    outcome      text        NOT NULL DEFAULT '',
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS autopilot_executions_pr_idx
    ON autopilot_executions (workspace_id, project_id, kind, pr_number, created_at DESC);
CREATE INDEX IF NOT EXISTS autopilot_executions_review_idx
    ON autopilot_executions (workspace_id, review_id, kind, created_at DESC);

-- What the pull request watcher last saw in a workspace.
CREATE TABLE IF NOT EXISTS pr_watch_state (
    workspace_id uuid        PRIMARY KEY,
    last_run_at  timestamptz,
    -- Why the last look failed; '' when it worked.
    last_error   text        NOT NULL DEFAULT '',
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- The watcher asks for the workspaces that switched auto_review on.
CREATE INDEX IF NOT EXISTS settings_auto_review_idx ON settings (workspace_id) WHERE auto_review;
