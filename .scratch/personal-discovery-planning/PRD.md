Status: ready-for-agent

# Personal Discovery from User evidence

## Problem Statement

The User can submit Content References, give Feed feedback, and ask an Agent Harness to discover content, but those behaviors do not yet form a complete autonomous discovery loop. A submitted URL becomes a private Candidate without becoming weak evidence about the User's interests, existing Discovery Tasks are Pod-scoped, and an Agent Harness must be told where to search instead of receiving a privacy-minimized plan inferred from the User's own links and feedback. As a result, the User still has to name sources such as X or Hacker News and personally direct the browsing strategy.

## Solution

Stumble will turn User-submitted Content References into retractable Interest Seeds, corroborate those seeds with explicit User actions, and derive inspectable Source Affinities inside the private Taste Profile. The Home Node will combine those private signals with locally matched Discovery Leads from verified public Stumble metadata to build an immutable, task-specific Discovery Plan. Personal Discovery Tasks will be User-scoped rather than Pod-scoped, and a narrowly authorized Agent Harness will browse the selected source neighborhoods through its own Browser Connector without receiving the full Taste Profile or raw history.

On-demand and scheduled Personal Discovery will use the same leaseable task contract. Each completed run will produce a finite private Discovery Result Batch of Candidates for explicit review. Agent-found content will never train the Taste Profile by itself, enter a Pod, create a Subscription, or cross federation boundaries. User feedback on results will close the learning loop and influence later plans.

## User Stories

