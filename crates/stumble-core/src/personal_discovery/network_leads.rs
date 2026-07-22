//! Local network Discovery Lead production and matching.
//!
//! Reads only verified public metadata already on the Home Node. Relevance is
//! recomputed from private matching inputs held in memory for the request.

use crate::domain::*;
use crate::interest_seeds::{source_affinity_is_blocked, TasteProfileProjections};
use crate::store::InMemoryStore;
use std::collections::{BTreeSet, HashMap, HashSet};
use uuid::Uuid;

/// Maximum adjacent source neighborhoods filled from network leads.
pub(super) const MAX_ADJACENT_NETWORK_SOURCES: usize = 3;

/// Private matching inputs for local network-lead relevance.
pub(super) struct NetworkMatchContext {
    pub topics: BTreeSet<String>,
    pub sources: HashSet<SourceAffinitySignal>,
}

impl NetworkMatchContext {
    pub(super) fn from_plan(
        preferences: Option<&UserPreferences>,
        plan_topics: &[DiscoveryPlanTopic],
        plan_sources: &[DiscoveryPlanSourceNeighborhood],
        projections: &TasteProfileProjections,
    ) -> Self {
        Self {
            topics: plan_matching_topics(preferences, plan_topics, projections),
            sources: plan_matching_sources(plan_sources, projections),
        }
    }
}

/// Inserts matched network leads as adjacent neighborhoods, counting only
/// successfully inserted (non-overlapping) signals toward capacity.
pub(super) fn select_adjacent_from_matched(
    matched: impl IntoIterator<Item = DiscoveryLead>,
    seen_sources: &mut HashSet<SourceAffinitySignal>,
    adjacent_cap: usize,
) -> Vec<DiscoveryPlanSourceNeighborhood> {
    let mut adjacent = Vec::new();
    let mut adjacent_added = 0;
    for lead in matched {
        if adjacent_added >= adjacent_cap {
            break;
        }
        if !seen_sources.insert(lead.signal.clone()) {
            continue;
        }
        adjacent.push(DiscoveryPlanSourceNeighborhood {
            signal: lead.signal,
            rationale: network_lead_rationale(&lead.provenance),
            temporary: false,
            role: DiscoveryPlanSourceRole::Adjacent,
        });
        adjacent_added += 1;
    }
    adjacent
}

