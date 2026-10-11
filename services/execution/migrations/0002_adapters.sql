-- Phase 2: real adapters. Executions remember what they produced (outputs)
-- and what a pull-request step must commit (artifacts); new policies cover
-- the actions the docker and github adapters run; a second seeded identity
-- holds the scopes local Docker execution needs.
--
-- The github-token identity is NOT seeded here: it exists only while
-- GITHUB_TOKEN is configured, so the service (re)seeds it at startup.

ALTER TABLE executions
    ADD COLUMN IF NOT EXISTS outputs   jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS artifacts jsonb NOT NULL DEFAULT '{}'::jsonb;

-- CONTRACT.md §5.2, phase-2 rows.
INSERT INTO policies (action_type, label, risk, decision, editable) VALUES
    ('repo.checkout',          'Download the repository at a commit',        'SAFE',   'AUTO',     false),
    ('env.destroy_temporary',  'Remove a temporary environment',             'SAFE',   'AUTO',     true),
    ('repo.open_pull_request', 'Open a pull request with generated files',   'MEDIUM', 'APPROVAL', true)
ON CONFLICT (action_type) DO NOTHING;

-- CONTRACT.md §5.3: local-docker adds container:run to the simulated scopes.
INSERT INTO identities (id, name, kind, description) VALUES
    ('00000000-0000-7000-8000-000000000002', 'local-docker', 'docker',
     'Runs steps on the host Docker daemon. Holds read, build, run and temporary-environment scopes; nothing that reaches a cloud or a repository write.')
ON CONFLICT (name) DO NOTHING;

INSERT INTO grants (identity_id, permission, expires_at) VALUES
    ('00000000-0000-7000-8000-000000000002', 'repository:read', NULL),
    ('00000000-0000-7000-8000-000000000002', 'container:build', NULL),
    ('00000000-0000-7000-8000-000000000002', 'container:run', NULL),
    ('00000000-0000-7000-8000-000000000002', 'environment:write', NULL)
ON CONFLICT DO NOTHING;
