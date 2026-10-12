-- Code review (CONTRACT.md §20): one review per (project, pull request,
-- head commit); its findings; and the activity log that feeds the Activity
-- page. Project ids are opaque text like everywhere in this schema.

CREATE TABLE IF NOT EXISTS reviews (
    id                 uuid        PRIMARY KEY,
    workspace_id       uuid        NOT NULL,
    project_id         text        NOT NULL,
    project_name       text        NOT NULL DEFAULT '',
    repo_full_name     text        NOT NULL DEFAULT '',
    pr_number          bigint      NOT NULL,
    pr_title           text        NOT NULL DEFAULT '',
    pr_author          text        NOT NULL DEFAULT '',
    head_sha           text        NOT NULL DEFAULT '',
    base_ref           text        NOT NULL DEFAULT '',
    -- context | analyzing | proposed | published | failed
    status             text        NOT NULL DEFAULT 'context',
    -- [{key, label, status, detail, duration_ms}]
    steps              jsonb       NOT NULL DEFAULT '[]'::jsonb,
    summary            text        NOT NULL DEFAULT '',
    files_reviewed     integer     NOT NULL DEFAULT 0,
    findings_total     integer     NOT NULL DEFAULT 0,
    findings_accepted  integer     NOT NULL DEFAULT 0,
    reasoner           text        NOT NULL DEFAULT 'heuristic',
    published_url      text        NOT NULL DEFAULT '',
    error              text        NOT NULL DEFAULT '',
    requested_by       text        NOT NULL DEFAULT '',
    -- The review of the previous head commit of the same pull request.
    previous_review_id text        NOT NULL DEFAULT '',
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, project_id, pr_number, head_sha)
);
CREATE INDEX IF NOT EXISTS reviews_workspace_idx ON reviews (workspace_id, project_id, created_at DESC);

CREATE TABLE IF NOT EXISTS findings (
    id            uuid        PRIMARY KEY,
    workspace_id  uuid        NOT NULL,
    review_id     uuid        NOT NULL REFERENCES reviews (id) ON DELETE CASCADE,
    position      integer     NOT NULL DEFAULT 0,
    category      text        NOT NULL,
    -- info | low | medium | high | critical
    severity      text        NOT NULL,
    title         text        NOT NULL,
    detail        text        NOT NULL DEFAULT '',
    path          text        NOT NULL DEFAULT '',
    line          integer     NOT NULL DEFAULT 0,
    end_line      integer     NOT NULL DEFAULT 0,
    suggestion    text        NOT NULL DEFAULT '',
    rule          text        NOT NULL DEFAULT '',
    -- pending | accepted | dismissed
    decision      text        NOT NULL DEFAULT 'pending',
    decision_note text        NOT NULL DEFAULT '',
    decided_by    text        NOT NULL DEFAULT '',
    decided_at    timestamptz
);
CREATE INDEX IF NOT EXISTS findings_review_idx ON findings (workspace_id, review_id, position);

CREATE TABLE IF NOT EXISTS review_activity (
    id           uuid        PRIMARY KEY,
    workspace_id uuid        NOT NULL,
    review_id    uuid        NOT NULL,
    project_id   text        NOT NULL,
    project_name text        NOT NULL DEFAULT '',
    pr_number    bigint      NOT NULL,
    -- review.started | review.proposed | finding.accepted | finding.dismissed | review.published | review.failed
    action       text        NOT NULL,
    actor        text        NOT NULL DEFAULT '',
    detail       text        NOT NULL DEFAULT '',
    at           timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS review_activity_workspace_idx ON review_activity (workspace_id, at DESC);
CREATE INDEX IF NOT EXISTS review_activity_project_idx ON review_activity (workspace_id, project_id, at DESC);

-- Webhooks start a review only when the workspace asks for it (default off).
ALTER TABLE settings ADD COLUMN IF NOT EXISTS auto_review boolean NOT NULL DEFAULT false;