/// Produces private Discovery Leads from verified public metadata already local
/// to the Home Node. Never issues remote Index queries or profile-derived searches.
pub(crate) fn produce_network_discovery_leads(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
) -> Vec<DiscoveryLead> {
    let policy = store
        .trust_policies
        .get(&(user_id, tenant_id))
        .cloned()
        .unwrap_or_else(|| TrustPolicy::new(user_id, tenant_id));
    let local_node_id = store
        .node_identities
        .values()
        .find(|node| node.tenant_id == tenant_id)
        .map(|node| node.id);
    let mut leads = Vec::new();
    let mut seen = HashSet::new();

    for known in store.known_pod_announcements.values() {
        if !announcement_is_usable(store, &policy, known) {
            continue;
        }
        let announcement = &known.announcement;
        let public_topics = topic_tokens(&announcement.subject);
        let community = SourceAffinitySignal::Community(announcement.pod_slug.clone());
        push_lead(
            &mut leads,
            &mut seen,
            DiscoveryLead {
                signal: community,
                public_topics: public_topics.clone(),
                provenance: DiscoveryLeadProvenance::PodAnnouncement {
                    announcement_id: announcement.id,
                    origin_node_id: announcement.origin_node_id,
                    pod_slug: announcement.pod_slug.clone(),
                },
                local_relevance: 0.0,
            },
        );

        if let Some(samples) = store.pod_explore_sample_sets.get(&announcement.id) {
            if samples_are_usable(samples, announcement) {
                for sample in &samples.samples {
                    if policy.blocks_content_reference(sample) {
                        continue;
                    }
                    if content_reference_is_withdrawn(store, sample) {
                        continue;
                    }
                    let source = sample.source.trim().to_lowercase();
                    if source.is_empty() {
                        continue;
                    }
                    let mut sample_topics = public_topics.clone();
                    sample_topics.extend(sample.tags.iter().map(|tag| tag.to_lowercase()));
                    sample_topics.sort();
                    sample_topics.dedup();
                    push_lead(
                        &mut leads,
                        &mut seen,
                        DiscoveryLead {
                            signal: SourceAffinitySignal::Source(source.clone()),
                            public_topics: sample_topics,
                            provenance: DiscoveryLeadProvenance::ExploreSample {
                                announcement_id: announcement.id,
                                sample_artifact_id: samples.id,
                                source,
                            },
                            local_relevance: 0.0,
                        },
                    );
                }
            }
        }
    }

    for endorsement in store.pod_endorsements.values() {
        if !endorsement_is_usable(store, &policy, endorsement) {
            continue;
        }
        let Some(known) = store.known_pod_announcements.get(&(
            endorsement.endorsed_node_id,
            endorsement.endorsed_pod_slug.clone(),
        )) else {
            continue;
        };
        let public_topics = topic_tokens(&known.announcement.subject);
        push_lead(
            &mut leads,
            &mut seen,
            DiscoveryLead {
                signal: SourceAffinitySignal::Community(endorsement.endorsed_pod_slug.clone()),
                public_topics,
                provenance: DiscoveryLeadProvenance::Endorsement {
                    endorsement_id: endorsement.id,
                    endorsed_node_id: endorsement.endorsed_node_id,
                    endorsed_pod_slug: endorsement.endorsed_pod_slug.clone(),
                },
                local_relevance: 0.0,
            },
        );
    }

    if let Some(local_node_id) = local_node_id {
        let public_pods: HashMap<PodId, &Pod> = store
            .pods
            .values()
            .filter(|pod| {
                pod.tenant_id == tenant_id
                    && pod.visibility == Visibility::Public
                    && pod.origin_node_id.unwrap_or(local_node_id) == local_node_id
            })
            .map(|pod| (pod.id, pod))
            .collect();

        // Single pass over placements keyed by public pod (avoids O(|pods|×|placements|)).
        for ((content_item_id, placement_pod_id), placement) in
            &store.accepted_placement_projections
        {
            let Some(pod) = public_pods.get(placement_pod_id) else {
                continue;
            };
            if policy.blocks_pod(local_node_id, &pod.slug) {
                continue;
            }
            if placement_is_withdrawn(store, placement) {
                continue;
            }
            let Some(submission) = store.submissions.get(&Uuid::from(*content_item_id)) else {
                continue;
            };
            if policy.blocks_source_and_topics(
                &submission.domain,
                &submission.tags,
                &submission.title,
                submission.summary.as_deref(),
            ) {
                continue;
            }
            let source = submission.domain.trim().to_lowercase();
            if source.is_empty() {
                continue;
            }
            let mut sample_topics = topic_tokens(&pod.description);
            sample_topics.extend(submission.tags.iter().map(|tag| tag.to_lowercase()));
            sample_topics.sort();
            sample_topics.dedup();
            push_lead(
                &mut leads,
                &mut seen,
                DiscoveryLead {
                    signal: SourceAffinitySignal::Source(source.clone()),
                    public_topics: sample_topics,
                    provenance: DiscoveryLeadProvenance::PublicContentReference {
                        content_item_id: *content_item_id,
                        pod_id: pod.id,
                        source,
                    },
                    local_relevance: 0.0,
                },
            );
        }
    }

    leads.sort_by(|left, right| {
        left.signal
            .key()
            .cmp(&right.signal.key())
            .then_with(|| left.provenance.cmp(&right.provenance))
    });
    leads
}

