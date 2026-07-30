-- Derived BM25 full-text index over everything saved on this node. The index
-- is rebuilt from the authoritative store whenever the store generation moves
-- and never participates in the record diff, so it can always be dropped and
-- recreated without data loss (ADR-0053).
CREATE VIRTUAL TABLE IF NOT EXISTS stumble_search_index USING fts5(
  submission_id UNINDEXED,
  tenant_id UNINDEXED,
  title,
  url,
  domain,
  description,
  summary,
  tags,
  notes,
  snapshot,
  tokenize = 'porter unicode61'
);
