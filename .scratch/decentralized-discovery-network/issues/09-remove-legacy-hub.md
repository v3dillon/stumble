# Remove the legacy Hub contract and cached state

Status: ready-for-agent
Blocked by: 04, 05, 07
Source: ../PRD.md

## What to build

Delete the centralized legacy Hub registration, refresh, search, and event-import model now that its valid behavior is covered by the signed Stumble Substrate. Do not retain aliases, redirects, compatibility switches, dead domain objects, or non-authoritative cache data.

## Acceptance criteria

- [x] Every legacy Hub HTTP route is absent and is not redirected or aliased to a new substrate route.
  - Evidence: removed `/hub/*` and hub-era `/discovery/pods` from `stumble-api` router; tests `retired_hub_http_routes_are_absent_without_redirect`, feed_adapters 404 checks
- [x] Hub refresh daemons, runtime options, reports, transports, and event-import paths are removed.
  - Evidence: `stumble-api` main no longer spawns hub refresh; `HubRefreshReport` / `refresh_hub_index` / `import_public_events_from_hub_node` removed from `stumble-sync` and Core
- [x] Hub domain types, Agent Tools, store collections, serializers, seed data, fixtures, adapters, and dedicated tests are removed with no remaining production callers.
  - Evidence: domain `Hub*` / feed types deleted; store `hub_nodes`/`hub_pods` collections gone; Agent Tools hub methods gone; dedicated hub seed tests removed
- [x] Well-known node metadata and public route documentation contain no Hub terminology or endpoint.
  - Evidence: `well_known_node` no longer advertises `hub_search_pods`; tests `well_known_metadata_contains_no_hub_terminology`, `public_route_docs_contain_no_legacy_hub_routes_or_terminology`
- [x] Home discovery and public-Pod search use only Bootstrap, Index, Announcement Stream, Discovery Peer, and direct-address contracts.
  - Evidence: `GET /home/discover-public-pods` returns `ExploreResponse` via `explore_public_pods`; Index search remains `GET /discovery/announcements`; CLI explore already substrate-only
- [x] A forward SQLite migration drops legacy Hub cache tables without transforming their non-authoritative contents.
  - Evidence: `migrations/sqlite/0003_drop_legacy_hub.sql` applied on open; test `forward_sqlite_migration_drops_hub_caches_and_preserves_unrelated_state`
- [x] The corresponding hosted-store schema no longer creates legacy Hub tables for new deployments.
  - Evidence: hub tables removed from `migrations/sqlite/0001_init.sql` and `migrations/postgres/0001_init.sql`; test `hosted_init_schema_source_contains_no_hub_tables`, `new_sqlite_deployments_never_create_hub_tables`
- [x] Migrating an existing database preserves node identity, credentials, Pods, Pod Events, Subscriptions, private projections, and all unrelated state.
  - Evidence: migration only DROP/DELETE hub tables/collections; test `forward_sqlite_migration_drops_hub_caches_and_preserves_unrelated_state`
- [x] Retired Hub command-line options fail clearly rather than being silently ignored.
  - Evidence: clap rejects `--disable-hub-refresh` and `--hub-refresh-interval-seconds`; test `api_process_rejects_retired_hub_refresh_options`
- [x] Repository documentation and tests consistently use canonical Stumble Substrate terminology.
  - Evidence: `docs/discovery.md` Bootstrap wording; ADR-0050 retained as historical decision; tests assert no Hub fields on Explore surfaces

## Comments

- Forward migration is re-applied idempotently from `open_sqlite_store` after `0002_authoritative_store.sql`.
- No commit performed by the implementing agent.
