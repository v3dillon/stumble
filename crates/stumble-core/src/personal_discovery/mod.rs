mod network_leads;
mod result_batches;
mod review;
pub(crate) mod schedules;
pub(crate) mod source_availability;

pub(crate) use result_batches::build_discovery_result_batch;
pub(crate) use result_batches::BatchAvailabilityInput;
pub(crate) use review::{
    clear_discovery_result_learning, discovery_result_allowed_actions, ensure_private_inbox,
    record_discovery_result_learning, set_discovery_result_learning_link,
    DiscoveryResultLearningInput,
};
pub(crate) use schedules::{
    ensure_results_ready_event, materialize_due_personal_schedules, normalize_intent,
    notification_state_for_schedule, schedule_status, validate_name, validate_result_count,
};
pub(crate) use source_availability::{
    evaluate_authentication_notices, filter_neighborhoods_by_browser_grant,
    normalize_browser_grant_eligibility, normalize_reports, resolve_completion_reports,
    task_is_scheduled, upsert_task_source_availability, TaskAvailabilityIdentity,
};

use crate::agent_tools::AgentToolsError;
use crate::domain::*;
use crate::interest_seeds::{source_affinity_is_blocked, taste_profile_projections};
use crate::store::{InMemoryStore, StoreError};
use chrono::Utc;
use std::collections::HashSet;
use url::Url;
use uuid::Uuid;

const MAX_SELECTED_TOPICS: usize = 5;
const MAX_SELECTED_SOURCE_NEIGHBORHOODS: usize = 5;

pub(crate) struct PreparedPersonalDiscoveryRequest {
    pub(crate) result_count: u16,
    intent: Option<PreparedPersonalDiscoveryIntent>,
    focus_topics: Vec<String>,
    avoid_topics: Vec<String>,
    /// Optional Browser Grant eligibility that restricts planned neighborhoods.
    browser_grant_eligible_sources: Option<Vec<String>>,
}

enum PreparedPersonalDiscoveryIntent {
    Topic(String),
    SimilarToUrl { value: String, domain: String },
}

impl PreparedPersonalDiscoveryRequest {
    pub(crate) fn persisted_intent(&self) -> Option<PersonalDiscoveryIntent> {
        self.intent.as_ref().map(|intent| match intent {
            PreparedPersonalDiscoveryIntent::Topic(value) => {
                PersonalDiscoveryIntent::Topic(value.clone())
            }
            PreparedPersonalDiscoveryIntent::SimilarToUrl { value, .. } => {
                PersonalDiscoveryIntent::SimilarToUrl(value.clone())
            }
        })
    }
}

pub(crate) fn readiness(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
) -> PersonalDiscoveryReadiness {
    let preferences = store.user_preferences.get(&(user_id, tenant_id));
    let projections = taste_profile_projections(store, user_id, tenant_id, preferences);
    let mut basis = preferences
        .into_iter()
        .flat_map(|preferences| &preferences.interests)
        .filter(|topic| {
            !preferences.is_some_and(|preferences| {
                preferences
                    .blocked_topics
                    .iter()
                    .any(|blocked| blocked.eq_ignore_ascii_case(topic))
            })
        })
        .map(|topic| DiscoveryPlanBasis::ExplicitTopic(topic.clone()))
        .collect::<Vec<_>>();
    basis.extend(projections.learned.into_iter().filter_map(|weight| {
        (weight.weight > 0.0).then(|| match weight.signal {
            LearnedTasteSignal::Topic(topic) => DiscoveryPlanBasis::CorroboratedTopic(topic),
            _ => unreachable!("source signals are projected separately"),
        })
    }));
    basis.extend(
        projections
            .source_affinities
            .into_iter()
            .filter(|affinity| affinity.weight > 0.0 && !affinity.explicitly_blocked)
            .map(|affinity| DiscoveryPlanBasis::CorroboratedSource(affinity.signal)),
    );
    basis.sort_by_key(|item| serde_json::to_string(item).expect("basis is serializable"));
    PersonalDiscoveryReadiness {
        ready: !basis.is_empty(),
        basis,
    }
}

