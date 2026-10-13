-- Pull request autopilot (CONTRACT.md §19, §23): whether source lines may be
-- sent to the workspace's model so it can write a fix. `off` (the default,
-- and the most private value) sends nothing; `changed_lines` allows the
-- lines one finding points at plus a few around them. It only has an effect
-- while `reasoner` is not `off`.

ALTER TABLE data_controls
    ADD COLUMN IF NOT EXISTS code_to_model text NOT NULL DEFAULT 'off';
