-- Phase 2: delivery maturity, repository configuration, scanner settings,
-- configured facts and the pattern cost model (CONTRACT.md §5.6, §12, §13).

-- Maturity is a report per project, recomputed by every successful scan.
-- axes: [{key, label, score, summary, findings, recommended_patterns}]
CREATE TABLE maturity (
    project_id  uuid PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    overall     integer NOT NULL DEFAULT 0 CHECK (overall BETWEEN 0 AND 100),
    stage       text NOT NULL DEFAULT 'legacy' CHECK (stage IN ('legacy', 'transitional', 'modern')),
    axes        jsonb NOT NULL DEFAULT '[]',
    computed_at timestamptz NOT NULL DEFAULT now()
);

-- What `.sdi.yml` declared at the last scan; `present=false` when the file is
-- missing. Stored as one document because it is read and written as a whole.
CREATE TABLE project_config (
    project_id uuid PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    config     jsonb NOT NULL DEFAULT '{}',
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Scanner limits, one row. `id = 1` makes the single row addressable and the
-- check keeps it single.
CREATE TABLE settings (
    id                  integer PRIMARY KEY CHECK (id = 1),
    scan_max_files      integer NOT NULL DEFAULT 150,
    scan_max_file_bytes integer NOT NULL DEFAULT 200000,
    scan_concurrency    integer NOT NULL DEFAULT 6,
    pipeline_runs_limit integer NOT NULL DEFAULT 50,
    cache_github        boolean NOT NULL DEFAULT true,
    updated_at          timestamptz NOT NULL DEFAULT now()
);
INSERT INTO settings (id) VALUES (1);

-- "user" | "config"; empty while open.
ALTER TABLE questions ADD COLUMN answered_by text NOT NULL DEFAULT '';

-- List views show maturity and the config badge without joining.
ALTER TABLE projects ADD COLUMN maturity_score integer NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN has_config boolean NOT NULL DEFAULT false;

-- Facts gained `configured` (from `.sdi.yml`) and the runtime/configuration
-- categories; the spelling is the proto name without its prefix, lowercased.
ALTER TABLE facts ADD CONSTRAINT facts_kind_check
    CHECK (kind IN ('detected', 'inferred', 'configured'));
ALTER TABLE facts ADD CONSTRAINT facts_category_check
    CHECK (category IN ('repository', 'language', 'framework', 'datastore', 'container', 'ci_cd', 'iac',
                        'cloud', 'testing', 'dependency', 'security', 'documentation', 'deployment',
                        'migration', 'runtime', 'configuration'));

-- Cost model and maturity targets: {cost_lines: [{item, monthly_usd, note}], targets_axes: [...]}.
ALTER TABLE patterns ADD COLUMN attributes jsonb NOT NULL DEFAULT '{}';

-- "Latest scan of a project" is asked on every project page load.
CREATE INDEX scans_project_started_idx ON scans(project_id, started_at DESC);
