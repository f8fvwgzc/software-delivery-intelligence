-- Pull requests (CONTRACT.md §20): what GitHub lists for a project, cached
-- per project, and the changed files with their parsed unified-diff hunks,
-- cached per head commit. Diffs are code, so the file rows follow the
-- workspace's `code_cache` mode (§19): never written under `none`, carry
-- `expires_at` under `session` (swept with the code cache), kept under
-- `persistent`; the summaries are metadata and are kept until disconnect.

CREATE TABLE pull_requests (
    workspace_id      uuid        NOT NULL,
    project_id        uuid        NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number            bigint      NOT NULL,
    title             text        NOT NULL DEFAULT '',
    body              text        NOT NULL DEFAULT '',
    author            text        NOT NULL DEFAULT '',
    author_avatar_url text        NOT NULL DEFAULT '',
    -- "open" | "closed" | "merged" | "draft"
    state             text        NOT NULL DEFAULT 'open',
    base_ref          text        NOT NULL DEFAULT '',
    head_ref          text        NOT NULL DEFAULT '',
    head_sha          text        NOT NULL DEFAULT '',
    changed_files     integer     NOT NULL DEFAULT 0,
    additions         integer     NOT NULL DEFAULT 0,
    deletions         integer     NOT NULL DEFAULT 0,
    html_url          text        NOT NULL DEFAULT '',
    pr_created_at     timestamptz,
    pr_updated_at     timestamptz,
    synced_at         timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, number)
);
CREATE INDEX pull_requests_workspace_idx ON pull_requests (workspace_id, project_id, pr_updated_at DESC);

-- files: [{path, previous_path, status, additions, deletions, language, hunks: [{old_start, old_lines,
-- new_start, new_lines, lines}], truncated}]; truncated marks a list cut at the file limit.
CREATE TABLE pull_request_files (
    workspace_id uuid        NOT NULL,
    project_id   uuid        NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number       bigint      NOT NULL,
    head_sha     text        NOT NULL,
    files        jsonb       NOT NULL DEFAULT '[]',
    truncated    boolean     NOT NULL DEFAULT false,
    fetched_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz,
    PRIMARY KEY (project_id, number, head_sha)
);
CREATE INDEX pull_request_files_workspace_idx ON pull_request_files (workspace_id);
CREATE INDEX pull_request_files_expires_idx ON pull_request_files (expires_at) WHERE expires_at IS NOT NULL;
