-- Intelligence schema: proposals with their ranked options, the negotiation
-- transcript, decision memory and generated documents. Ids that belong to
-- other services (project, pattern, execution) are opaque text: this schema
-- never joins on them.

CREATE TABLE IF NOT EXISTS proposals (
    id                    uuid PRIMARY KEY,
    project_id            text        NOT NULL,
    kind                  text        NOT NULL,
    status                text        NOT NULL,
    title                 text        NOT NULL,
    summary               text        NOT NULL DEFAULT '',
    prompt                text        NOT NULL DEFAULT '',
    recommended_option_id text        NOT NULL DEFAULT '',
    accepted_option_id    text        NOT NULL DEFAULT '',
    execution_id          text        NOT NULL DEFAULT '',
    options               jsonb       NOT NULL DEFAULT '[]'::jsonb,
    constraints           jsonb       NOT NULL DEFAULT '[]'::jsonb,
    weights               jsonb       NOT NULL,
    revision              integer     NOT NULL DEFAULT 1,
    reasoner              text        NOT NULL DEFAULT 'heuristic',
    created_at            timestamptz NOT NULL DEFAULT now(),
    updated_at            timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS proposals_project_idx ON proposals (project_id, created_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    id                     uuid PRIMARY KEY,
    proposal_id            uuid        NOT NULL REFERENCES proposals (id) ON DELETE CASCADE,
    role                   text        NOT NULL,
    content                text        NOT NULL,
    changed_recommendation boolean     NOT NULL DEFAULT false,
    "references"           jsonb       NOT NULL DEFAULT '[]'::jsonb,
    created_at             timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS messages_proposal_idx ON messages (proposal_id, created_at);

CREATE TABLE IF NOT EXISTS decisions (
    id            uuid PRIMARY KEY,
    project_id    text        NOT NULL DEFAULT '',
    scope         text        NOT NULL,
    status        text        NOT NULL DEFAULT 'active',
    source        text        NOT NULL,
    title         text        NOT NULL,
    statement     text        NOT NULL DEFAULT '',
    reasons       text[]      NOT NULL DEFAULT '{}',
    approved_by   text        NOT NULL DEFAULT '',
    proposal_id   text        NOT NULL DEFAULT '',
    tags          text[]      NOT NULL DEFAULT '{}',
    created_at    timestamptz NOT NULL DEFAULT now(),
    superseded_at timestamptz,
    superseded_by text        NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS decisions_project_idx ON decisions (project_id, status);

CREATE TABLE IF NOT EXISTS documents (
    id           uuid PRIMARY KEY,
    project_id   text        NOT NULL,
    kind         text        NOT NULL,
    title        text        NOT NULL,
    markdown     text        NOT NULL,
    commit_sha   text        NOT NULL DEFAULT '',
    facts_used   integer     NOT NULL DEFAULT 0,
    generated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS documents_project_idx ON documents (project_id, generated_at DESC);
