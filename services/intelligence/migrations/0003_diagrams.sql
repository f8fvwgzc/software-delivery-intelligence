-- Diagrams (CONTRACT.md §16): the spec is the source of truth for the
-- editorial renderer; `mermaid` is rendered from it until a user edit makes
-- the Mermaid source authoritative (`source = 'mermaid'`).

CREATE TABLE IF NOT EXISTS diagrams (
    id          uuid PRIMARY KEY,
    project_id  text        NOT NULL,
    kind        text        NOT NULL,
    title       text        NOT NULL,
    explanation text        NOT NULL DEFAULT '',
    spec        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    mermaid     text        NOT NULL DEFAULT '',
    source      text        NOT NULL DEFAULT 'generated',
    commit_sha  text        NOT NULL DEFAULT '',
    revision    integer     NOT NULL DEFAULT 1,
    reasoner    text        NOT NULL DEFAULT 'heuristic',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS diagrams_project_idx ON diagrams (project_id, created_at DESC);