pub(crate) fn prepare_request(
    request: &RequestPersonalDiscovery,
) -> Result<PreparedPersonalDiscoveryRequest, AgentToolsError> {
    if request.idempotency_key.trim().is_empty() {
        return Err(StoreError::Validation("idempotency key must not be empty".into()).into());
    }
    let result_count = request.result_count.unwrap_or(10);
    if !(1..=100).contains(&result_count) {
        return Err(StoreError::Validation(
            "Personal Discovery result count must be between 1 and 100".into(),
        )
        .into());
    }
    let intent = match request.intent.as_ref() {
        Some(PersonalDiscoveryIntent::Topic(topic)) if topic.trim().is_empty() => {
            return Err(StoreError::Validation("temporary topic must not be empty".into()).into());
        }
        Some(PersonalDiscoveryIntent::Topic(topic)) => {
            Some(PreparedPersonalDiscoveryIntent::Topic(topic.clone()))
        }
        Some(PersonalDiscoveryIntent::SimilarToUrl(url)) => {
            let canonical = canonicalize_web_url(url).map_err(|error| {
                StoreError::Validation(format!("invalid temporary reference: {error}"))
            })?;
            let mut parsed = Url::parse(&canonical).map_err(|error| {
                StoreError::Validation(format!("invalid temporary reference: {error}"))
            })?;
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(StoreError::Validation(
                    "temporary reference must not include credentials".into(),
                )
                .into());
            }
            let domain = parsed.domain().map(str::to_lowercase).ok_or_else(|| {
                StoreError::Validation("temporary reference has no domain".into())
            })?;
            parsed.set_query(None);
            parsed.set_fragment(None);
            Some(PreparedPersonalDiscoveryIntent::SimilarToUrl {
                value: parsed.to_string(),
                domain,
            })
        }
        None => None,
    };
    let browser_grant_eligible_sources = source_availability::normalize_browser_grant_eligibility(
        request.browser_grant_eligible_sources.clone(),
    )
    .map_err(StoreError::Validation)?;
    Ok(PreparedPersonalDiscoveryRequest {
        result_count,
        intent,
        focus_topics: Vec::new(),
        avoid_topics: Vec::new(),
        browser_grant_eligible_sources,
    })
}

/// Prepares a Personal Discovery run from a schedule's batch size and temporary intent.
///
/// Schedules never receive Browser Grant eligibility from Taste Profile, Pod Package,
/// or Discovery Leads; unattended workers report eligibility at execution time.
pub(crate) fn prepare_schedule_run(
    schedule: &PersonalDiscoverySchedule,
) -> Result<PreparedPersonalDiscoveryRequest, AgentToolsError> {
    let result_count =
        validate_result_count(schedule.result_count).map_err(StoreError::Validation)?;
    let intent = normalize_intent(schedule.intent.clone()).map_err(StoreError::Validation)?;
    Ok(PreparedPersonalDiscoveryRequest {
        result_count,
        intent: None,
        focus_topics: intent.focus_topics,
        avoid_topics: intent.avoid_topics,
        browser_grant_eligible_sources: None,
    })
}

pub(crate) fn retry(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    idempotency_key: &str,
    requested_by: Option<AgentHarnessId>,
) -> Option<RequestedPersonalDiscovery> {
    store.discovery_tasks.values().find_map(|task| {
        let DiscoveryTaskOrigin::PersonalRequest {
            idempotency_key: stored_idempotency_key,
            requested_by: stored_requested_by,
        } = &task.origin
        else {
            return None;
        };
        let plan = task
            .target
            .discovery_plan_id()
            .and_then(|plan_id| store.discovery_plans.get(&plan_id))?;
        (stored_idempotency_key == idempotency_key
            && *stored_requested_by == requested_by
            && plan.user_id == user_id
            && plan.tenant_id == tenant_id)
            .then(|| RequestedPersonalDiscovery {
                plan: plan.clone(),
                task: task.clone(),
            })
    })
}