1. As a User, I want a URL I submit to become weak private evidence, so that Stumble gradually learns what I care about without requiring manual profile maintenance.
2. As a User, I want one submitted URL to have no material recommendation weight until corroborated, so that an incidental link does not distort my profile.
3. As a User, I want a second related submission or explicit positive action to corroborate an Interest Seed, so that repeated intent improves discovery.
4. As a User, I want repeated submission of the same canonical URL to remain one evidence action, so that retries cannot inflate an affinity.
5. As a User, I want an explicit interest to satisfy cold start immediately, so that I can begin discovery without first submitting multiple links.
6. As a User, I want a specific request such as finding content similar to one link to act as temporary intent, so that an otherwise cold profile can support that run.
7. As a User, I want generic Personal Discovery to wait until the profile contains an explicit interest or corroborated evidence, so that the agent does not browse randomly.
8. As a User, I want scheduled discovery to remain dormant during cold start, so that unattended workers do not spend effort without a meaningful basis.
9. As a User, I want to submit a URL without learning from it, so that archival or work references do not shape Personal Discovery.
10. As a User, I want to retract what one submitted URL taught Stumble without deleting the saved Content Reference, so that storage and personalization remain independent.
11. As a User, I want my Taste Profile to show aggregate topics and Source Affinities, so that I can understand what Stumble has learned.
12. As a User, I want Source Affinities to distinguish domains, publishers, authors or accounts, and communities, so that `x.com` is not treated as one undifferentiated interest.
13. As a User, I want explicit source and topic blocks to override all inferred affinities, so that blocked material is never selected into a plan.
14. As a User, I want negative feedback to oppose or block related affinities, so that discovery adapts away from unwanted content.
15. As a User, I want agent-discovered Candidates to create no learning evidence until I react, so that the agent cannot train itself into a feedback loop.
16. As a User, I want Personal Discovery to work without choosing or creating a Pod, so that discovery is about my private interests rather than curation topology.
17. As a User, I want Pod discovery to remain governed by Pod Packages, so that Personal Discovery does not weaken Pod curation boundaries.
18. As a User, I want the Home Node to build a Discovery Plan automatically, so that I can say “find me something interesting” without naming websites.
19. As a User, I want the plan to allocate 70% of results to proven source neighborhoods and 30% to adjacent exploration by default, so that discovery is relevant without becoming closed.
20. As a User, I want the discovery mix to be inspectable and configurable, so that I can adjust familiarity and exploration deliberately.
21. As a User, I want the default Discovery Result Batch to contain ten results, so that each run is finite and reviewable.
22. As a User, I want no more than three results from one domain and two from one author, account, publisher, or community, so that one source cannot dominate a batch.
23. As a User, I want canonical duplicates and recently reviewed items excluded, so that the agent does not repeatedly rediscover the same content.
24. As a User, I want to request a different finite result count for an on-demand run, so that the batch fits my immediate attention budget.
25. As a User, I want the plan to explain why each source neighborhood was selected, so that autonomous source choice remains inspectable.
26. As a User, I want each result to preserve its actual source URL, referrer, author, publication facts, discovery method, and plan provenance, so that I can verify where it came from.
27. As a User, I want the Home Node to use verified public Pod Announcements, Explore samples, endorsements, and Content References as Discovery Leads, so that the wider Stumble network helps identify relevant websites.
28. As a User, I want public network leads matched against my interests locally, so that my private Taste Profile is not disclosed to another node.
29. As a User, I want autonomous discovery to avoid profile-derived remote Index queries, so that private interests are not leaked through search terms.
30. As a User, I want an explicit Explore query to remain possible, so that I may deliberately send a query when I choose to search the public network.
31. As a User, I want network-assisted discovery to avoid automatically subscribing to Pods, so that source exploration does not silently change my Subscriptions.
32. As a User, I want the Agent Harness to receive only the task-specific Discovery Plan, so that an unattended worker cannot read my complete Taste Profile or raw Interest Seeds.
33. As a User, I want Browser Grants to restrict which planned sources the harness may visit, so that inferred interest never grants browser authority.
34. As a User, I want Stumble to keep browser credentials outside the Home Node and Discovery Plan, so that Personal Discovery does not weaken authentication boundaries.
35. As a User, I want an on-demand run to ask me to restore an expired high-value authenticated session while continuing with accessible sources, so that one login failure does not discard the run.
36. As a User, I want a scheduled run to skip unavailable authenticated sources, reallocate its quota, and finish, so that unattended tasks do not wait indefinitely.
37. As a User, I want at most one authentication-needed notice until the session is restored, so that scheduled discovery does not repeatedly nag me.
38. As a User, I want the harness to inspect broadly but submit only a finite relevant shortlist, so that Stumble does not retain everything it scrolls past.
39. As a User, I want autonomous discoveries to remain private Candidates until I act, so that nothing is silently published or placed in a Pod.
40. As a User, I want Save to create an Accepted Placement in my private Inbox, so that a useful result becomes durable without public exposure.
41. As a User, I want Add to Pod to create an Accepted Placement in the selected authorized Pod, so that provenance-bearing discovery can become curation.
42. As a User, I want More like this and Not for me on a result to update private learning explicitly, so that later plans improve.
43. As a User, I want ignoring a result to create no learning signal, so that silence is not interpreted as preference.
44. As a User, I want dismissing an entire batch to create no item-level negative evidence, so that clearing a queue does not mean disliking every result.
45. As a User, I want every Discovery Result Batch to retain its task and plan identity, so that the run is explainable after completion.
46. As a User, I want Personal Discovery on demand, so that I can request a batch conversationally at any time.
47. As a User, I want multiple named discovery schedules, so that general daily discovery and deeper weekly reading can coexist.
48. As a User, I want each schedule to configure cadence, optional temporary focus and avoidance, batch size, and delivery mode, so that schedules express intent without naming sources.
49. As a User, I want each schedule to use my current private profile when its task materializes, so that scheduled discovery evolves with my feedback.
50. As a User, I want each schedule to allow only one unreviewed result batch, so that unattended agents cannot accumulate an endless scrape pile.
51. As a User, I want a due scheduled run deferred behind its unreviewed batch rather than duplicated, so that backpressure is retry-safe.
52. As a User, I want an explicit on-demand run to remain available despite scheduled backpressure, so that I retain control.
53. As a User, I want a harness with its own scheduler to wake and claim due tasks, so that Stumble works naturally in capable agent environments.
54. As a User, I want Stumble's local Scheduler Adapter to wake workers when the harness lacks scheduling, so that the same feature works in simpler environments.
55. As a User, I want both scheduler paths to materialize the same idempotent Discovery Task, so that changing schedulers cannot duplicate runs or lose results.
56. As a User, I want one concise results-ready notification when the harness supports delivery, so that I know a scheduled batch is waiting.
57. As a User, I want queue-only delivery when notifications are unavailable or disabled, so that the batch remains privately available for my next conversation.
58. As a User, I want results-ready notifications to be one-shot and not mark the batch reviewed, so that notification and consumption remain distinct.
59. As a Home Node Owner, I want separate grants for interactive Personal Discovery management and unattended task execution, so that a worker cannot edit schedules, inspect the full profile, or approve its own authority.
60. As a Home Node Owner, I want revoking a worker grant to stop future claims without deleting completed private batches, so that authorization changes preserve local history.
61. As a Pod Curator, I want Personal Discovery changes to leave Pod curation, Packages, Source Rules, and federation unchanged, so that the new feature cannot bypass Pod authority.
62. As a User, I want Interest Seeds, Source Affinities, Discovery Plans, schedules, result batches, and reactions to persist across restart, so that the autonomous loop is durable.
63. As a User, I want all Personal Discovery state to remain absent from Pod Events, packages, manifests, announcements, Explore artifacts, and subscription synchronization, so that it stays private.
64. As a User, I want failed or partially unavailable runs to expose inspectable reasons, so that the harness can explain missing quota without inventing results.
65. As a User, I want retries to preserve task, plan, Candidate, and batch identity, so that transport or worker restarts cannot duplicate evidence.

