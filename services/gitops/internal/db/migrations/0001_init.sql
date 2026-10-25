-- The gitops schema (CONTRACT.md §24). Every table is per workspace: it
-- carries workspace_id, every query filters on it, and the foreign keys
-- include it, so a row can never point into another workspace.

-- A Kubernetes cluster: a name plus the *name* of a workspace secret that
-- holds its kubeconfig (identity, §22). The kubeconfig itself is never
-- stored here.
CREATE TABLE clusters (
    id                 uuid PRIMARY KEY,
    workspace_id       uuid NOT NULL,
    name               text NOT NULL,
    kubeconfig_secret  text NOT NULL,
    context            text NOT NULL DEFAULT '',
    -- API server URL read from the kubeconfig; shown, never used to connect.
    server             text NOT NULL DEFAULT '',
    default_namespace  text NOT NULL DEFAULT 'default',
    namespaces         text[] NOT NULL DEFAULT '{}',
    status             text NOT NULL DEFAULT 'unknown',
    message            text NOT NULL DEFAULT '',
    server_version     text NOT NULL DEFAULT '',
    checked_at         timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, name)
);
CREATE INDEX clusters_workspace ON clusters (workspace_id);

-- An application: where its desired state comes from, where it goes, how it
-- syncs, and what the last comparison found (statuses, conditions and the
-- resource tree as a JSON snapshot).
CREATE TABLE applications (
    id                 uuid PRIMARY KEY,
    workspace_id       uuid NOT NULL,
    project_id         text NOT NULL,
    name               text NOT NULL,
    cluster_id         uuid NOT NULL,
    namespace          text NOT NULL,
    source             jsonb NOT NULL,
    sync_policy        jsonb NOT NULL,
    sync_status        text NOT NULL DEFAULT 'unknown',
    health_status      text NOT NULL DEFAULT 'unknown',
    health_message     text NOT NULL DEFAULT '',
    -- The commit the last comparison rendered, and the digest of what it
    -- rendered (automation tells a new desired state from drift by it).
    revision           text NOT NULL DEFAULT '',
    desired_digest     text NOT NULL DEFAULT '',
    -- The commit and digest the last successful sync applied.
    synced_revision    text NOT NULL DEFAULT '',
    synced_digest      text NOT NULL DEFAULT '',
    conditions         jsonb NOT NULL DEFAULT '[]',
    resources          jsonb NOT NULL DEFAULT '[]',
    resource_count     integer NOT NULL DEFAULT 0,
    out_of_sync_count  integer NOT NULL DEFAULT 0,
    reconciled_at      timestamptz,
    -- The operation in flight or the last one that ran, and the lease that
    -- makes it exclusive: an operation may only start while no unexpired
    -- lease exists, across every replica.
    operation          jsonb,
    op_lease_until     timestamptz,
    -- Automated sync bookkeeping: the desired digest the failed attempts
    -- were for, how many failed, and when the next attempt may start.
    auto_digest        text NOT NULL DEFAULT '',
    auto_attempts      integer NOT NULL DEFAULT 0,
    auto_next_at       timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, project_id, name),
    FOREIGN KEY (workspace_id, cluster_id) REFERENCES clusters (workspace_id, id)
);
CREATE INDEX applications_workspace ON applications (workspace_id);
CREATE INDEX applications_project ON applications (workspace_id, project_id);
CREATE INDEX applications_cluster ON applications (workspace_id, cluster_id);

-- Every sync that was not a dry run, newest first; the last 50 per
-- application are kept.
CREATE TABLE history (
    id                 uuid PRIMARY KEY,
    workspace_id       uuid NOT NULL,
    application_id     uuid NOT NULL,
    -- "sync" | "rollback"
    kind               text NOT NULL,
    revision           text NOT NULL,
    -- The source as it was synced, so a rollback can sync it again.
    source             jsonb NOT NULL,
    phase              text NOT NULL,
    message            text NOT NULL DEFAULT '',
    initiated_by       text NOT NULL,
    execution_id       text NOT NULL DEFAULT '',
    automated          boolean NOT NULL DEFAULT false,
    started_at         timestamptz NOT NULL,
    finished_at        timestamptz NOT NULL,
    FOREIGN KEY (workspace_id, application_id) REFERENCES applications (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX history_workspace ON history (workspace_id);
CREATE INDEX history_application ON history (workspace_id, application_id, started_at DESC);
