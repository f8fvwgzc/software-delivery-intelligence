-- SDI's GitOps engine (CONTRACT.md §24.7): the four §5.2 policies of the
-- steps that preview, sync, roll back and watch an application, the
-- `gitops` identity those runs execute as, and `cluster:read` for it and
-- for the `delivery` identity (an `sdi_gitops` environment watches its
-- application). `cluster:write` is granted to nobody: a sync or a rollback
-- always waits for a timed grant.
--
-- For every workspace that already has its §5.2 copy and its identities;
-- new workspaces get the same rows from the seed on first use
-- (db::policies::SEED and db::identities::seed, which list them too). A
-- row a workspace already has keeps its decision.
INSERT INTO policies (workspace_id, action_type, label, risk, decision, editable)
SELECT w.workspace_id, p.action_type, p.label, p.risk, p.decision, p.editable
FROM (SELECT DISTINCT workspace_id FROM policies) w
CROSS JOIN (VALUES
    ('gitops.preview',       'Preview what a sync would change',            'SAFE', 'AUTO',     false),
    ('gitops.sync',          'Sync an application to its cluster',          'HIGH', 'APPROVAL', true),
    ('gitops.rollback',      'Roll an application back to an earlier sync', 'HIGH', 'APPROVAL', true),
    ('gitops.verify_health', 'Wait for an application to be healthy',       'SAFE', 'AUTO',     false)
) AS p (action_type, label, risk, decision, editable)
ON CONFLICT (workspace_id, action_type) DO NOTHING;

INSERT INTO identities (id, workspace_id, name, kind, description)
SELECT gen_random_uuid(), w.workspace_id, 'gitops', 'gitops',
       'Drives SDI''s GitOps engine. Holds cluster read to preview syncs and watch health; every sync and rollback needs a timed cluster:write grant.'
FROM (SELECT DISTINCT workspace_id FROM identities) w
ON CONFLICT (workspace_id, name) DO NOTHING;

INSERT INTO grants (workspace_id, identity_id, permission, expires_at)
SELECT i.workspace_id, i.id, 'cluster:read', NULL
FROM identities i
WHERE i.name IN ('delivery', 'gitops')
ON CONFLICT (identity_id, permission) DO UPDATE SET expires_at = NULL;
