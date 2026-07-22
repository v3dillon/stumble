# Search through replaceable Index Nodes privately

Status: ready-for-agent
Blocked by: 02, 03
Source: ../PRD.md

## What to build

Provide efficient intentional public-Pod search through replaceable Index Nodes while preserving the rule that passive personalization is local. Only words explicitly authored by the User may become a remote search query, and remote scores must not control local eligibility or order.

## Acceptance criteria

- [x] An Index-capable node searches its admitted valid announcement catalog for an explicit bounded query.
  - Evidence: `search_index_catalog` / `AgentTools::search_pod_announcements`; tests `searches_admitted_catalog_for_explicit_query`, `index_capable_node_searches_catalog_without_user_or_analytics`
- [x] A Home Node sends a remote query only from an explicit User-authored Explore action, never from Taste Profile, Source Affinity, Subscription, feedback, or Discovery Plan inference.
  - Evidence: `explore_public_pods_with_indexes` / `import_explicit_index_search` take explicit query only; `empty_query_explore_never_contacts_index`; `personal_discovery_never_receives_index_client`; `index_request_is_public_only`
- [x] Search results contain current Origin-signed announcements and retrieval evidence without quality, trust, popularity, or personalized authority fields.
  - Evidence: `PodAnnouncementSearchResponse` wire shape; tests assert no `global_quality_score` / `authority` / `popularity` / `trust` fields
- [x] The Home Node verifies returned announcements, applies Trust Policy, discards remote ordering authority, and recomputes relevance locally.
  - Evidence: `retain_index_search_results` discards remote relevance; `explicit_explore_queries_indexes_local_rerank_and_replacement` asserts local score > remote 0.001
- [x] Multiple configured Index Nodes are supported and removing one excludes results known only through it without affecting independent copies.
  - Evidence: `multi_index_fallthrough_and_independent_copies`; `removing_index_from_policy_excludes_sole_source`; discovery_substrate replaceable Index test
- [x] Empty, oversized, malformed, rate-limited, and incompatible searches return bounded typed outcomes.
  - Evidence: `IndexSearchFailureKind`; tests `rejects_disabled_oversized_malformed_and_rate_limited`, `typed_failures_for_disabled_oversized_malformed`, HTTP `http_index_search_returns_typed_disabled_and_oversized_codes`
- [x] Explicit search processing requires no User account or stable User identifier and does not create retained product analytics state.
  - Evidence: public GET has no auth; `index_runtime` stores timestamps only (no query text); test asserts runtime serialization lacks query
- [x] Search and local result provenance survive restart where persistence is required for audit and replacement behavior.
  - Evidence: `index_search_provenance_survives_sqlite_restart` (`received_from_index_urls` + `index_runtime`)
- [x] HTTP, CLI, and Agent Harness-facing Explore behavior uses the same domain contract.
  - Evidence: `ExploreRequest` / `ExploreResponse` via Core; CLI `stumble pod explore` and HTTP/MCP call the same domain types; Index import is optional Explore path with shared ranking

## Comments

- Implemented in `crates/stumble-core/src/index/` (`search.rs`, `client.rs`, `types.rs`) with thin `AgentTools` wiring (`with_index_capability`, `explore_public_pods_with_indexes`, `import_explicit_index_search`).
- Docs: `docs/discovery.md` (Replaceable private Index search).
