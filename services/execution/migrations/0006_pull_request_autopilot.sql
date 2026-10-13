-- Pull request autopilot (CONTRACT.md §23): the five §5.2 policies of the
-- steps that commit review fixes, check and merge a pull request, delete
-- its branch and turn on Dependabot. Nothing else is stored for it: these
-- are ordinary executions on the `github` adapter.
--
-- For every workspace that already has its §5.2 copy; new workspaces get
-- the rows from the seed on first use (db::policies::SEED, which lists the
-- same rows). A row a workspace already has keeps its decision.
INSERT INTO policies (workspace_id, action_type, label, risk, decision, editable)
SELECT w.workspace_id, p.action_type, p.label, p.risk, p.decision, p.editable
FROM (SELECT DISTINCT workspace_id FROM policies) w
CROSS JOIN (VALUES
    ('repo.commit_fixes',            'Commit review fixes to a pull request branch',   'MEDIUM', 'APPROVAL', true),
    ('repo.verify_mergeable',        'Check that a pull request can be merged',        'SAFE',   'AUTO',     false),
    ('repo.merge_pull_request',      'Merge a pull request',                           'HIGH',   'APPROVAL', true),
    ('repo.delete_branch',           'Delete a merged branch',                         'MEDIUM', 'AUTO',     true),
    ('repo.enable_security_updates', 'Turn on Dependabot alerts and security updates', 'MEDIUM', 'APPROVAL', true)
) AS p (action_type, label, risk, decision, editable)
ON CONFLICT (workspace_id, action_type) DO NOTHING;
