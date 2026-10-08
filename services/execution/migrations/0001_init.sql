-- Execution schema. Runs against a pool whose search_path is "execution, public".
-- Enum-like columns are stored as the proto name without its prefix
-- ("PLANNING", "READY", "APPROVAL"), so the rows read naturally in psql.

CREATE TABLE IF NOT EXISTS identities (
    id          uuid PRIMARY KEY,
    name        text NOT NULL UNIQUE,
    kind        text NOT NULL,
    description text NOT NULL DEFAULT ''
);

-- expires_at NULL = part of the identity's base scope. Timed grants are
-- upserted so repeated requests extend rather than duplicate.
CREATE TABLE IF NOT EXISTS grants (
    identity_id uuid NOT NULL REFERENCES identities (id) ON DELETE CASCADE,
    permission  text NOT NULL,
    expires_at  timestamptz,
    PRIMARY KEY (identity_id, permission)
);

CREATE TABLE IF NOT EXISTS policies (
    action_type text PRIMARY KEY,
    label       text NOT NULL,
    description text NOT NULL DEFAULT '',
    risk        text NOT NULL,
    decision    text NOT NULL,
    editable    boolean NOT NULL DEFAULT false,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS executions (
    id              uuid PRIMARY KEY,
    number          bigserial NOT NULL UNIQUE,
    project_id      text NOT NULL DEFAULT '',
    project_name    text NOT NULL DEFAULT '',
    proposal_id     text NOT NULL DEFAULT '',
    pattern_id      text NOT NULL DEFAULT '',
    title           text NOT NULL,
    status          text NOT NULL,
    adapter         text NOT NULL,
    identity_id     uuid NOT NULL REFERENCES identities (id),
    -- Dry-run fault injection requested at start; cleared once it fired.
    fault           jsonb,
    -- Present while paused on a failure.
    failure         jsonb,
    last_checkpoint text,
    revision        integer NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    started_at      timestamptz,
    finished_at     timestamptz
);

CREATE INDEX IF NOT EXISTS executions_project_idx ON executions (project_id, number DESC);
CREATE INDEX IF NOT EXISTS executions_status_idx ON executions (status);

CREATE TABLE IF NOT EXISTS steps (
    id                   uuid PRIMARY KEY,
    execution_id         uuid NOT NULL REFERENCES executions (id) ON DELETE CASCADE,
    key                  text NOT NULL,
    position             integer NOT NULL,
    title                text NOT NULL,
    description          text NOT NULL DEFAULT '',
    action_type          text NOT NULL,
    adapter              text NOT NULL,
    depends_on           text[] NOT NULL DEFAULT '{}',
    status               text NOT NULL,
    risk                 text NOT NULL,
    policy_decision      text NOT NULL,
    required_permissions text[] NOT NULL DEFAULT '{}',
    approval_id          uuid,
    params               jsonb NOT NULL DEFAULT '{}'::jsonb,
    verification         text NOT NULL DEFAULT '',
    output               text NOT NULL DEFAULT '',
    checkpointed         boolean NOT NULL DEFAULT false,
    attempts             integer NOT NULL DEFAULT 0,
    started_at           timestamptz,
    finished_at          timestamptz,
    UNIQUE (execution_id, key)
);

CREATE TABLE IF NOT EXISTS events (
    execution_id uuid NOT NULL REFERENCES executions (id) ON DELETE CASCADE,
    seq          bigint NOT NULL,
    type         text NOT NULL,
    step_key     text NOT NULL DEFAULT '',
    message      text NOT NULL DEFAULT '',
    status       text NOT NULL,
    data         jsonb NOT NULL DEFAULT '{}'::jsonb,
    at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (execution_id, seq)
);

CREATE TABLE IF NOT EXISTS approvals (
    id           uuid PRIMARY KEY,
    execution_id uuid NOT NULL REFERENCES executions (id) ON DELETE CASCADE,
    -- Approvals belong to one plan revision; a replan expires the pending ones.
    revision     integer NOT NULL,
    step_key     text NOT NULL,
    reason       text NOT NULL,
    status       text NOT NULL,
    decided_by   text NOT NULL DEFAULT '',
    note         text NOT NULL DEFAULT '',
    requested_at timestamptz NOT NULL DEFAULT now(),
    decided_at   timestamptz
);

CREATE INDEX IF NOT EXISTS approvals_status_idx ON approvals (status, requested_at DESC);
CREATE INDEX IF NOT EXISTS approvals_execution_idx ON approvals (execution_id);

-- CONTRACT.md §5.3: the simulated identity only holds the harmless scopes.
INSERT INTO identities (id, name, kind, description) VALUES
    ('00000000-0000-7000-8000-000000000001', 'simulated', 'simulated',
     'Dry-run identity. Holds read and build scopes only; everything else needs a timed grant.')
ON CONFLICT (name) DO NOTHING;

INSERT INTO grants (identity_id, permission, expires_at) VALUES
    ('00000000-0000-7000-8000-000000000001', 'repository:read', NULL),
    ('00000000-0000-7000-8000-000000000001', 'container:build', NULL),
    ('00000000-0000-7000-8000-000000000001', 'environment:write', NULL)
ON CONFLICT DO NOTHING;

-- CONTRACT.md §5.2. Rows with editable=false can never be relaxed.
INSERT INTO policies (action_type, label, risk, decision, editable) VALUES
    ('repo.read',                 'Read repository',                        'SAFE',    'AUTO',     false),
    ('repo.analyze',              'Analyze repository',                     'SAFE',    'AUTO',     false),
    ('tests.run_local',           'Run tests locally',                      'SAFE',    'AUTO',     true),
    ('docs.generate',             'Generate documentation',                 'SAFE',    'AUTO',     false),
    ('deps.install',              'Install dependencies',                   'MEDIUM',  'AUTO',     true),
    ('container.build',           'Build container image',                  'MEDIUM',  'AUTO',     true),
    ('container.run',             'Run container',                          'MEDIUM',  'APPROVAL', true),
    ('env.create_temporary',      'Create temporary environment',           'MEDIUM',  'AUTO',     true),
    ('db.create_temporary',       'Create temporary database',              'MEDIUM',  'APPROVAL', true),
    ('cloud.create_dev_resource', 'Create development cloud resource',      'MEDIUM',  'APPROVAL', true),
    ('cloud.modify_resource',     'Modify existing cloud resource',         'HIGH',    'APPROVAL', true),
    ('deploy.staging',            'Deploy to staging',                      'HIGH',    'APPROVAL', true),
    ('deploy.production',         'Deploy to production',                   'HIGH',    'REQUIRED', false),
    ('db.migrate_production',     'Run production database migration',      'HIGH',    'REQUIRED', false),
    ('backup.restore_production', 'Restore production backup',              'HIGH',    'REQUIRED', false),
    ('resource.delete',           'Delete resource',                        'HIGH',    'REQUIRED', false),
    ('firewall.modify',           'Modify firewall',                        'HIGH',    'REQUIRED', false),
    ('secrets.rotate',            'Rotate secrets',                         'HIGH',    'REQUIRED', false),
    ('db.destroy_production',     'Destroy production database',            'BLOCKED', 'BLOCKED',  false)
ON CONFLICT (action_type) DO NOTHING;
