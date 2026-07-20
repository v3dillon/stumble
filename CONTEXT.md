# Stumble

Stumble is a decentralized personal discovery system that assembles a user's feed from independently curated Pods.

## Language

**Feed**:
The continuously refreshed, personalized sequence of content drawn from the Pods a User subscribes to.
_Avoid_: Timeline, brief

**Pod**:
A subscribable, decentralized collection of thematically related content with its own discovery and curation behavior.
_Avoid_: Bucket, channel, community

**Subscription**:
The relationship through which a User makes a Pod's accepted content eligible for their Feed; it grants no authority to curate or administer that Pod.
_Avoid_: Membership, follow

**Pod Role**:
An authority assignment connecting a User to a Pod as either its Owner or a Curator, independent of any Subscription.
_Avoid_: Membership, admin role

**Pod Owner**:
A User with authority to govern a Pod and delegate its Pod Roles.
_Avoid_: Pod admin, member

**Pod Curator**:
A User authorized to curate a Pod's content and package within the capabilities granted to the acting Agent Harness.
_Avoid_: Pod member, moderator

**User**:
A person whose private preferences, Subscriptions, and feedback shape their Feed.
_Avoid_: Account, member

**Taste Profile**:
The User's private combination of explicit preferences and inspectable learned weights that shapes Attention Value on their Home Node.
_Avoid_: Engagement profile, recommendation identity

**Interest Seed**:
Weak private evidence created when a User submits a Content Reference, describing possible topic, author, source, and source-context affinities without becoming durable preference until corroborated. Agent-discovered Candidates never create Interest Seeds by themselves.
_Avoid_: Implicit like, browsing event, engagement signal

**Source Affinity**:
An inspectable private preference for a content neighborhood such as a domain, publisher, author, account, or community, learned from Interest Seeds and Feedback Signals.
_Avoid_: Follow, browsing history, platform profile

**Discovery Plan**:
A private, finite source-selection strategy for one discovery run that balances proven Source Affinities with adjacent exploration while honoring explicit blocks.
_Avoid_: Search prompt, crawl configuration, source list

**Discovery Lead**:
A potential source neighborhood inferred from private User evidence or verified public Stumble metadata that may be selected into a Discovery Plan but is not yet a Candidate.
_Avoid_: Candidate, recommendation, remote search query

**Discovery Result Batch**:
A finite private shortlist of Candidates returned from one Discovery Task for the User to review and explicitly save, place, reinforce, or reject.
_Avoid_: Feed Batch, search results, scraped links

**Home Node**:
The User-controlled Stumble node that synchronizes subscribed Pod content and assembles the User's Feed locally.
_Avoid_: Client, central server

**Home Node Owner Credential**:
The keychain-backed local credential created when a Home Node is initialized, automatically authorizing its User to manage that node and its Agent Harnesses.
_Avoid_: Root token, admin API key

**Origin Node**:
The Stumble node that publishes the authoritative public version of a Pod and its content.
_Avoid_: Host, upstream server

**Content Item**:
The canonical unit of discovered content that may be placed in one or more Pods and appear once in a Feed.
_Avoid_: Submission, post, link

**Pod Placement**:
The association between a Content Item and a Pod, including why that item belongs there.
_Avoid_: Copy, duplicate

**Candidate**:
Potential content discovered during ingestion that has not yet been accepted into a Pod and is never synchronized to subscribers.
_Avoid_: Draft post, pending submission

**Candidate Submission**:
The idempotent, provenance-bearing structured input through which an Agent Harness submits a Candidate to an explicit User or Pod Placements target.
_Avoid_: Scrape result, link submission

**Accepted Placement**:
A Pod Placement approved by the Pod's curation policy, making the Content Item eligible for synchronization and Feeds.
_Avoid_: Promotion, publication

**Source Rule**:
A Pod-owned instruction telling an Agent Harness what sources to inspect, what to seek, and how often discovery is due.
_Avoid_: Connector configuration, scraper implementation

**Routing Agent**:
The local decision-maker that proposes one or more Pod Placements for a Candidate without gaining authority to modify remote Pods.
_Avoid_: Global classifier, recommendation agent

**Feed Batch**:
A finite, stable, structured set of Content Items and ranking evidence selected from subscribed Pods for an Agent Harness to present during one consumption session.
_Avoid_: Page, infinite scroll

