---
status: superseded by ADR-0025
---

# Run Source Connectors on Origin Nodes

A Pod's Origin Node executes its Source Rules and keeps Connector Secrets local, while subscribers synchronize only accepted Content References. When a User runs a private or local Pod, their Home Node is also its Origin Node; credentials never enter Pod exports, skill packs, or federation data.
