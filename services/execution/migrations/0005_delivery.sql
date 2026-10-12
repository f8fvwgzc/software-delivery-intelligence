-- Continuous delivery (CONTRACT.md §22): where images live (registries),
-- where releases run and how they get there (environments), one immutable
-- image at one commit (releases) and a release going to an environment
-- (deployment_runs, each backed by an execution so approvals, logs,
-- diagnosis and replanning are the ones of §7). Everything is per
-- workspace and every query is scoped by it (§18).
--
-- Secrets never land here: registries and environment configs hold the
-- *names* of workspace secrets, which the identity service resolves when a
-- step runs.

CREATE TABLE IF NOT EXISTS registries (
    id              uuid PRIMARY KEY,
    workspace_id    uuid NOT NULL,
    name            text NOT NULL,
    -- ghcr | dockerhub | acr | ecr | gar | generic
    kind            text NOT NULL,
    host            text NOT NULL,
    namespace       text NOT NULL DEFAULT '',
    -- github_token | oidc | secret
    auth            text NOT NULL,
    username_secret text NOT NULL DEFAULT '',
    password_secret text NOT NULL DEFAULT '',
    is_default      boolean NOT NULL DEFAULT false,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    -- Also serves ListRegistries (WHERE workspace_id ORDER BY name).
    UNIQUE (workspace_id, name)
);

-- Only one default registry per workspace.
CREATE UNIQUE INDEX IF NOT EXISTS registries_one_default_idx
    ON registries (workspace_id) WHERE is_default;

CREATE TABLE IF NOT EXISTS releases (
    id              uuid PRIMARY KEY,
    workspace_id    uuid NOT NULL,
    project_id      text NOT NULL,
    version         text NOT NULL,
    commit_sha      text NOT NULL,
    -- host/namespace/name, without tag.
    image           text NOT NULL,
    tag             text NOT NULL,
    digest          text NOT NULL DEFAULT '',
    -- pipeline | package | manual
    source          text NOT NULL DEFAULT 'manual',
    pipeline_run_id text NOT NULL DEFAULT '',
    notes           text NOT NULL DEFAULT '',
    verified        boolean NOT NULL DEFAULT false,
    created_by      text NOT NULL DEFAULT '',
    -- What the release was cut from, kept so a promotion (also an automatic
    -- one, hours later) needs no call to the knowledge service.
    repo_full_name  text NOT NULL DEFAULT '',
    project_name    text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    -- CreateRelease is idempotent on (project, tag).
    UNIQUE (workspace_id, project_id, tag)
);

-- ListReleases: newest first within a project, paged by created_at.
CREATE INDEX IF NOT EXISTS releases_workspace_project_created_idx
    ON releases (workspace_id, project_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS environments (
    id                 uuid PRIMARY KEY,
    workspace_id       uuid NOT NULL,
    project_id         text NOT NULL,
    name               text NOT NULL,
    -- Promotion order, 0 first.
    position           integer NOT NULL DEFAULT 0,
    -- github_actions | gitops_flux | gitops_argocd | jenkins | azure_devops | webhook | docker_local
    kind               text NOT NULL,
    config             jsonb NOT NULL DEFAULT '{}'::jsonb,
    url                text NOT NULL DEFAULT '',
    health_path        text NOT NULL DEFAULT '',
    -- NULL uses the workspace default registry.
    registry_id        uuid REFERENCES registries (id) ON DELETE SET NULL,
    protected          boolean NOT NULL DEFAULT false,
    auto_promote       boolean NOT NULL DEFAULT false,
    -- ui | config (declared in .sdi.yml, replaced by SyncEnvironments)
    source             text NOT NULL DEFAULT 'ui',
    current_release_id uuid REFERENCES releases (id) ON DELETE SET NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, project_id, name)
);

-- ListEnvironments / GetPipeline: a project's stages in promotion order.
CREATE INDEX IF NOT EXISTS environments_workspace_project_position_idx
    ON environments (workspace_id, project_id, position, name);
-- "Which environments run this release" (Release.deployed_to).
CREATE INDEX IF NOT EXISTS environments_current_release_idx
    ON environments (workspace_id, current_release_id) WHERE current_release_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS deployment_runs (
    id                  uuid PRIMARY KEY,
    workspace_id        uuid NOT NULL,
    project_id          text NOT NULL,
    environment_id      uuid NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    environment_name    text NOT NULL,
    release_id          uuid NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    release_version     text NOT NULL,
    -- promote | rollback
    kind                text NOT NULL,
    -- DeploymentStatus without its prefix: PENDING | AWAITING_APPROVAL |
    -- DEPLOYING | VERIFYING | SUCCEEDED | FAILED | CANCELLED. A projection of
    -- the execution's state, written in the runner's own transaction.
    status              text NOT NULL,
    -- The gated execution that carries the steps, approvals and logs.
    execution_id        uuid NOT NULL UNIQUE REFERENCES executions (id) ON DELETE CASCADE,
    external_url        text NOT NULL DEFAULT '',
    previous_release_id uuid,
    started_by          text NOT NULL DEFAULT '',
    reason              text NOT NULL DEFAULT '',
    created_at          timestamptz NOT NULL DEFAULT now(),
    -- Set once the execution is terminal. A run paused on a failure reads
    -- FAILED with finished_at NULL: it still waits for a remediation.
    finished_at         timestamptz
);

-- ListDeploymentRuns: newest first within a project, optionally one
-- environment, paged by created_at.
CREATE INDEX IF NOT EXISTS deployment_runs_workspace_project_created_idx
    ON deployment_runs (workspace_id, project_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS deployment_runs_workspace_environment_created_idx
    ON deployment_runs (workspace_id, environment_id, created_at DESC, id DESC);
-- "Is a deployment to this environment still open" (one at a time).
CREATE INDEX IF NOT EXISTS deployment_runs_open_idx
    ON deployment_runs (workspace_id, environment_id) WHERE finished_at IS NULL;

-- CONTRACT.md §22 policies for every workspace that already has its §5.2
-- copy; new workspaces get them from the seed on first use
-- (db::policies::SEED, which lists the same rows).
INSERT INTO policies (workspace_id, action_type, label, risk, decision, editable)
SELECT w.workspace_id, p.action_type, p.label, p.risk, p.decision, p.editable
FROM (SELECT DISTINCT workspace_id FROM policies) w
CROSS JOIN (VALUES
    ('registry.verify_image', 'Verify the image exists in the registry', 'SAFE',   'AUTO',     false),
    ('deploy.development',    'Deploy to development',                   'MEDIUM', 'AUTO',     true),
    ('deploy.verify_health',  'Verify the deployment is healthy',        'SAFE',   'AUTO',     false),
    ('deploy.record',         'Record the deployment on GitHub',         'SAFE',   'AUTO',     true),
    ('deploy.rollback',       'Roll back an environment',                'HIGH',   'APPROVAL', true)
) AS p (action_type, label, risk, decision, editable)
ON CONFLICT (workspace_id, action_type) DO NOTHING;
