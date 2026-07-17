CREATE TABLE IF NOT EXISTS stumble_store_records (
  collection TEXT NOT NULL,
  record_key TEXT NOT NULL,
  value_json TEXT NOT NULL,
  PRIMARY KEY (collection, record_key)
);

CREATE TABLE IF NOT EXISTS stumble_store_metadata (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS stumble_pods_tenant_slug_idx
ON stumble_store_records (
  COALESCE(json_extract(value_json, '$.tenant_id'), ''),
  json_extract(value_json, '$.slug')
)
WHERE collection = 'pods';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_tenants_slug_idx
ON stumble_store_records (json_extract(value_json, '$.slug'))
WHERE collection = 'tenants';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_submissions_tenant_canonical_idx
ON stumble_store_records (
  COALESCE(json_extract(value_json, '$.tenant_id'), ''),
  json_extract(value_json, '$.canonical_url')
)
WHERE collection = 'submissions';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_events_content_hash_idx
ON stumble_store_records (json_extract(value_json, '$.content_hash'))
WHERE collection = 'event_log';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_events_pod_chain_idx
ON stumble_store_records (
  json_extract(value_json, '$.pod_slug'),
  COALESCE(json_extract(value_json, '$.previous_event_hash'), '')
)
WHERE collection = 'event_log';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_api_tokens_hash_idx
ON stumble_store_records (json_extract(value_json, '$.token_hash'))
WHERE collection = 'api_tokens';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_scheduled_discovery_task_idx
ON stumble_store_records (
  json_extract(value_json, '$.pod_id'),
  json_extract(value_json, '$.package_version'),
  json_extract(value_json, '$.origin.source_rule_index'),
  json_extract(value_json, '$.due_at')
)
WHERE collection = 'discovery_tasks'
  AND json_extract(value_json, '$.origin.kind') = 'scheduled';

CREATE UNIQUE INDEX IF NOT EXISTS stumble_immediate_discovery_task_idx
ON stumble_store_records (
  json_extract(value_json, '$.origin.requested_by'),
  json_extract(value_json, '$.origin.idempotency_key')
)
WHERE collection = 'discovery_tasks'
  AND json_extract(value_json, '$.origin.kind') = 'immediate';