**Batch Intent**:
Temporary focus and avoidance instructions that affect one Feed Batch without changing the User's durable preferences.
_Avoid_: Preference update, search query

**Feed Mix**:
The configurable composition constraints that balance highest-value subscribed content, Exploration Items, Old Gems, Priority Subscriptions, and per-Pod or per-source caps within a Feed Batch.
_Avoid_: Global sort, content quota

**Drip**:
A friendly conversational intent through which a User asks an Agent Harness to retrieve the current or next Feed Batch from Stumble; it is not a transport-level operation name.
_Avoid_: Dopamine score, infinite feed

**Caught Up**:
The explicit Feed state reached when the current Feed Batch has been consumed, before the User deliberately requests another batch.
_Avoid_: Empty state, end of page

**Delivered Item**:
A Content Item included in a Feed Batch returned to an Agent Harness; delivery suppresses near-term repetition but does not permanently exclude the item.
_Avoid_: Seen item, read item

**Resurfaced Item**:
A previously Delivered Item selected again after its repetition penalty has decayed or new evidence makes it valuable.
_Avoid_: Duplicate, repost

**Old Gem**:
A Resurfaced Item selected because its durable Attention Value makes it worth revisiting after the repetition penalty has decayed.
_Avoid_: Throwback, duplicate

**Placement Tombstone**:
An origin-authored withdrawal of a Pod Placement that stops future delivery through that placement without deleting independent local placements or saves.
_Avoid_: Item deletion, purge

**Pod Event**:
A signed, append-only statement from an Origin Node describing a public or authorized change to a Pod.
_Avoid_: Sync record, database update

**Private Projection**:
Home Node state derived for a User, such as Feed history, feedback, saves, and Subscriptions, that is never included in Pod federation.
_Avoid_: Private event stream, user export

**Stumble Substrate**:
The decentralized network of Stumble nodes that exchanges signed public Pod metadata through direct addressing, peer federation, and optional indexing roles.
_Avoid_: Central directory, blockchain, global database

**Pod Announcement**:
A compact signed advertisement of a public Pod's identity, subject, Origin Node, package version, and latest event pointer that may be relayed without synchronizing its content.
_Avoid_: Pod replica, feed export

**Index Node**:
An optional replaceable Stumble node role that aggregates public Pod Announcements to accelerate discovery without becoming authoritative for those Pods.
_Avoid_: Central hub, registry

**Relay Node**:
An optional Stumble node role that caches and serves signed Pod Events for an Origin Node without gaining authority to alter them.
_Avoid_: Origin proxy, central host

**Trust Policy**:
A Home Node's local rules for accepting peers, querying Index Nodes, admitting Exploration Items, and blocking Pods, nodes, sources, or topics.
_Avoid_: Global reputation, moderation score

**Pod Endorsement**:
A signed recommendation of one public Pod by another that may inform local discovery without granting authority or establishing a universal rank.
_Avoid_: Verification badge, global reputation

**Attention Value**:
Stumble's estimate that a Content Item is worth a User's limited attention based on relevance, quality, novelty, diversity, timeliness, and explicit feedback.
_Avoid_: Engagement, dwell time, retention

**Explore**:
The intentional discovery surface for public Pods and their Content Items beyond the User's current Subscriptions.
_Avoid_: Search, trending page

**Exploration Item**:
A clearly labeled Content Item selected from an unsubscribed public Pod to introduce controlled discovery into a Feed Batch.
_Avoid_: Advertisement, suggested post

**Add to Pod**:
An authorized User action that immediately creates an Accepted Placement for an existing Content Item while preserving its provenance.
_Avoid_: Repost, copy, submit

**Public Pod**:
A Pod that anyone may discover, subscribe to, and synchronize through federation.

**Invite-only Pod**:
A Pod whose discovery, Subscription, and synchronization require explicit authorization.
_Avoid_: Unlisted Pod

**Private Pod**:
A Pod confined to its Home Node that never participates in federation or Explore.
_Avoid_: Personal list, local-only channel

**Content Reference**:
The durable source URL, permitted metadata, generated understanding, and provenance through which Stumble represents third-party content without necessarily mirroring it.
_Avoid_: Archived copy, repost

