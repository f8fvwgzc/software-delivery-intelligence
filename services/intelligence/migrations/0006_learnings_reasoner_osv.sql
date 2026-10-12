-- Per-project review memory, reasoner provider settings and usage, and the
-- OSV vulnerability cache (CONTRACT.md §12, §19, §20).

-- What each project taught the review: how often a rule's findings were
-- accepted or dismissed. Read by the rule engine before every analysis.
CREATE TABLE IF NOT EXISTS review_learnings (
    workspace_id uuid        NOT NULL,
    project_id   text        NOT NULL,
    rule         text        NOT NULL,
    accepted     integer     NOT NULL DEFAULT 0,
    dismissed    integer     NOT NULL DEFAULT 0,
    last_note    text        NOT NULL DEFAULT '',
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, project_id, rule)
);

-- The reasoner is any LLM behind a provider: "anthropic" (Messages API) or
-- "openai_compatible" (chat completions at `reasoner_base_url`). An empty
-- base URL means the provider's public endpoint. The daily cap is in tokens
-- (input + output), per workspace, UTC days.
ALTER TABLE settings ADD COLUMN IF NOT EXISTS reasoner_provider text NOT NULL DEFAULT '';
ALTER TABLE settings ADD COLUMN IF NOT EXISTS reasoner_base_url text NOT NULL DEFAULT '';
ALTER TABLE settings ADD COLUMN IF NOT EXISTS reasoner_daily_token_cap bigint NOT NULL DEFAULT 200000;

-- Tokens spent per workspace and day; one row per (workspace, day).
CREATE TABLE IF NOT EXISTS reasoner_usage (
    workspace_id  uuid        NOT NULL,
    day           date        NOT NULL,
    input_tokens  bigint      NOT NULL DEFAULT 0,
    output_tokens bigint      NOT NULL DEFAULT 0,
    calls         integer     NOT NULL DEFAULT 0,
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, day)
);

-- OSV answers per (ecosystem, name, version), shared by every workspace:
-- a vulnerability is a fact about a package, not about a tenant. Rows older
-- than the TTL (24 h) are refreshed on the next lookup.
CREATE TABLE IF NOT EXISTS osv_cache (
    ecosystem  text        NOT NULL,
    name       text        NOT NULL,
    version    text        NOT NULL,
    -- [{id, summary, severity, fixed}]
    vulns      jsonb       NOT NULL DEFAULT '[]'::jsonb,
    fetched_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ecosystem, name, version)
);
