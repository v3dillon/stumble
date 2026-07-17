# Content curation

Candidate evidence is evaluated independently for each proposed Pod Placement. Pods default to Assisted Curation at a confidence threshold of `0.8`: only evidence tied to a valid Discovery Task is trusted for automatic acceptance. Manual Curation always queues review, while Autonomous Curation can be enabled only through an independently approved Pending Proposal.

Accepted placements refer to one canonical Content Item even when several Pods accept it. Every placement retains its reason, confidence evidence, source Candidate Submission identities, curation path, actor, optional note, and transition history. Routing Agent proposals are limited to authorized local Pods. Explicit Add to Pod bypasses Candidate review but preserves the existing Content Item identity and source URL.

The public domain API uses `ContentItem` and `ContentItemId`; legacy `Submission` records remain a private persistence compatibility layer behind the canonical type. Routing reasons and curation notes use a validated non-empty rationale type, so invalid values are rejected during both direct construction and deserialization.

Federation emits a synchronization-safe Accepted Placement projection containing the Content Item identity, public Pod-fit reason, curation path, Origin Node, and acceptance time. Candidate IDs, Candidate Submission IDs, local actor identities, audit notes, and legacy private fields never enter that event. Subscribers persist the projection for Feed attribution alongside the accepted Content Item.

Rejected and reversed routes remain auditable and suppress the same local route from being proposed again. Their private reasons do not create federation events. Reversing a public placement continues to require the sensitive-change approval flow.
