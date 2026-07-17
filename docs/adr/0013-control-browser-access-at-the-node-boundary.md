---
status: superseded by ADR-0017
---

# Control browser access at the node boundary

The Node Agent may attach to a User-approved Chrome or CDP session through a Browser Connector. Browser Grants restrict domains, actions, and requesting local Pods; reads are the default, account mutations require separate explicit authorization, access is locally audited, credentials are not extracted, and remote Pods cannot invoke a subscriber's browser.