pub(crate) fn build_plan(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    request: PreparedPersonalDiscoveryRequest,
    now: chrono::DateTime<Utc>,
) -> Result<DiscoveryPlan, AgentToolsError> {
    let result_count = request.result_count;
    let persisted_intent = request.persisted_intent();
    let intent = request.intent;
    let preferences = store.user_preferences.get(&(user_id, tenant_id));
    let projections = taste_profile_projections(store, user_id, tenant_id, preferences);
    let mut ranked_topics = Vec::new();
    if let Some(preferences) = preferences {
        ranked_topics.extend(
            preferences
                .interests
                .iter()
                .filter(|topic| !topic_is_blocked(Some(preferences), topic))
                .map(|topic| RankedTopic {
                    priority: 2,
                    weight: f32::INFINITY,
                    topic: DiscoveryPlanTopic {
                        value: topic.clone(),
                        rationale: "explicit User interest".into(),
                        temporary: false,
                    },
                }),
        );
    }
    ranked_topics.extend(projections.learned.iter().filter_map(|weight| {
        let LearnedTasteSignal::Topic(topic) = &weight.signal else {
            return None;
        };
        (weight.weight > 0.0).then(|| RankedTopic {
            priority: 1,
            weight: weight.weight,
            topic: DiscoveryPlanTopic {
                value: topic.clone(),
                rationale: "corroborated aggregate User evidence".into(),
                temporary: false,
            },
        })
    }));
    if let Some(PreparedPersonalDiscoveryIntent::Topic(topic)) = &intent {
        if topic_is_blocked(preferences, topic) {
            return Err(
                StoreError::Validation("temporary topic is explicitly blocked".into()).into(),
            );
        }
        ranked_topics.push(RankedTopic {
            priority: 3,
            weight: f32::INFINITY,
            topic: DiscoveryPlanTopic {
                value: topic.trim().to_lowercase(),
                rationale: "temporary intent for this run".into(),
                temporary: true,
            },
        });
    }
    for topic in &request.focus_topics {
        if topic_is_blocked(preferences, topic)
            || request
                .avoid_topics
                .iter()
                .any(|avoid| avoid.eq_ignore_ascii_case(topic))
        {
            continue;
        }
        ranked_topics.push(RankedTopic {
            priority: 3,
            weight: f32::INFINITY,
            topic: DiscoveryPlanTopic {
                value: topic.trim().to_lowercase(),
                rationale: "temporary schedule focus for this run".into(),
                temporary: true,
            },
        });
    }
    ranked_topics.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| right.weight.total_cmp(&left.weight))
            .then_with(|| left.topic.value.cmp(&right.topic.value))
    });
    let mut seen_topics = HashSet::new();
    let mut topics = ranked_topics
        .into_iter()
        .filter(|candidate| seen_topics.insert(candidate.topic.value.to_lowercase()))
        .map(|candidate| candidate.topic)
        .collect::<Vec<_>>();
    topics.truncate(MAX_SELECTED_TOPICS);

    let mut ranked_sources = projections
        .source_affinities
        .iter()
        .filter(|affinity| affinity.weight > 0.0 && !affinity.explicitly_blocked)
        .map(|affinity| RankedSource {
            priority: 1,
            weight: affinity.weight,
            source: DiscoveryPlanSourceNeighborhood {
                signal: affinity.signal.clone(),
                rationale: "corroborated aggregate User evidence".into(),
                temporary: false,
                role: DiscoveryPlanSourceRole::Proven,
            },
        })
        .collect::<Vec<_>>();
    if let Some(PreparedPersonalDiscoveryIntent::SimilarToUrl { domain, .. }) = &intent {
        let signal = SourceAffinitySignal::Source(domain.clone());
        if preferences.is_some_and(|preferences| source_affinity_is_blocked(preferences, &signal)) {
            return Err(StoreError::Validation(
                "temporary reference source is explicitly blocked".into(),
            )
            .into());
        }
        ranked_sources.push(RankedSource {
            priority: 3,
            weight: f32::INFINITY,
            source: DiscoveryPlanSourceNeighborhood {
                signal,
                rationale: "temporary similar-link intent for this run".into(),
                temporary: true,
                role: DiscoveryPlanSourceRole::Proven,
            },
        });
    }
    ranked_sources.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| right.weight.total_cmp(&left.weight))
            .then_with(|| left.source.signal.key().cmp(&right.source.signal.key()))
    });
    let mut seen_sources = HashSet::new();
    let mut source_neighborhoods = ranked_sources
        .into_iter()
        .filter(|candidate| seen_sources.insert(candidate.source.signal.clone()))
        .map(|candidate| candidate.source)
        .collect::<Vec<_>>();
    source_neighborhoods.truncate(MAX_SELECTED_SOURCE_NEIGHBORHOODS);
    seen_sources = source_neighborhoods
        .iter()
        .map(|source| source.signal.clone())
        .collect();

    // Network leads only occupy remaining neighborhood slots and only as adjacent.
    // Capacity counts successfully inserted neighborhoods only so proven-signal
    // collisions do not underfill adjacent slots.
    let adjacent_slots =
        MAX_SELECTED_SOURCE_NEIGHBORHOODS.saturating_sub(source_neighborhoods.len());
    let adjacent_cap = adjacent_slots.min(network_leads::MAX_ADJACENT_NETWORK_SOURCES);
    if adjacent_cap > 0 {
        let matching = network_leads::NetworkMatchContext::from_plan(
            preferences,
            &topics,
            &source_neighborhoods,
            &projections,
        );
        let leads = network_leads::produce_network_discovery_leads(store, user_id, tenant_id);
        let matched = network_leads::match_network_leads_locally(&leads, &matching, preferences);
        source_neighborhoods.extend(network_leads::select_adjacent_from_matched(
            matched,
            &mut seen_sources,
            adjacent_cap,
        ));
    }

    // Browser Grant eligibility restricts after taste/lead selection and never expands
    // from those signals. Unreported eligibility leaves neighborhoods unrestricted.
    source_neighborhoods = filter_neighborhoods_by_browser_grant(
        source_neighborhoods,
        request.browser_grant_eligible_sources.as_deref(),
    );

    let proven = (result_count.saturating_mul(7).saturating_add(9)) / 10;
    let mut blocked_topics = preferences
        .map(|preferences| preferences.blocked_topics.clone())
        .unwrap_or_default();
    for topic in &request.avoid_topics {
        if !blocked_topics
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(topic))
        {
            blocked_topics.push(topic.clone());
        }
    }
    Ok(DiscoveryPlan {
        id: Uuid::now_v7().into(),
        user_id,
        tenant_id,
        result_count,
        topics,
        source_neighborhoods,
        allocation: DiscoveryPlanAllocation {
            proven,
            adjacent: result_count - proven,
        },
        constraints: DiscoveryPlanConstraints {
            max_per_domain: 3,
            max_per_author_or_account: 2,
            max_per_publisher: 2,
            max_per_community: 2,
            canonical_deduplication: true,
            suppress_recently_reviewed: true,
            blocked_topics,
            blocked_sources: preferences
                .map(|preferences| preferences.blocked_sources.clone())
                .unwrap_or_default(),
            blocked_source_affinities: preferences
                .map(|preferences| preferences.blocked_source_affinities.clone())
                .unwrap_or_default(),
        },
        intent: persisted_intent,
        created_at: now,
    })
}

