use crate::domain::{
    DiscoveryItem, DiscoveryMode, FeedbackEvent, FeedbackKind, Pod, PodRules, PodSkillPack,
    RecommendationExplanation, Submission, UserPreferences,
};
use crate::skill_pack::extract_yaml_list;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct RankingInput<'a> {
    pub pod: &'a Pod,
    pub rules: Option<&'a PodRules>,
    pub skill_pack: &'a PodSkillPack,
    pub submissions: Vec<&'a Submission>,
    pub preferences: Option<&'a UserPreferences>,
    pub feedback: Vec<&'a FeedbackEvent>,
    pub query: &'a str,
    pub avoid: &'a [String],
    pub mode: DiscoveryMode,
    pub limit: usize,
}

pub fn rank_discovery(input: RankingInput<'_>) -> Vec<DiscoveryItem> {
    let query_terms = terms(input.query);
    let avoid_terms: HashSet<String> = input.avoid.iter().flat_map(|s| terms(s)).collect();
    let preference_interests: HashSet<String> = input
        .preferences
        .map(|p| p.interests.iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let blocked_sources: HashSet<String> = input
        .preferences
        .map(|p| p.blocked_sources.iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let user_blocked_topics: HashSet<String> = input
        .preferences
        .map(|p| p.blocked_topics.iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let rule_blocked_topics: HashSet<String> = input
        .rules
        .map(|r| r.blocked_topics.iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let rule_blocked_domains: HashSet<String> = input
        .rules
        .map(|r| r.blocked_domains.iter().map(|s| s.to_lowercase()).collect())
        .unwrap_or_default();
    let positive_signals: HashSet<String> =
        extract_yaml_list(&input.skill_pack.pod_yaml, "positive_signals")
            .into_iter()
            .collect();
    let negative_signals: HashSet<String> =
        extract_yaml_list(&input.skill_pack.pod_yaml, "negative_signals")
            .into_iter()
            .chain(extract_yaml_list(
                &input.skill_pack.filters_yaml,
                "downrank",
            ))
            .collect();
    let dismissed: HashSet<_> = input
        .feedback
        .iter()
        .filter(|f| {
            matches!(
                f.event_type,
                FeedbackKind::Dismissed | FeedbackKind::NotForMe
            )
        })
        .map(|f| f.submission_id)
        .collect();
    let saved: HashSet<_> = input
        .feedback
        .iter()
        .filter(|f| {
            matches!(
                f.event_type,
                FeedbackKind::Saved | FeedbackKind::Interesting
            )
        })
        .map(|f| f.submission_id)
        .collect();

    let mut scored = Vec::new();
    for submission in input.submissions {
        let haystack = format!(
            "{} {} {} {} {}",
            submission.title,
            submission.description.clone().unwrap_or_default(),
            submission.summary.clone().unwrap_or_default(),
            submission.submitter_note.clone().unwrap_or_default(),
            submission.tags.join(" ")
        )
        .to_lowercase();
        let domain = submission.domain.to_lowercase();
        if blocked_sources.contains(&domain) || rule_blocked_domains.contains(&domain) {
            continue;
        }
        if user_blocked_topics.iter().any(|t| haystack.contains(t))
            || rule_blocked_topics.iter().any(|t| haystack.contains(t))
            || avoid_terms.iter().any(|t| haystack.contains(t))
        {
            continue;
        }
        let mut score = 1.0_f32;
        let mut matched_interests = Vec::new();
        let mut matched_pod_signals = Vec::new();
        for term in &query_terms {
            if haystack.contains(term) {
                score += 2.0;
            }
        }
        for interest in &preference_interests {
            if haystack.contains(interest)
                || submission
                    .tags
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(interest))
            {
                score += 1.25;
                matched_interests.push(interest.clone());
            }
        }
        for signal in &positive_signals {
            if haystack.contains(signal) {
                score += 1.5;
                matched_pod_signals.push(signal.clone());
            }
        }
        for signal in &negative_signals {
            if haystack.contains(signal) {
                score -= 2.0;
            }
        }
        if submission
            .submitter_note
            .as_ref()
            .is_some_and(|n| !n.trim().is_empty())
        {
            score += 1.5;
        }
        if saved.contains(&submission.id) {
            score += 2.0;
        }
        if dismissed.contains(&submission.id) {
            score -= 3.0;
        }
        match input.mode {
            DiscoveryMode::OldGem => {
                let age_days = (chrono::Utc::now() - submission.created_at)
                    .num_days()
                    .max(0) as f32;
                score += (age_days / 180.0).min(2.0);
            }
            DiscoveryMode::HumanPick => {
                if !submission.discovered_by_crawler {
                    score += 2.0;
                }
            }
            DiscoveryMode::Adjacent | DiscoveryMode::RabbitHole | DiscoveryMode::Stumble => {
                score += stable_jitter(&submission.canonical_url);
            }
            DiscoveryMode::DeepMatch => {}
        }
        if score <= 0.0 {
            continue;
        }
        let origin = if submission.discovered_by_crawler {
            "crawler-discovered"
        } else {
            "human-submitted"
        };
        let explanation = RecommendationExplanation {
            matched_interests,
            matched_pod_signals,
            blocked_or_downranked_signals_avoided: negative_signals.iter().cloned().collect(),
            source_reason: format!("Domain {} is allowed for this pod.", submission.domain),
            novelty_reason: match input.mode {
                DiscoveryMode::OldGem => "Old gem mode boosted durable older links.".to_string(),
                DiscoveryMode::Stumble => "Stumble mode added controlled randomness.".to_string(),
                _ => {
                    "Not filtered by reading history or blocked topics in this request.".to_string()
                }
            },
            human_or_crawler_origin: origin.to_string(),
            final_score: score,
        };
        scored.push((
            score,
            DiscoveryItem {
                title: submission.title.clone(),
                url: submission.url.clone(),
                short_summary: submission
                    .summary
                    .clone()
                    .or_else(|| submission.description.clone())
                    .unwrap_or_else(|| "No summary available yet.".to_string()),
                why_matches_request: if query_terms.iter().any(|t| haystack.contains(t)) {
                    format!("Matches request terms from '{}'.", input.query)
                } else {
                    "Adjacent to the request through pod taste and source quality.".to_string()
                },
                why_belongs_in_pod: format!(
                    "Fits the '{}' pod because it aligns with the configured skill pack and curation rules.",
                    input.pod.name
                ),
                source: submission.domain.clone(),
                origin: origin.to_string(),
                recommendation_explanation: explanation,
                submission_id: submission.id,
            },
        ));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .take(input.limit.clamp(1, 10))
        .map(|(_, item)| item)
        .collect()
}

fn terms(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|s| s.len() > 2)
        .map(|s| s.to_lowercase())
        .collect()
}

fn stable_jitter(value: &str) -> f32 {
    let sum = value
        .bytes()
        .fold(0_u32, |acc, b| acc.wrapping_add(b as u32));
    (sum % 100) as f32 / 100.0
}