/// Locally matches network leads against private interests. Remote scores are ignored.
pub(crate) fn match_network_leads_locally(
    leads: &[DiscoveryLead],
    matching: &NetworkMatchContext,
    preferences: Option<&UserPreferences>,
) -> Vec<DiscoveryLead> {
    let mut matched = leads
        .iter()
        .filter_map(|lead| {
            if preferences.is_some_and(|preferences| {
                source_affinity_is_blocked(preferences, &lead.signal)
                    || lead_topics_blocked(preferences, &lead.public_topics)
            }) {
                return None;
            }
            let local_relevance = local_lead_relevance(lead, &matching.topics, &matching.sources);
            (local_relevance > 0.0).then(|| DiscoveryLead {
                signal: lead.signal.clone(),
                public_topics: lead.public_topics.clone(),
                provenance: lead.provenance.clone(),
                local_relevance,
            })
        })
        .collect::<Vec<_>>();
    matched.sort_by(|left, right| {
        right
            .local_relevance
            .total_cmp(&left.local_relevance)
            .then_with(|| left.signal.key().cmp(&right.signal.key()))
            .then_with(|| left.provenance.cmp(&right.provenance))
    });
    // Deduplicate by signal, keeping the highest local relevance provenance.
    let mut seen = HashSet::new();
    matched
        .into_iter()
        .filter(|lead| seen.insert(lead.signal.clone()))
        .collect()
}

fn plan_matching_topics(
    preferences: Option<&UserPreferences>,
    plan_topics: &[DiscoveryPlanTopic],
    projections: &TasteProfileProjections,
) -> BTreeSet<String> {
    let mut topics = BTreeSet::new();
    for topic in plan_topics {
        for token in topic_tokens(&topic.value) {
            topics.insert(token);
        }
    }
    if let Some(preferences) = preferences {
        for interest in &preferences.interests {
            if !topic_is_blocked(Some(preferences), interest) {
                for token in topic_tokens(interest) {
                    topics.insert(token);
                }
            }
        }
    }
    for weight in &projections.learned {
        if weight.weight <= 0.0 {
            continue;
        }
        if let LearnedTasteSignal::Topic(topic) = &weight.signal {
            if !topic_is_blocked(preferences, topic) {
                for token in topic_tokens(topic) {
                    topics.insert(token);
                }
            }
        }
    }
    topics
}

fn plan_matching_sources(
    plan_sources: &[DiscoveryPlanSourceNeighborhood],
    projections: &TasteProfileProjections,
) -> HashSet<SourceAffinitySignal> {
    let mut sources = HashSet::new();
    for source in plan_sources {
        sources.insert(source.signal.clone());
    }
    for affinity in &projections.source_affinities {
        if affinity.weight > 0.0 && !affinity.explicitly_blocked {
            sources.insert(affinity.signal.clone());
        }
    }
    sources
}

fn local_lead_relevance(
    lead: &DiscoveryLead,
    matching_topics: &BTreeSet<String>,
    matching_sources: &HashSet<SourceAffinitySignal>,
) -> f32 {
    if matching_topics.is_empty() && matching_sources.is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    if matching_sources
        .iter()
        .any(|source| source.eq_ignore_ascii_case(&lead.signal))
    {
        score += 1.0;
    }
    if matching_topics.is_empty() {
        return score;
    }
    let lead_tokens = lead
        .public_topics
        .iter()
        .flat_map(|topic| topic_tokens(topic))
        .collect::<BTreeSet<_>>();
    if lead_tokens.is_empty() {
        return score;
    }
    let matched = lead_tokens
        .iter()
        .filter(|token| matching_topics.contains(token.as_str()))
        .count();
    if matched == 0 {
        return score;
    }
    let matched = u16::try_from(matched).unwrap_or(u16::MAX);
    let total = u16::try_from(matching_topics.len())
        .unwrap_or(u16::MAX)
        .max(1);
    score + f32::from(matched) / f32::from(total)
}

fn network_lead_rationale(provenance: &DiscoveryLeadProvenance) -> String {
    match provenance {
        DiscoveryLeadProvenance::PodAnnouncement { .. } => {
            "adjacent exploration from verified public Pod Announcement".into()
        }
        DiscoveryLeadProvenance::ExploreSample { .. } => {
            "adjacent exploration from verified public Explore sample".into()
        }
        DiscoveryLeadProvenance::Endorsement { .. } => {
            "adjacent exploration from verified public Pod Endorsement".into()
        }
        DiscoveryLeadProvenance::PublicContentReference { .. } => {
            "adjacent exploration from local public Content Reference".into()
        }
    }
}

