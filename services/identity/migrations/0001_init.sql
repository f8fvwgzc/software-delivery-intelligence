-- Identity schema (CONTRACT.md §18, §19). Runs against a pool whose
-- search_path is "identity, public". Role and kind columns hold the proto
-- name without its prefix ("OWNER", "ORGANIZATION") so rows read naturally.

CREATE TABLE IF NOT EXISTS users (
    id          uuid PRIMARY KEY,
    github_host text NOT NULL DEFAULT 'github.com',
    github_id   bigint NOT NULL,
    login       text NOT NULL,
    name        text NOT NULL DEFAULT '',
    avatar_url  text NOT NULL DEFAULT '',
    email       text NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (github_host, github_id)
);

CREATE INDEX IF NOT EXISTS users_login_idx ON users (github_host, lower(login));

-- The cookie carries the raw token; only its keyed hash is stored. The OAuth
-- token GitHub issued is sealed with SDI_SESSION_SECRET (AES-256-GCM).
CREATE TABLE IF NOT EXISTS sessions (
    id           uuid PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash   bytea NOT NULL UNIQUE,
    oauth_token  bytea NOT NULL,
    scopes       text NOT NULL DEFAULT '',
    created_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz NOT NULL,
    revoked_at   timestamptz,
    last_seen_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS sessions_user_idx ON sessions (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS workspaces (
    id            uuid PRIMARY KEY,
    kind          text NOT NULL,
    github_host   text NOT NULL DEFAULT 'github.com',
    slug          text NOT NULL,
    name          text NOT NULL DEFAULT '',
    avatar_url    text NOT NULL DEFAULT '',
    -- The account a PERSONAL workspace belongs to; NULL for organisations.
    owner_user_id uuid REFERENCES users (id) ON DELETE CASCADE,
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (github_host, kind, slug)
);

-- One GitHub App installation = one workspace. `removed_at` is set when the
-- app is uninstalled; the workspace and its data stay until deleted.
CREATE TABLE IF NOT EXISTS installations (
    installation_id      bigint PRIMARY KEY,
    workspace_id         uuid NOT NULL UNIQUE REFERENCES workspaces (id) ON DELETE CASCADE,
    account_login        text NOT NULL,
    account_type         text NOT NULL DEFAULT 'Organization',
    github_host          text NOT NULL DEFAULT 'github.com',
    api_base_url         text NOT NULL DEFAULT 'https://api.github.com',
    repository_selection text NOT NULL DEFAULT 'selected',
    installed_by_user_id uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at           timestamptz NOT NULL DEFAULT now(),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    removed_at           timestamptz
);

-- Synced from GitHub (§18). override_role is NULL or strictly lower than
-- derived_role; the check is in code so the message can say why.
CREATE TABLE IF NOT EXISTS memberships (
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    github_role   text NOT NULL,
    teams         text[] NOT NULL DEFAULT '{}',
    derived_role  text NOT NULL,
    override_role text,
    synced_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);

CREATE INDEX IF NOT EXISTS memberships_user_idx ON memberships (user_id);

-- No foreign key on workspace_id: the entry that records a workspace
-- deletion must outlive the workspace.
CREATE TABLE IF NOT EXISTS audit (
    id           uuid PRIMARY KEY,
    workspace_id uuid NOT NULL,
    user_id      text NOT NULL DEFAULT '',
    user_login   text NOT NULL DEFAULT '',
    action       text NOT NULL,
    entity       text NOT NULL DEFAULT '',
    entity_id    text NOT NULL DEFAULT '',
    detail       text NOT NULL DEFAULT '',
    request_id   text NOT NULL DEFAULT '',
    ip           text NOT NULL DEFAULT '',
    at           timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_workspace_at_idx ON audit (workspace_id, at DESC);

-- One row per workspace; the defaults are the most private ones (§19).
CREATE TABLE IF NOT EXISTS data_controls (
    workspace_id         uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    code_cache           text NOT NULL DEFAULT 'session',
    code_cache_ttl_hours integer NOT NULL DEFAULT 24,
    minimal_chunks       boolean NOT NULL DEFAULT false,
    reasoner             text NOT NULL DEFAULT 'off',
    audit_retention_days integer NOT NULL DEFAULT 365,
    updated_at           timestamptz NOT NULL DEFAULT now()
);

-- Workspace-owned LLM key, sealed with SDI_SESSION_SECRET; only the last
-- four characters are kept in clear for recognition.
CREATE TABLE IF NOT EXISTS reasoner_keys (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    sealed_key   bytea NOT NULL,
    hint         text NOT NULL DEFAULT '',
    updated_at   timestamptz NOT NULL DEFAULT now()
);
