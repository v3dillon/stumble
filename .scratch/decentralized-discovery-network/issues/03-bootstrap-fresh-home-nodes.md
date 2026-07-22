# Bootstrap fresh Home Nodes from replaceable defaults

Status: ready-for-agent
Blocked by: 02
Source: ../PRD.md

## What to build

Give a newly initialized Home Node immediate outbound access to the neutral public discovery catalog through a removable sponsored Bootstrap Node while supporting multiple independently operated replacements from the first release.

## Acceptance criteria

- [x] New Home Nodes receive the sponsored Bootstrap endpoint as an ordinary removable default rather than a protocol constant or authority.
  - Evidence: `seed_store` / `ensure_default_bootstrap_endpoint`; test `new_home_node_receives_sponsored_default_as_removable_entry`
- [x] Bootstrap configuration is an ordered User-controlled list that supports adding, disabling, removing, and inspecting multiple endpoints.
  - Evidence: `add/set_enabled/remove/list_bootstrap_endpoints`; test `ordered_list_supports_add_disable_remove_and_inspect`
- [x] The Home Node fetches Announcement Stream pages outbound, verifies them locally, and persists a separate cursor and provenance for each Bootstrap Node.
  - Evidence: `sync_bootstrap_endpoints` + `received_from_bootstrap_urls`; test `sync_fetches_verifies_and_persists_cursor_per_bootstrap`
- [x] Refresh falls through to another configured Bootstrap after transport or protocol failure without discarding previously verified announcements.
  - Evidence: test `refresh_falls_through_without_discarding_verified_announcements` and unit `multi_bootstrap_fallthrough_preserves_verified_announcements`
- [x] Removing a Bootstrap excludes announcements known only through that endpoint from current eligibility while preserving audit state and any independently learned copies.
  - Evidence: test `remove_excludes_sole_source_preserves_audit_and_independent_copies`
- [x] Bootstrap synchronization sends no Taste Profile, Subscription list, feedback, Source Affinity, or background interest-derived query.
  - Evidence: `request_is_public_only` + test `outbound_sync_sends_no_private_evidence`
- [x] Configuration and synchronization progress survive SQLite restart.
  - Evidence: test `config_and_sync_progress_survive_sqlite_restart`
- [x] Direct Pod URL discovery and Subscription continue to work with every Bootstrap disabled or unavailable.
  - Evidence: test `direct_pod_url_subscription_works_with_all_bootstraps_disabled`
- [x] CLI and HTTP operator surfaces report configured endpoints, cursor state, last success, and typed failure without exposing private discovery evidence.
  - Evidence: `stumble sync bootstrap …`; `GET/POST /home/bootstrap/*`; `docs/discovery.md`

## Comments

