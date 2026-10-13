-- Pull request autopilot (CONTRACT.md §23): where a pull request's head
-- branch lives and who opened it. A fork cannot be committed to, a bot is
-- not a person, and Dependabot's pull requests are the ones `auto_merge =
-- dependabot` may merge, so the summary carries all three.
--
-- Rows cached before this migration keep the defaults until the next sync
-- of their project rewrites them (the first `ListPullRequests(refresh)`
-- after the upgrade does, for every pull request GitHub still lists).

ALTER TABLE pull_requests
    -- `owner/name` of the head repository; empty when the fork was deleted.
    ADD COLUMN head_repo     text    NOT NULL DEFAULT '',
    ADD COLUMN is_fork       boolean NOT NULL DEFAULT false,
    ADD COLUMN is_bot        boolean NOT NULL DEFAULT false,
    ADD COLUMN is_dependabot boolean NOT NULL DEFAULT false;