## Implementation Decisions

- Extend the Taste Profile's learned layer with Interest Seed evidence and richer generic Source Affinities for domains, publishers, authors or accounts, and communities. Preserve explicit-preference precedence and aggregate explainability.
- Record a typed acquisition origin for Candidate Submissions. Only an authenticated interactive User action may create an Interest Seed. A worker submission tied to a Discovery Task is always agent-discovered and cannot be promoted into User evidence by caller-supplied metadata.
- Create at most one Interest Seed per User and canonical Content Reference. Store whether learning was disabled or later retracted independently from Candidate or Content Item retention.
- Enrich Interest Seeds monotonically as permitted page metadata becomes available. Derived facts must retain provenance and may include topics, source domain, publisher, author or account, community, and referrer context without storing browser credentials or unpermitted page content.
- Require either an explicit preference or two independent User evidence actions before an inferred signal receives non-zero discovery weight. Duplicate canonical URLs and retries do not corroborate themselves. Explicit blocks always win.
- Introduce a first-class Personal Discovery target alongside the existing Pod target in the Discovery Task model. Pod tasks remain pinned to a Pod Package; Personal Discovery tasks are pinned to an immutable private Discovery Plan.
- Build Discovery Plans on the Home Node from explicit preferences, corroborated Interest Seeds, Feedback Signals, Source Affinities, block state, recent result history, schedule intent, Browser Grant eligibility reported by the harness, and locally available Discovery Leads.
- A Discovery Plan is a minimized worker contract. It exposes selected topics and source locators, exclusions, allocation, quotas, rationale, and stable identity, but not raw Feedback history, the complete Taste Profile, raw Interest Seed history, private URLs unrelated to the task, or credentials.
- Represent source locators generically rather than adding X-, Hacker News-, or platform-specific connectors to Stumble. Platform navigation and rendered-page extraction remain Agent Harness responsibilities under Browser Grants.
- Use verified public Stumble metadata as an optional Discovery Lead reservoir. Matching against private preferences occurs locally. Autonomous planning cannot issue profile-derived queries to a remote Index Node, automatically subscribe to a Pod, or treat signatures and endorsements as global quality scores.
- Default plans target ten results with a 70/30 proven-to-adjacent allocation, a maximum of three results per domain, and two per author, account, publisher, or community. On-demand requests may choose another bounded finite count.
- Apply canonical deduplication and recent-review suppression before batch completion. When one quota cannot be filled, preserve the batch's inspectable shortfall or reallocation reason rather than silently weakening blocks or provenance requirements.
- Add a persistent Discovery Result Batch lifecycle with ready, reviewed, and dismissed outcomes. A batch owns an ordered finite set of private Candidate references and records its task, plan, allocation evidence, source availability, and notification state.
- Saving a result creates an Accepted Placement in the User's private Inbox. Add to Pod uses existing authorization and placement semantics. More like this and Not for me create explicit private learning evidence. Ignoring an item and dismissing a whole batch do not.
- Support multiple named private Personal Discovery schedules. Each stores cadence, optional Batch Intent-like focus and avoidance, finite batch size, and delivery mode. Schedule configuration is local private state and does not alter Pod Source Rules.
- Preserve scheduler neutrality: harness-owned scheduling and the local Scheduler Adapter both materialize or list the same idempotent due tasks. Each schedule defers while it owns an unreviewed result batch; explicit on-demand tasks are independent.
- Emit a private one-shot Discovery-results-ready Event after successful scheduled completion. Capable harnesses may notify once; queue-only mode retains the batch for later retrieval. Notification never marks a batch reviewed.
- Treat authentication as a harness-owned source-availability concern. On-demand execution may request User assistance while continuing accessible work. Scheduled execution skips unavailable sources, reallocates within policy, and suppresses repeated authentication-needed notices until availability changes.
- Add distinct authorization boundaries for interactive Personal Discovery management and unattended Personal Discovery execution. Workers may claim tasks, read only their pinned plans, submit task-bound Candidates, report source availability, and complete batches; they cannot inspect the complete Taste Profile, configure schedules, retract seeds, broaden Browser Grants, or approve authority changes.
- Persist all new private state in the existing Home Node store with backward-compatible migration and restart behavior. None of the new state may enter any federation serialization surface.
- Update the Stumble skill so a generic request such as “find me something interesting” reads readiness, creates Personal Discovery, lets the Home Node choose sources, uses the approved Browser Connector, and presents the Discovery Result Batch without asking the User to name platforms.

