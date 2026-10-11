-- Code browser (CONTRACT.md §15): GitHub's recursive tree and single files,
-- cached per scanned commit. Both tables hold at most the latest commit's
-- rows per project; a successful scan at a new commit prunes the rest.

-- entries: [{path, is_dir, size}]; name and language derive from the path.
CREATE TABLE repo_trees (
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    commit_sha text NOT NULL,
    entries    jsonb NOT NULL DEFAULT '[]',
    truncated  boolean NOT NULL DEFAULT false,
    fetched_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, commit_sha)
);

-- content is empty for binary files ("binary" is quoted: a reserved word); truncated marks a cut at the size
-- limit; size is the file's real size on GitHub when known.
CREATE TABLE repo_files (
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    commit_sha text NOT NULL,
    path       text NOT NULL,
    content    text NOT NULL DEFAULT '',
    "binary"   boolean NOT NULL DEFAULT false,
    truncated  boolean NOT NULL DEFAULT false,
    size       bigint NOT NULL DEFAULT 0,
    language   text NOT NULL DEFAULT '',
    fetched_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, commit_sha, path)
);