struct RankedTopic {
    priority: u8,
    weight: f32,
    topic: DiscoveryPlanTopic,
}

struct RankedSource {
    priority: u8,
    weight: f32,
    source: DiscoveryPlanSourceNeighborhood,
}

fn topic_is_blocked(preferences: Option<&UserPreferences>, topic: &str) -> bool {
    preferences.is_some_and(|preferences| {
        preferences
            .blocked_topics
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(topic.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_preparation_rejects_credential_bearing_temporary_urls() {
        let result = prepare_request(&RequestPersonalDiscovery {
            intent: Some(PersonalDiscoveryIntent::SimilarToUrl(
                "https://user:password@example.com/article".into(),
            )),
            result_count: None,
            idempotency_key: "credential-bearing-url".into(),
            browser_grant_eligible_sources: None,
        });

        assert!(matches!(
            result,
            Err(AgentToolsError::Store(StoreError::Validation(message)))
                if message.contains("credentials")
        ));
    }

    #[test]
    fn strongest_source_affinities_survive_minimization() {
        let user_id = Uuid::now_v7();
        let mut store = InMemoryStore::default();
        for (index, source) in [
            "a.example",
            "b.example",
            "c.example",
            "d.example",
            "e.example",
            "f.example",
        ]
        .into_iter()
        .enumerate()
        {
            for _ in 0..(index + 2) {
                store.taste_learning_evidence.push(TasteLearningEvidence {
                    id: Uuid::now_v7(),
                    user_id,
                    tenant_id: None,
                    signal: LearnedTasteSignal::Source(source.into()),
                    kind: LearnedTasteEvidenceKind::MoreLikeThis,
                    direction: TasteEvidenceDirection::Supporting,
                    created_at: Utc::now(),
                });
            }
        }

        let plan = build_plan(
            &store,
            user_id,
            None,
            PreparedPersonalDiscoveryRequest {
                result_count: 10,
                intent: None,
                focus_topics: Vec::new(),
                avoid_topics: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
        let selected = plan
            .source_neighborhoods
            .iter()
            .map(|source| source.signal.key().1)
            .collect::<Vec<_>>();

        assert_eq!(selected.len(), 5);
        assert!(!selected.contains(&"a.example"));
        assert!(selected.contains(&"f.example"));
    }

    #[test]
    fn browser_grant_eligibility_restricts_plan_and_is_not_broadened_by_taste() {
        let user_id = Uuid::now_v7();
        let mut store = InMemoryStore::default();
        for source in ["taste-a.example", "taste-b.example", "open.example"] {
            for _ in 0..3 {
                store.taste_learning_evidence.push(TasteLearningEvidence {
                    id: Uuid::now_v7(),
                    user_id,
                    tenant_id: None,
                    signal: LearnedTasteSignal::Source(source.into()),
                    kind: LearnedTasteEvidenceKind::MoreLikeThis,
                    direction: TasteEvidenceDirection::Supporting,
                    created_at: Utc::now(),
                });
            }
        }

        let plan = build_plan(
            &store,
            user_id,
            None,
            PreparedPersonalDiscoveryRequest {
                result_count: 10,
                intent: None,
                focus_topics: Vec::new(),
                avoid_topics: Vec::new(),
                browser_grant_eligible_sources: Some(vec!["open.example".into()]),
            },
            Utc::now(),
        )
        .unwrap();
        let selected: Vec<_> = plan
            .source_neighborhoods
            .iter()
            .map(|source| source.signal.key().1.to_string())
            .collect();
        assert_eq!(selected, vec!["open.example".to_string()]);
    }

    #[test]
    fn adjacent_slots_fill_despite_proven_overlap_with_top_network_leads() {
        // When a top-ranked network lead equals a proven SourceAffinity, adjacent
        // fill must still reach min(remaining, 3) with only non-proven signals.
        let user_id = Uuid::now_v7();
        let node_id = Uuid::now_v7();
        let mut store = InMemoryStore::default();
        store.user_preferences.insert(
            (user_id, None),
            UserPreferences {
                user_id,
                tenant_id: None,
                interests: vec!["distributed systems".into()],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            },
        );
        // Proven SourceAffinity equal to the top network sample source.
        for _ in 0..4 {
            store.taste_learning_evidence.push(TasteLearningEvidence {
                id: Uuid::now_v7(),
                user_id,
                tenant_id: None,
                signal: LearnedTasteSignal::Source("allowed.example".into()),
                kind: LearnedTasteEvidenceKind::MoreLikeThis,
                direction: TasteEvidenceDirection::Supporting,
                created_at: Utc::now(),
            });
        }

        // Distinct network leads that match the interest; one collides with proven.
        let leads = vec![
            DiscoveryLead {
                signal: SourceAffinitySignal::Source("allowed.example".into()),
                public_topics: vec!["systems".into(), "distributed".into()],
                provenance: DiscoveryLeadProvenance::ExploreSample {
                    announcement_id: Uuid::now_v7(),
                    sample_artifact_id: Uuid::now_v7(),
                    source: "allowed.example".into(),
                },
                local_relevance: 0.0,
            },
            DiscoveryLead {
                signal: SourceAffinitySignal::Source("network-a.example".into()),
                public_topics: vec!["systems".into()],
                provenance: DiscoveryLeadProvenance::ExploreSample {
                    announcement_id: Uuid::now_v7(),
                    sample_artifact_id: Uuid::now_v7(),
                    source: "network-a.example".into(),
                },
                local_relevance: 0.0,
            },
            DiscoveryLead {
                signal: SourceAffinitySignal::Source("network-b.example".into()),
                public_topics: vec!["systems".into()],
                provenance: DiscoveryLeadProvenance::ExploreSample {
                    announcement_id: Uuid::now_v7(),
                    sample_artifact_id: Uuid::now_v7(),
                    source: "network-b.example".into(),
                },
                local_relevance: 0.0,
            },
            DiscoveryLead {
                signal: SourceAffinitySignal::Community("rust-systems".into()),
                public_topics: vec!["systems".into(), "distributed".into()],
                provenance: DiscoveryLeadProvenance::PodAnnouncement {
                    announcement_id: Uuid::now_v7(),
                    origin_node_id: node_id,
                    pod_slug: "rust-systems".into(),
                },
                local_relevance: 0.0,
            },
        ];
        let preferences = store.user_preferences.get(&(user_id, None));
        let projections =
            crate::interest_seeds::taste_profile_projections(&store, user_id, None, preferences);
        let matching = network_leads::NetworkMatchContext {
            topics: std::collections::BTreeSet::from([
                "distributed".to_string(),
                "systems".to_string(),
            ]),
            sources: projections
                .source_affinities
                .iter()
                .filter(|affinity| affinity.weight > 0.0)
                .map(|affinity| affinity.signal.clone())
                .collect(),
        };
        assert!(matching
            .sources
            .contains(&SourceAffinitySignal::Source("allowed.example".into())));

        let matched = network_leads::match_network_leads_locally(&leads, &matching, preferences);
        // Proven overlap ranks highest; take-before-skip would underfill.
        assert_eq!(
            matched.first().map(|lead| &lead.signal),
            Some(&SourceAffinitySignal::Source("allowed.example".into()))
        );

        let mut seen_sources =
            HashSet::from([SourceAffinitySignal::Source("allowed.example".into())]);
        let remaining = MAX_SELECTED_SOURCE_NEIGHBORHOODS.saturating_sub(1);
        let adjacent_cap = remaining.min(network_leads::MAX_ADJACENT_NETWORK_SOURCES);
        let adjacent =
            network_leads::select_adjacent_from_matched(matched, &mut seen_sources, adjacent_cap);

        assert_eq!(adjacent.len(), adjacent_cap);
        assert!(adjacent.iter().all(|source| {
            source.role == DiscoveryPlanSourceRole::Adjacent
                && source.signal != SourceAffinitySignal::Source("allowed.example".into())
                && source.rationale.contains("adjacent exploration from")
        }));
        let adjacent_signals: HashSet<_> = adjacent
            .iter()
            .map(|source| source.signal.clone())
            .collect();
        assert!(
            adjacent_signals.contains(&SourceAffinitySignal::Source("network-a.example".into()))
        );
        assert!(
            adjacent_signals.contains(&SourceAffinitySignal::Source("network-b.example".into()))
        );
        assert!(adjacent_signals.contains(&SourceAffinitySignal::Community("rust-systems".into())));
    }
}
