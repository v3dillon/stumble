CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE node_identity (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL,
  display_name text NOT NULL,
  public_key text NOT NULL,
  private_key_encrypted_or_local text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE trusted_peers (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL,
  display_name text NOT NULL,
  base_url text NOT NULL,
  public_key text NOT NULL,
  trust_level text NOT NULL CHECK (trust_level IN ('read_only', 'read_write')),
  enabled boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sync_runs (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL,
  peer_id uuid NOT NULL,
  pod_slug text NULL,
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz NULL,
  status text NOT NULL,
  imported_events integer NOT NULL DEFAULT 0
);

CREATE TABLE sync_errors (
  id uuid PRIMARY KEY,
  sync_run_id uuid NULL REFERENCES sync_runs(id),
  peer_id uuid NULL,
  message text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE tenants (
  id uuid PRIMARY KEY,
  name text NOT NULL,
  slug text NOT NULL UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE users (
  id uuid PRIMARY KEY,
  display_name text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE tenant_users (
  tenant_id uuid NOT NULL REFERENCES tenants(id),
  user_id uuid NOT NULL REFERENCES users(id),
  role text NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, user_id)
);

CREATE TABLE api_tokens (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id),
  tenant_id uuid NULL REFERENCES tenants(id),
  token_hash text NOT NULL UNIQUE,
  label text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  last_used_at timestamptz NULL,
  revoked_at timestamptz NULL
);

CREATE TABLE managed_node_identities (
  id uuid PRIMARY KEY REFERENCES node_identity(id),
  tenant_id uuid NOT NULL REFERENCES tenants(id),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE pods (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  name text NOT NULL,
  slug text NOT NULL,
  description text NOT NULL,
  visibility text NOT NULL CHECK (visibility IN ('public', 'invite_only', 'private')),
  created_by uuid NULL REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  origin_node_id uuid NULL,
  UNIQUE (tenant_id, slug)
);

CREATE TABLE pod_memberships (
  user_id uuid NOT NULL REFERENCES users(id),
  pod_id uuid NOT NULL REFERENCES pods(id),
  role text NOT NULL CHECK (role IN ('owner', 'moderator', 'member')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, pod_id)
);

CREATE TABLE pod_rules (
  pod_id uuid PRIMARY KEY REFERENCES pods(id),
  blocked_topics text[] NOT NULL DEFAULT '{}',
  blocked_domains text[] NOT NULL DEFAULT '{}',
  auto_promote_crawler_candidates boolean NOT NULL DEFAULT false,
  federate_sources boolean NOT NULL DEFAULT true
);

CREATE TABLE pod_skill_packs (
  id uuid PRIMARY KEY,
  pod_id uuid NOT NULL REFERENCES pods(id),
  version integer NOT NULL,
  pod_yaml text NOT NULL,
  skill_md text NOT NULL,
  sources_yaml text NOT NULL,
  filters_yaml text NOT NULL,
  examples_good_md text NOT NULL,
  examples_bad_md text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE event_log (
  event_id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  event_type text NOT NULL,
  pod_slug text NOT NULL,
  author_node_id uuid NOT NULL,
  author_display_name text NULL,
  payload_json jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  previous_event_hash text NULL,
  content_hash text NOT NULL UNIQUE,
  signature text NOT NULL,
  imported_from_peer_id uuid NULL REFERENCES trusted_peers(id),
  verified boolean NOT NULL DEFAULT false
);

CREATE TABLE submissions (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  url text NOT NULL,
  canonical_url text NOT NULL,
  title text NOT NULL,
  description text NULL,
  domain text NOT NULL,
  submitted_by uuid NULL REFERENCES users(id),
  discovered_by_crawler boolean NOT NULL DEFAULT false,
  submitter_note text NULL,
  summary text NULL,
  tags text[] NOT NULL DEFAULT '{}',
  embedding real[] NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  origin_event_id uuid NULL REFERENCES event_log(event_id),
  UNIQUE (tenant_id, canonical_url)
);

CREATE TABLE submission_pods (
  submission_id uuid NOT NULL REFERENCES submissions(id),
  pod_id uuid NOT NULL REFERENCES pods(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (submission_id, pod_id)
);

CREATE TABLE submission_assets (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  asset_type text NOT NULL CHECK (asset_type IN ('representative_image')),
  source text NOT NULL CHECK (source IN ('page_image', 'ai_generated', 'user_provided')),
  url text NULL,
  local_path text NULL,
  mime_type text NULL,
  alt_text text NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (url IS NOT NULL OR local_path IS NOT NULL)
);

CREATE TABLE source_domains (
  domain text PRIMARY KEY,
  quality_score real NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE crawler_sources (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  pod_id uuid NOT NULL REFERENCES pods(id),
  source_type text NOT NULL CHECK (source_type IN ('rss', 'atom', 'sitemap', 'webpage')),
  url text NOT NULL,
  enabled boolean NOT NULL DEFAULT true,
  crawl_interval_minutes integer NOT NULL DEFAULT 1440,
  last_crawled_at timestamptz NULL,
  origin_event_id uuid NULL REFERENCES event_log(event_id)
);

CREATE TABLE crawl_runs (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  crawler_source_id uuid NOT NULL REFERENCES crawler_sources(id),
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz NULL,
  status text NOT NULL,
  error text NULL
);

CREATE TABLE crawl_candidates (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  pod_id uuid NOT NULL REFERENCES pods(id),
  crawler_source_id uuid NOT NULL REFERENCES crawler_sources(id),
  url text NOT NULL,
  canonical_url text NOT NULL,
  title text NOT NULL,
  description text NULL,
  domain text NOT NULL,
  summary text NULL,
  tags text[] NOT NULL DEFAULT '{}',
  status text NOT NULL CHECK (status IN ('pending', 'promoted', 'rejected')),
  rejection_reason text NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE annotations (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  user_id uuid NULL REFERENCES users(id),
  body text NOT NULL,
  public boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE user_preferences (
  user_id uuid NOT NULL REFERENCES users(id),
  tenant_id uuid NULL REFERENCES tenants(id),
  interests text[] NOT NULL DEFAULT '{}',
  blocked_topics text[] NOT NULL DEFAULT '{}',
  blocked_sources text[] NOT NULL DEFAULT '{}',
  preferred_brief_length integer NOT NULL DEFAULT 7,
  preferred_discovery_mode text NOT NULL DEFAULT 'deep_match',
  PRIMARY KEY (user_id, tenant_id)
);

CREATE TABLE saves (
  user_id uuid NOT NULL REFERENCES users(id),
  tenant_id uuid NULL REFERENCES tenants(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, submission_id)
);

CREATE TABLE private_notes (
  user_id uuid NOT NULL REFERENCES users(id),
  tenant_id uuid NULL REFERENCES tenants(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  body text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, submission_id)
);

CREATE TABLE reading_history (
  user_id uuid NOT NULL REFERENCES users(id),
  tenant_id uuid NULL REFERENCES tenants(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  read_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, submission_id)
);

CREATE TABLE feedback_events (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id),
  tenant_id uuid NULL REFERENCES tenants(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  event_type text NOT NULL,
  reason text NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  local_only boolean NOT NULL DEFAULT true
);

CREATE TABLE briefs (
  id uuid PRIMARY KEY,
  tenant_id uuid NULL REFERENCES tenants(id),
  user_id uuid NULL REFERENCES users(id),
  title text NOT NULL,
  query text NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  private boolean NOT NULL DEFAULT true,
  reflection text NULL
);

CREATE TABLE brief_items (
  brief_id uuid NOT NULL REFERENCES briefs(id),
  submission_id uuid NOT NULL REFERENCES submissions(id),
  role text NOT NULL,
  position integer NOT NULL,
  PRIMARY KEY (brief_id, submission_id)
);
