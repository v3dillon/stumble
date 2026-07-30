-- Forward migration: drop non-authoritative legacy Hub cache state without
-- transforming its contents. Hub tables and document collections hold only
-- re-acquirable discovery caches; node identity, Pods, events, Subscriptions,
-- and private projections are unrelated and untouched.

DROP TABLE IF EXISTS hub_registered_nodes;
DROP TABLE IF EXISTS hub_registered_pods;

DELETE FROM stumble_store_records
WHERE collection IN ('hub_nodes', 'hub_pods');

INSERT INTO stumble_store_metadata (key, value)
VALUES ('schema_legacy_hub_removed', '1')
ON CONFLICT (key) DO UPDATE SET value = excluded.value;
