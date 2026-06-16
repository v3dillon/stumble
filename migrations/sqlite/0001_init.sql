CREATE TABLE node_identity (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  display_name TEXT NOT NULL,
  public_key TEXT NOT NULL,
  private_key_encrypted_or_local TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE trusted_peers (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  display_name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  public_key TEXT NOT NULL,
  trust_level TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

CREATE TABLE hub_registered_nodes (
  node_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  public_key TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  registered_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL
);

CREATE TABLE hub_registered_pods (
  id TEXT PRIMARY KEY,
  node_id TEXT NOT NULL,
  node_base_url TEXT NOT NULL,
  pod_slug TEXT NOT NULL,
  pod_name TEXT NOT NULL,
  description TEXT NOT NULL,
  tags TEXT NOT NULL DEFAULT '[]',
  skill_pack_version INTEGER NOT NULL,
  latest_event_hash TEXT NULL,
  manifest_url TEXT NOT NULL,
  events_url TEXT NOT NULL,
  registered_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX hub_registered_pods_node_slug_idx ON hub_registered_pods (node_id, pod_slug);

CREATE TABLE sync_runs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  peer_id TEXT NOT NULL,
  pod_slug TEXT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT NULL,
  status TEXT NOT NULL,
  imported_events INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE sync_errors (
  id TEXT PRIMARY KEY,
  sync_run_id TEXT NULL,
  peer_id TEXT NULL,
  message TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE tenants (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE tenant_users (
  tenant_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  role TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (tenant_id, user_id)
);

CREATE TABLE api_tokens (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  tenant_id TEXT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  label TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT NULL,
  revoked_at TEXT NULL
);

CREATE TABLE managed_node_identities (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE pods (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  name TEXT NOT NULL,
  slug TEXT NOT NULL,
  description TEXT NOT NULL,
  visibility TEXT NOT NULL,
  created_by TEXT NULL,
  created_at TEXT NOT NULL,
  origin_node_id TEXT NULL
);

CREATE UNIQUE INDEX pods_tenant_slug_idx ON pods (tenant_id, slug);

CREATE TABLE pod_memberships (
  user_id TEXT NOT NULL,
  pod_id TEXT NOT NULL,
  role TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (user_id, pod_id)
);

CREATE TABLE pod_rules (
  pod_id TEXT PRIMARY KEY,
  blocked_topics TEXT NOT NULL DEFAULT '[]',
  blocked_domains TEXT NOT NULL DEFAULT '[]',
  auto_promote_crawler_candidates INTEGER NOT NULL DEFAULT 0,
  federate_sources INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE pod_skill_packs (
  id TEXT PRIMARY KEY,
  pod_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  pod_yaml TEXT NOT NULL,
  skill_md TEXT NOT NULL,
  sources_yaml TEXT NOT NULL,
  filters_yaml TEXT NOT NULL,
  examples_good_md TEXT NOT NULL,
  examples_bad_md TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE event_log (
  event_id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  event_type TEXT NOT NULL,
  pod_slug TEXT NOT NULL,
  author_node_id TEXT NOT NULL,
  author_display_name TEXT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  previous_event_hash TEXT NULL,
  content_hash TEXT NOT NULL UNIQUE,
  signature TEXT NOT NULL,
  imported_from_peer_id TEXT NULL,
  verified INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE submissions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  url TEXT NOT NULL,
  canonical_url TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT NULL,
  domain TEXT NOT NULL,
  submitted_by TEXT NULL,
  discovered_by_crawler INTEGER NOT NULL DEFAULT 0,
  submitter_note TEXT NULL,
  summary TEXT NULL,
  tags TEXT NOT NULL DEFAULT '[]',
  embedding TEXT NULL,
  created_at TEXT NOT NULL,
  origin_event_id TEXT NULL
);

CREATE UNIQUE INDEX submissions_tenant_canonical_idx ON submissions (tenant_id, canonical_url);

CREATE TABLE submission_pods (
  submission_id TEXT NOT NULL,
  pod_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (submission_id, pod_id)
);

CREATE TABLE submission_assets (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  submission_id TEXT NOT NULL,
  asset_type TEXT NOT NULL,
  source TEXT NOT NULL,
  url TEXT NULL,
  local_path TEXT NULL,
  mime_type TEXT NULL,
  alt_text TEXT NULL,
  created_at TEXT NOT NULL,
  CHECK (url IS NOT NULL OR local_path IS NOT NULL)
);

CREATE TABLE source_domains (
  domain TEXT PRIMARY KEY,
  quality_score REAL NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE crawler_sources (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  pod_id TEXT NOT NULL,
  source_type TEXT NOT NULL,
  url TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  crawl_interval_minutes INTEGER NOT NULL DEFAULT 1440,
  last_crawled_at TEXT NULL,
  origin_event_id TEXT NULL
);

CREATE TABLE crawl_runs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  crawler_source_id TEXT NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT NULL,
  status TEXT NOT NULL,
  error TEXT NULL
);

CREATE TABLE crawl_candidates (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  pod_id TEXT NOT NULL,
  crawler_source_id TEXT NOT NULL,
  url TEXT NOT NULL,
  canonical_url TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT NULL,
  domain TEXT NOT NULL,
  summary TEXT NULL,
  tags TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL,
  rejection_reason TEXT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE annotations (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  submission_id TEXT NOT NULL,
  user_id TEXT NULL,
  body TEXT NOT NULL,
  public INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE user_preferences (
  user_id TEXT NOT NULL,
  tenant_id TEXT NULL,
  interests TEXT NOT NULL DEFAULT '[]',
  blocked_topics TEXT NOT NULL DEFAULT '[]',
  blocked_sources TEXT NOT NULL DEFAULT '[]',
  preferred_brief_length INTEGER NOT NULL DEFAULT 7,
  preferred_discovery_mode TEXT NOT NULL DEFAULT 'deep_match',
  PRIMARY KEY (user_id, tenant_id)
);

CREATE TABLE saves (
  user_id TEXT NOT NULL,
  tenant_id TEXT NULL,
  submission_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (user_id, submission_id)
);

CREATE TABLE private_notes (
  user_id TEXT NOT NULL,
  tenant_id TEXT NULL,
  submission_id TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (user_id, submission_id)
);

CREATE TABLE reading_history (
  user_id TEXT NOT NULL,
  tenant_id TEXT NULL,
  submission_id TEXT NOT NULL,
  read_at TEXT NOT NULL,
  PRIMARY KEY (user_id, submission_id)
);

CREATE TABLE feedback_events (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  tenant_id TEXT NULL,
  submission_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  reason TEXT NULL,
  created_at TEXT NOT NULL,
  local_only INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE briefs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NULL,
  user_id TEXT NULL,
  title TEXT NOT NULL,
  query TEXT NULL,
  created_at TEXT NOT NULL,
  private INTEGER NOT NULL DEFAULT 1,
  reflection TEXT NULL
);

CREATE TABLE brief_items (
  brief_id TEXT NOT NULL,
  submission_id TEXT NOT NULL,
  role TEXT NOT NULL,
  position INTEGER NOT NULL,
  PRIMARY KEY (brief_id, submission_id)
);
