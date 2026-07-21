# Bootstrap fresh Home Nodes from replaceable defaults

Status: ready-for-agent
Blocked by: 02
Source: ../PRD.md

## What to build

Give a newly initialized Home Node immediate outbound access to the neutral public discovery catalog through a removable sponsored Bootstrap Node while supporting multiple independently operated replacements from the first release.

## Acceptance criteria

- [ ] New Home Nodes receive the sponsored Bootstrap endpoint as an ordinary removable default rather than a protocol constant or authority.
- [ ] Bootstrap configuration is an ordered User-controlled list that supports adding, disabling, removing, and inspecting multiple endpoints.
- [ ] The Home Node fetches Announcement Stream pages outbound, verifies them locally, and persists a separate cursor and provenance for each Bootstrap Node.
- [ ] Refresh falls through to another configured Bootstrap after transport or protocol failure without discarding previously verified announcements.
- [ ] Removing a Bootstrap excludes announcements known only through that endpoint from current eligibility while preserving audit state and any independently learned copies.
- [ ] Bootstrap synchronization sends no Taste Profile, Subscription list, feedback, Source Affinity, or background interest-derived query.
- [ ] Configuration and synchronization progress survive SQLite restart.
- [ ] Direct Pod URL discovery and Subscription continue to work with every Bootstrap disabled or unavailable.
- [ ] CLI and HTTP operator surfaces report configured endpoints, cursor state, last success, and typed failure without exposing private discovery evidence.

## Comments

