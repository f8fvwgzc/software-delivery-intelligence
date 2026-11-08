-- CONTRACT.md §22.1: the inspection of a release's image. The last report
-- an `artifact.inspect` step produced for the release lives on the release
-- (a release is one immutable image, so the report describes it, whichever
-- environment's run produced it); the summary columns keep the release
-- list from reading the report. The report holds file paths, package
-- names and versions, binary metadata and findings: never file contents,
-- and never the values of environment variables or keys it names.
ALTER TABLE releases
    ADD COLUMN IF NOT EXISTS inspection              jsonb,
    -- passed | findings | incomplete ('' = never inspected)
    ADD COLUMN IF NOT EXISTS inspection_status       text NOT NULL DEFAULT '',
    -- info | low | medium | high | critical ('' = no finding)
    ADD COLUMN IF NOT EXISTS inspection_max_severity text NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS inspection_findings     integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS inspected_at            timestamptz;

-- The policy of the step, for every workspace that already has its §5.2
-- copy; new workspaces get it from the seed on first use
-- (db::policies::SEED). Reading an image changes nothing: safe, automatic,
-- and not editable, like `registry.verify_image`.
INSERT INTO policies (workspace_id, action_type, label, risk, decision, editable)
SELECT w.workspace_id, 'artifact.inspect', 'Inspect the shipped image', 'SAFE', 'AUTO', false
FROM (SELECT DISTINCT workspace_id FROM policies) w
ON CONFLICT (workspace_id, action_type) DO NOTHING;
