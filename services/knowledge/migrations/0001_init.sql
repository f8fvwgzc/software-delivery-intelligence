-- Knowledge schema. The pool's search_path is `knowledge, public`, so every
-- table lands in `knowledge` while the pgvector extension stays in `public`.
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE projects (
    id               uuid PRIMARY KEY,
    github_repo_id   bigint,
    name             text NOT NULL,
    repo_full_name   text NOT NULL UNIQUE,
    repo_url         text NOT NULL DEFAULT '',
    default_branch   text NOT NULL DEFAULT 'main',
    is_private       boolean NOT NULL DEFAULT false,
    description      text NOT NULL DEFAULT '',
    primary_language text NOT NULL DEFAULT '',
    status           text NOT NULL DEFAULT 'pending',
    mode             text NOT NULL DEFAULT 'observe',
    last_scan_id     uuid,
    last_scanned_at  timestamptz,
    tldr             text NOT NULL DEFAULT '',
    stack            text[] NOT NULL DEFAULT '{}',
    runs_synced_at   timestamptz,
    deployments_synced_at timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE scans (
    id             uuid PRIMARY KEY,
    project_id     uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status         text NOT NULL DEFAULT 'queued',
    commit_sha     text NOT NULL DEFAULT '',
    started_at     timestamptz,
    finished_at    timestamptz,
    files_total    integer NOT NULL DEFAULT 0,
    files_analyzed integer NOT NULL DEFAULT 0,
    error          text NOT NULL DEFAULT '',
    -- [{key, label, status, detail}], in pipeline order.
    stages         jsonb NOT NULL DEFAULT '[]',
    facts_found    integer NOT NULL DEFAULT 0,
    nodes_found    integer NOT NULL DEFAULT 0,
    edges_found    integer NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX scans_project_idx ON scans(project_id, created_at DESC);

CREATE TABLE facts (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        text NOT NULL,
    category    text NOT NULL,
    key         text NOT NULL,
    label       text NOT NULL DEFAULT '',
    value       text NOT NULL DEFAULT '',
    confidence  double precision NOT NULL DEFAULT 1.0,
    source_path text NOT NULL DEFAULT '',
    evidence    text NOT NULL DEFAULT '',
    detected_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);
CREATE INDEX facts_project_idx ON facts(project_id);

CREATE TABLE questions (
    id              uuid PRIMARY KEY,
    project_id      uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key             text NOT NULL,
    prompt          text NOT NULL,
    reason          text NOT NULL DEFAULT '',
    options         text[] NOT NULL DEFAULT '{}',
    allow_free_text boolean NOT NULL DEFAULT true,
    answer          text NOT NULL DEFAULT '',
    answered_at     timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);
CREATE INDEX questions_project_idx ON questions(project_id);

CREATE TABLE understandings (
    project_id   uuid PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    tldr         text NOT NULL DEFAULT '',
    architecture text NOT NULL DEFAULT '',
    stack        text[] NOT NULL DEFAULT '{}',
    -- [{key, title, summary, items}]
    sections     jsonb NOT NULL DEFAULT '[]',
    request_flow text[] NOT NULL DEFAULT '{}',
    commit_sha   text NOT NULL DEFAULT '',
    generated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE graph_nodes (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        text NOT NULL,
    key         text NOT NULL,
    label       text NOT NULL DEFAULT '',
    description text NOT NULL DEFAULT '',
    source_path text NOT NULL DEFAULT '',
    attributes  jsonb NOT NULL DEFAULT '{}',
    UNIQUE (project_id, key)
);
CREATE INDEX graph_nodes_project_idx ON graph_nodes(project_id);

-- Edges are temporal: valid_to IS NULL means "still true".
CREATE TABLE graph_edges (
    id         uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_id  uuid NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    target_id  uuid NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    relation   text NOT NULL,
    category   text NOT NULL,
    valid_from timestamptz NOT NULL DEFAULT now(),
    valid_to   timestamptz,
    attributes jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX graph_edges_project_idx ON graph_edges(project_id, valid_to);

CREATE TABLE dependencies (
    id            uuid PRIMARY KEY,
    project_id    uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ecosystem     text NOT NULL,
    name          text NOT NULL,
    version       text NOT NULL DEFAULT '',
    manifest_path text NOT NULL DEFAULT '',
    scope         text NOT NULL DEFAULT 'runtime'
);
CREATE INDEX dependencies_project_idx ON dependencies(project_id);

CREATE TABLE dependabot (
    project_id         uuid PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    configured         boolean NOT NULL DEFAULT false,
    config_path        text NOT NULL DEFAULT '',
    ecosystems         text[] NOT NULL DEFAULT '{}',
    open_pull_requests integer NOT NULL DEFAULT 0,
    synced_at          timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE pipelines (
    id         uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider   text NOT NULL,
    name       text NOT NULL,
    path       text NOT NULL,
    triggers   text[] NOT NULL DEFAULT '{}',
    -- [{key, name, runs_on, needs, steps}]
    jobs       jsonb NOT NULL DEFAULT '[]'
);
CREATE INDEX pipelines_project_idx ON pipelines(project_id);

CREATE TABLE pipeline_runs (
    id               uuid PRIMARY KEY,
    project_id       uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider_run_id  bigint NOT NULL,
    pipeline_name    text NOT NULL DEFAULT '',
    pipeline_path    text NOT NULL DEFAULT '',
    run_number       bigint NOT NULL DEFAULT 0,
    status           text NOT NULL,
    branch           text NOT NULL DEFAULT '',
    commit_sha       text NOT NULL DEFAULT '',
    commit_message   text NOT NULL DEFAULT '',
    actor            text NOT NULL DEFAULT '',
    event            text NOT NULL DEFAULT '',
    started_at       timestamptz,
    duration_seconds integer NOT NULL DEFAULT 0,
    html_url         text NOT NULL DEFAULT '',
    -- {job, step, summary, suggested_fix}; only for failed runs.
    failure          jsonb,
    synced_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, provider_run_id)
);
CREATE INDEX pipeline_runs_project_idx ON pipeline_runs(project_id, started_at DESC);

CREATE TABLE deployments (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider_id bigint NOT NULL,
    -- github_deployment | workflow_run
    source      text NOT NULL,
    environment text NOT NULL DEFAULT '',
    ref         text NOT NULL DEFAULT '',
    commit_sha  text NOT NULL DEFAULT '',
    status      text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    creator     text NOT NULL DEFAULT '',
    url         text NOT NULL DEFAULT '',
    description text NOT NULL DEFAULT '',
    synced_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, source, provider_id)
);
CREATE INDEX deployments_project_idx ON deployments(project_id, created_at DESC);

-- Ids are slugs: catalog patterns use the CONTRACT.md slug, learned ones
-- `learned:<key>`, so re-seeding and re-scanning upsert instead of duplicating.
CREATE TABLE patterns (
    id                         text PRIMARY KEY,
    name                       text NOT NULL,
    summary                    text NOT NULL DEFAULT '',
    category                   text NOT NULL,
    technologies               text[] NOT NULL DEFAULT '{}',
    cloud                      text NOT NULL DEFAULT 'any',
    runtime                    text NOT NULL DEFAULT '',
    iac                        text NOT NULL DEFAULT '',
    delivery                   text NOT NULL DEFAULT '',
    requires                   text[] NOT NULL DEFAULT '{}',
    scope                      text NOT NULL DEFAULT 'organization',
    source                     text NOT NULL DEFAULT 'learned',
    status                     text NOT NULL DEFAULT 'active',
    estimated_monthly_cost_usd double precision NOT NULL DEFAULT 0,
    security_score             double precision NOT NULL DEFAULT 0.5,
    complexity                 integer NOT NULL DEFAULT 2,
    pros                       text[] NOT NULL DEFAULT '{}',
    cons                       text[] NOT NULL DEFAULT '{}',
    -- [PlanStep]
    plan                       jsonb NOT NULL DEFAULT '[]',
    embedding                  vector(256),
    last_used_at               timestamptz,
    created_at                 timestamptz NOT NULL DEFAULT now(),
    updated_at                 timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX patterns_category_idx ON patterns(category, status);

CREATE TABLE pattern_usage (
    pattern_id   text NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    project_id   uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    deployments  integer NOT NULL DEFAULT 0,
    successes    integer NOT NULL DEFAULT 0,
    incidents    integer NOT NULL DEFAULT 0,
    last_used_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (pattern_id, project_id)
);
CREATE INDEX pattern_usage_project_idx ON pattern_usage(project_id);

-- Semantic memory. project_id is NULL for cross-project knowledge.
CREATE TABLE chunks (
    id         uuid PRIMARY KEY,
    kind       text NOT NULL,
    project_id uuid REFERENCES projects(id) ON DELETE CASCADE,
    ref_id     text NOT NULL DEFAULT '',
    title      text NOT NULL DEFAULT '',
    path       text NOT NULL DEFAULT '',
    content    text NOT NULL,
    embedding  vector(256) NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX chunks_project_idx ON chunks(project_id, kind);
CREATE INDEX chunks_embedding_idx ON chunks USING hnsw (embedding vector_cosine_ops);

CREATE TABLE project_profiles (
    project_id   uuid PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    embedding    vector(256) NOT NULL,
    profile_text text NOT NULL DEFAULT '',
    technologies text[] NOT NULL DEFAULT '{}',
    capabilities text[] NOT NULL DEFAULT '{}',
    updated_at   timestamptz NOT NULL DEFAULT now()
);
