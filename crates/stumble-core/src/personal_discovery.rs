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
            let parsed = Url::parse(&canonical).map_err(|error| {
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
            Some(PreparedPersonalDiscoveryIntent::SimilarToUrl {
                value: url.clone(),
                domain,
            })
        }
        None => None,
    };
    Ok(PreparedPersonalDiscoveryRequest {
        result_count,
        intent,
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
        .into_iter()
        .filter(|affinity| affinity.weight > 0.0 && !affinity.explicitly_blocked)
        .map(|affinity| RankedSource {
            priority: 1,
            weight: affinity.weight,
            source: DiscoveryPlanSourceNeighborhood {
                signal: affinity.signal,
                rationale: "corroborated aggregate User evidence".into(),
                temporary: false,
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

    let proven = (result_count.saturating_mul(7).saturating_add(9)) / 10;
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
            blocked_topics: preferences
                .map(|preferences| preferences.blocked_topics.clone())
                .unwrap_or_default(),
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
}
