---
status: superseded by ADR-0025
---

# Share Source Connectors across Pods

Stumble provides reusable Source Connectors for external platforms and protocols, while each Pod owns Source Rules describing accounts, communities, queries, cadence, and credentials. This separates fragile platform integration code from Pod-specific editorial intent and avoids duplicating the same scraper implementation across Pods.
