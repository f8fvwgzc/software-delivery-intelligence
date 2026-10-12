-- Workspace secrets (CONTRACT.md §22): credentials deployers reference by
-- name (a Jenkins token, a webhook signing key, a registry password). The
-- value is sealed with SDI_SESSION_SECRET (AES-256-GCM) like OAuth tokens
-- and reasoner keys; only the name, a 4-character hint and the description
-- ever leave this service through the public API. `ResolveSecret` is the
-- one service-to-service read of the value and every call is audited.

CREATE TABLE IF NOT EXISTS secrets (
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL,
    sealed_value bytea NOT NULL,
    hint         text NOT NULL DEFAULT '',
    description  text NOT NULL DEFAULT '',
    updated_by   text NOT NULL DEFAULT '',
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, name)
);
