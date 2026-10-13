-- Until now every settings save stored the *effective* provider and model as
-- if the workspace had chosen them (the gateway read them and sent them
-- back), which made the workspace deaf to LLM_PROVIDER / LLM_MODEL from then
-- on. Saves no longer do that. Rows that were pinned that way are released:
-- the built-in Anthropic model next to no endpoint of the workspace's own is
-- exactly what an inherited default looks like, and for a workspace that
-- really uses Anthropic the outcome is the same either way.

UPDATE settings
   SET model = ''
 WHERE model = 'claude-sonnet-5-5'
   AND reasoner_base_url = ''
   AND reasoner_provider IN ('', 'anthropic');

UPDATE settings
   SET reasoner_provider = ''
 WHERE reasoner_provider = 'anthropic'
   AND reasoner_base_url = '';
