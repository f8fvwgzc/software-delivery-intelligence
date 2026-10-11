-- One row of ranking defaults and reasoner choice (CONTRACT.md §8 settings).
-- `reasoner = 'claude'` means "Claude when ANTHROPIC_API_KEY is set, heuristic
-- otherwise", which keeps the v1 behaviour for an untouched installation.
-- An empty `model` means "whatever ANTHROPIC_MODEL says".

CREATE TABLE IF NOT EXISTS settings (
    id               smallint         PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    default_weights  jsonb            NOT NULL,
    reasoner         text             NOT NULL DEFAULT 'claude',
    model            text             NOT NULL DEFAULT '',
    cost_ceiling_usd double precision NOT NULL DEFAULT 500,
    updated_at       timestamptz      NOT NULL DEFAULT now()
);

INSERT INTO settings (id, default_weights)
VALUES (1, '{"semantic_similarity":0.20,"policy":0.20,"recency":0.10,"success":0.15,"architecture_fit":0.15,"cost":0.10,"security":0.10}'::jsonb)
ON CONFLICT (id) DO NOTHING;
