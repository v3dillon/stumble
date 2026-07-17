---
status: superseded by ADR-0025
---

# Let Agent Harnesses own browser access

Agent Harnesses own and authorize their browser sessions, read Pod context and curation instructions, and submit provenance-bearing Candidates to Stumble. Stumble never controls Chrome or receives browser credentials; its Node Agent continues to run non-browser Source Connectors, while a scheduled local Agent Harness may provide unattended browser discovery.