## Testing Decisions

- The primary acceptance seam is the MCP Agent Harness workflow against a persistent Home Node. It starts with interactive URL submissions, requests Personal Discovery without naming sources, lets an independently scoped worker consume only the plan and submit results, reviews the batch, records User feedback, and proves the next plan changes.
- Primary acceptance tests assert observable contracts and authorization behavior rather than internal storage layout or planner helper calls.
- Focused domain tests cover canonical Interest Seed idempotency, no-learning submissions, retraction, corroboration thresholds, negative evidence, explicit-block precedence, agent self-training prevention, cold-start readiness, deterministic plan identity, 70/30 allocation, diversity caps, recent-result suppression, and persistence across restart.
- Scheduler boundary tests extend the existing Scheduler Adapter and Discovery Task workflow precedent. They prove harness-owned and local fallback wakeups converge on the same task, schedule backpressure defers due work, on-demand work remains possible, and the results-ready event is one-shot.
- Network/privacy boundary tests extend existing discovery-substrate and federation tests. They import verified public metadata, produce locally matched Discovery Leads, and assert that private profile terms, seeds, affinities, plans, schedules, result batches, and reactions never appear in outbound Index requests or federation artifacts.
- Adapter parity tests prove the supported HTTP, MCP, and CLI surfaces return equivalent domain representations and authorization errors for profile readiness, plans, tasks, schedules, result batches, and result actions.
- Browser behavior is tested through harness-submitted provenance and source-availability contracts rather than live third-party websites. A manual acceptance check may use a User-approved authenticated X session, but CI must not depend on X, Hacker News availability, credentials, or anti-bot behavior.
- Migration tests start from a pre-feature Home Node store and prove existing Taste Profiles, Candidates, Discovery Tasks, Pods, and federation state remain valid after upgrade.
- Full validation requires formatting, warnings-as-errors linting, the complete workspace test suite, Standards review, Spec review, and an adversarial privacy check of every public serialization surface.

## Out of Scope

- Shipping dedicated X, Hacker News, browser, feed, or platform Source Connectors inside Stumble.
- Extracting, storing, or automating browser credentials, bypassing authentication or anti-bot controls, or broadening Browser Grants automatically.
- Mirroring full third-party pages or attached media bytes; Content References and permitted media references remain reference-first.
- Automatically publishing Candidates, changing Pod visibility, creating Subscriptions, accepting public Pod Placements, or modifying Pod Packages from Personal Discovery.
- Sending private profile-derived search terms to remote Index Nodes or implementing private-information-retrieval protocols.
- Treating page views, dwell time, scrolling time, silence, agent selections, or notification delivery as learning evidence.
- Building a graphical user interface. The first implementation remains headless and Agent Harness-oriented.
- Supporting infinite discovery, unbounded result queues, or repeated reminder notifications.

## Further Notes

- ADR-0035 records the User-evidence-only learning boundary.
- ADR-0036 records local, non-Pod Personal Discovery planning and minimized worker disclosure.
- Existing ADRs governing private Taste Profiles, Agent Harness ingestion, browser ownership, scheduler neutrality, local trust, and signed federation remain authoritative.
- “Scraping” is intentionally not a Stumble domain operation. The Agent Harness browses permitted sources and submits provenance-bearing Candidates; Stumble owns private planning, validation, persistence, ranking, task state, and review.