fn push_lead(
    leads: &mut Vec<DiscoveryLead>,
    seen: &mut HashSet<(SourceAffinitySignal, DiscoveryLeadProvenance)>,
    lead: DiscoveryLead,
) {
    if seen.insert((lead.signal.clone(), lead.provenance.clone())) {
        leads.push(lead);
    }
}

fn announcement_is_usable(
    store: &InMemoryStore,
    policy: &TrustPolicy,
    known: &KnownPodAnnouncement,
) -> bool {
    if !crate::pod_announcement::announcement_delivery_is_active(store, known, Some(policy)) {
        return false;
    }
    if policy.blocks_announcement(&known.announcement) {
        return false;
    }
    let now = chrono::Utc::now();
    crate::pod_announcement::announcement_is_discovery_eligible(store, &known.announcement, now)
        && known.announcement.verify().unwrap_or(false)
        && store
            .known_pod_announcements
            .get(&(
                known.announcement.origin_node_id,
                known.announcement.pod_slug.clone(),
            ))
            .is_some_and(|current| current.announcement.id == known.announcement.id)
}

fn samples_are_usable(samples: &PodExploreSamples, announcement: &PodAnnouncement) -> bool {
    samples.announcement_id == announcement.id
        && samples.origin_node_id == announcement.origin_node_id
        && samples.pod_slug == announcement.pod_slug
        && samples.signer.public_key == announcement.signer.public_key
        && samples.verify().unwrap_or(false)
}

fn endorsement_is_usable(
    store: &InMemoryStore,
    policy: &TrustPolicy,
    endorsement: &PodEndorsement,
) -> bool {
    if !endorsement.verify().unwrap_or(false) {
        return false;
    }
    let endorsing_ok = store
        .known_pod_announcements
        .get(&(
            endorsement.endorsing_node_id,
            endorsement.endorsing_pod_slug.clone(),
        ))
        .is_some_and(|known| {
            known.announcement.id == endorsement.endorsing_announcement_id
                && announcement_is_usable(store, policy, known)
        });
    let endorsed_ok = store
        .known_pod_announcements
        .get(&(
            endorsement.endorsed_node_id,
            endorsement.endorsed_pod_slug.clone(),
        ))
        .is_some_and(|known| {
            known.announcement.id == endorsement.endorsed_announcement_id
                && announcement_is_usable(store, policy, known)
        });
    endorsing_ok && endorsed_ok
}

fn content_reference_is_withdrawn(store: &InMemoryStore, reference: &FeedContentReference) -> bool {
    store.placement_tombstones.iter().any(|tombstone| {
        tombstone.content_reference.content_item_id == reference.content_item_id
            || tombstone.content_reference.canonical_url == reference.canonical_url
    })
}

fn placement_is_withdrawn(store: &InMemoryStore, placement: &AcceptedPlacementProjection) -> bool {
    store.placement_tombstones.iter().any(|tombstone| {
        tombstone.origin_placement.content_item_id == placement.content_item_id
            && tombstone.origin_placement.pod_id == placement.pod_id
    })
}

fn lead_topics_blocked(preferences: &UserPreferences, topics: &[String]) -> bool {
    preferences.blocked_topics.iter().any(|blocked| {
        let blocked = blocked.to_lowercase();
        topics.iter().any(|topic| {
            topic.eq_ignore_ascii_case(&blocked) || topic.to_lowercase().contains(&blocked)
        })
    })
}

fn topic_is_blocked(preferences: Option<&UserPreferences>, topic: &str) -> bool {
    preferences.is_some_and(|preferences| {
        preferences
            .blocked_topics
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(topic.trim()))
    })
}

/// Case-insensitive discovery tokens for private matching.
fn topic_tokens(text: &str) -> Vec<String> {
    discovery_tokens(&text.to_lowercase())
}