**Node Agent**:
The local automation process that runs synchronization, routing, enrichment, and Feed preparation on behalf of a node's Users and Pods.
_Avoid_: Pod agent, crawler daemon

**Agent Harness**:
An external conversational or autonomous agent environment through which a User operates Stumble using its transport-neutral tools.
_Avoid_: User interface, Stumble client

**Harness Grant**:
A revocable Home Node authorization assigning an Agent Harness only the Stumble capabilities and Pod scopes it may use.
_Avoid_: API key, agent role

**Feed-ready Event**:
An optional Home Node notification indicating that a new stable Feed Batch is available for an Agent Harness to retrieve.
_Avoid_: Push feed, notification item

**Discovery Task**:
A leaseable unit of due discovery work derived from either a Pod's Source Rules or a User's private Discovery Plan and completed by an Agent Harness.
_Avoid_: Crawl job, scheduled prompt

**Personal Discovery**:
User-scoped discovery governed by a private Discovery Plan that produces a Discovery Result Batch without requiring or modifying a Pod.
_Avoid_: For You Pod, personal Pod, global recommendations

**Scheduler Adapter**:
A mechanism that wakes discovery workers, supplied either by an Agent Harness or by Stumble's local platform integration without changing Discovery Task semantics.
_Avoid_: Stumble scheduler, cron rule

**Discovery-ready Event**:
A notification that browser-required Discovery Tasks are waiting for an authorized Agent Harness to claim them.
_Avoid_: Browser job, forced wake-up

**Discovery-results-ready Event**:
A private one-shot notification that a completed Discovery Result Batch is available for an Agent Harness to present or retain for the User.
_Avoid_: Feed-ready Event, repeated reminder, push result

**Browser Connector**:
An Agent Harness capability that reads through its own User-approved browser session and submits discovered references to Stumble as Candidates.
_Avoid_: Stumble browser, remote browser

**Browser Grant**:
A User-controlled authorization within an Agent Harness defining which domains and actions its Browser Connector may use.
_Avoid_: Browser credentials, blanket permission

**Agent Proposal**:
A confidence-scored, evidence-backed recommendation from the Node Agent that must pass deterministic policy before changing authoritative state.
_Avoid_: Agent decision, automatic write

**Pod Package**:
The signed, versioned bundle of Pod Context, Pod Skill, Source Rule suggestions, filters, and calibration examples synchronized with a Pod.
_Avoid_: Skill pack, prompt bundle

**Pod Context**:
The `CONTEXT.md` portion of a Pod Package defining its subject language, scope, and boundaries without implementation instructions.
_Avoid_: System prompt, documentation dump

**Pod Skill**:
The `SKILL.md` portion of a Pod Package containing scoped, untrusted instructions for an Agent Harness performing discovery or curation for that Pod.
_Avoid_: System prompt, executable plugin

**Package Revision**:
A validated proposed change to a Pod Package that becomes signed and authoritative only after an authorized owner accepts it.
_Avoid_: File edit, prompt update

**Pending Proposal**:
An expiring structured request for a sensitive change that cannot take effect until the Home Node Owner or a separately authorized interactive Agent Harness confirms it for the User.
_Avoid_: Confirmation prompt, queued action

**Feedback Signal**:
An explicit User action that changes future Feed ranking, including Save, More like this, Less like this, Dismiss, source or topic blocking, Add to Pod, and Subscription changes.
_Avoid_: Engagement event, dwell-time signal

**Curation Policy**:
A Pod-owned autonomy setting that governs whether Candidates require manual review or may receive an Accepted Placement from qualifying Agent Proposals.
_Avoid_: Moderation mode, agent permissions

**Manual Curation**:
A Curation Policy under which every Candidate requires authorized approval.

**Assisted Curation**:
The default Curation Policy under which trusted Source Rules and high-confidence Agent Proposals may create Accepted Placements while uncertain Candidates wait for review.

**Autonomous Curation**:
An explicitly enabled Curation Policy under which any Candidate clearing the Pod's configured confidence threshold may receive an Accepted Placement.

**Priority Subscription**:
A Subscription configured to guarantee representation from its Pod in each Feed Batch when unseen accepted content is available.
_Avoid_: Favorite Pod, pinned feed
