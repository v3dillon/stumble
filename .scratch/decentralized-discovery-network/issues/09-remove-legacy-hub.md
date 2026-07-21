# Remove the legacy Hub contract and cached state

Status: ready-for-agent
Blocked by: 04, 05, 07
Source: ../PRD.md

## What to build

Delete the centralized legacy Hub registration, refresh, search, and event-import model now that its valid behavior is covered by the signed Stumble Substrate. Do not retain aliases, redirects, compatibility switches, dead domain objects, or non-authoritative cache data.

## Acceptance criteria

- [ ] Every legacy Hub HTTP route is absent and is not redirected or aliased to a new substrate route.
- [ ] Hub refresh daemons, runtime options, reports, transports, and event-import paths are removed.
- [ ] Hub domain types, Agent Tools, store collections, serializers, seed data, fixtures, adapters, and dedicated tests are removed with no remaining production callers.
- [ ] Well-known node metadata and public route documentation contain no Hub terminology or endpoint.
- [ ] Home discovery and public-Pod search use only Bootstrap, Index, Announcement Stream, Discovery Peer, and direct-address contracts.
- [ ] A forward SQLite migration drops legacy Hub cache tables without transforming their non-authoritative contents.
- [ ] The corresponding hosted-store schema no longer creates legacy Hub tables for new deployments.
- [ ] Migrating an existing database preserves node identity, credentials, Pods, Pod Events, Subscriptions, private projections, and all unrelated state.
- [ ] Retired Hub command-line options fail clearly rather than being silently ignored.
- [ ] Repository documentation and tests consistently use canonical Stumble Substrate terminology.

## Comments

