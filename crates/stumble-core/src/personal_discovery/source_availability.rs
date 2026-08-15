//! Privacy-safe planned source availability and authentication-needed notices.
//!
//! Stumble stores availability facts only. Credentials, cookies, tokens, and raw
//! browser state never enter the Home Node. The Agent Harness owns login and
//! Browser Connector control under User-approved Browser Grants.

use crate::domain::*;
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const MAX_SOURCE_LEN: usize = 200;
const MAX_REASON_LEN: usize = 500;
const MAX_REPORTS: usize = 64;
const MAX_ELIGIBLE_SOURCES: usize = 64;

/// Forbidden substrings that must never appear in worker-supplied availability text.
const FORBIDDEN_AUTH_MARKERS: &[&str] = &[
    "password=",
    "passwd=",
    "cookie:",
    "set-cookie",
    "authorization:",
    "bearer ",
    "api_key=",
    "apikey=",
    "secret=",
    "session_token",
    "access_token",
    "refresh_token",
    "raw_browser",
    "cdp_session",
];

/// Normalizes a generic source locator for comparison and storage.
pub(crate) fn normalize_source_locator(source: &str) -> Result<String, String> {
    let value = source.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err("source locator must not be empty".into());
    }
    if value.len() > MAX_SOURCE_LEN {
        return Err(format!(
            "source locator must be at most {MAX_SOURCE_LEN} characters"
        ));
    }
    if contains_forbidden_auth_material(&value) {
        return Err("source locator must not contain authentication material".into());
    }
    Ok(value)
}

/// Validates a freeform inspectable reason without accepting auth material.
pub(crate) fn normalize_reason(reason: &str) -> Result<String, String> {
    let value = reason.trim().to_string();
    if value.len() > MAX_REASON_LEN {
        return Err(format!(
            "availability reason must be at most {MAX_REASON_LEN} characters"
        ));
    }
    if contains_forbidden_auth_material(&value.to_ascii_lowercase()) {
        return Err("availability reason must not contain authentication material".into());
    }
    Ok(value)
}

fn contains_forbidden_auth_material(value: &str) -> bool {
    FORBIDDEN_AUTH_MARKERS
        .iter()
        .any(|marker| value.contains(marker))
}

/// Validates and normalizes worker-reported availability facts.
pub(crate) fn normalize_reports(
    reports: Vec<ReportedSourceAvailability>,
) -> Result<Vec<ReportedSourceAvailability>, String> {
    if reports.len() > MAX_REPORTS {
        return Err(format!(
            "at most {MAX_REPORTS} source availability reports per request"
        ));
    }
    let mut by_source: HashMap<String, ReportedSourceAvailability> = HashMap::new();
    for report in reports {
        let source = normalize_source_locator(&report.source)?;
        let reason = normalize_reason(&report.reason)?;
        by_source.insert(
            source.clone(),
            ReportedSourceAvailability {
                source,
                state: report.state,
                reason,
            },
        );
    }
    let mut normalized: Vec<_> = by_source.into_values().collect();
    normalized.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(normalized)
}

/// Validates an optional Browser Grant eligibility set.
///
/// Eligibility only restricts. Callers must never expand this from Taste Profile,
/// Pod Package, Discovery Lead, Index metadata, or worker content.
pub(crate) fn normalize_browser_grant_eligibility(
    eligible: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, String> {
    let Some(eligible) = eligible else {
        return Ok(None);
    };
    if eligible.len() > MAX_ELIGIBLE_SOURCES {
        return Err(format!(
            "at most {MAX_ELIGIBLE_SOURCES} Browser Grant eligible sources"
        ));
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for source in eligible {
        let source = normalize_source_locator(&source)?;
        if seen.insert(source.clone()) {
            normalized.push(source);
        }
    }
    normalized.sort();
    Ok(Some(normalized))
}

/// True when `source` is allowed under an optional Browser Grant eligibility set.
///
/// When eligibility is unreported (`None`), no Browser Grant restriction is applied.
pub(crate) fn source_is_browser_grant_eligible(source: &str, eligible: Option<&[String]>) -> bool {
    match eligible {
        None => true,
        Some(list) => list.iter().any(|item| item.eq_ignore_ascii_case(source)),
    }
}

/// Filters plan source neighborhoods to the Browser Grant eligibility set.
///
/// Taste Profile, Pod Package, and Discovery Lead selection never broaden eligibility.
pub(crate) fn filter_neighborhoods_by_browser_grant(
    neighborhoods: Vec<DiscoveryPlanSourceNeighborhood>,
    eligible: Option<&[String]>,
) -> Vec<DiscoveryPlanSourceNeighborhood> {
    let Some(eligible) = eligible else {
        return neighborhoods;
    };
    neighborhoods
        .into_iter()
        .filter(|neighborhood| {
            let locator = neighborhood.signal.key().1;
            source_is_browser_grant_eligible(locator, Some(eligible))
        })
        .collect()
}

/// Merges a new report set with any prior lease-scoped snapshot (later wins per source).
pub(crate) fn merge_reports(
    prior: &[ReportedSourceAvailability],
    incoming: Vec<ReportedSourceAvailability>,
) -> Vec<ReportedSourceAvailability> {
    let mut by_source: HashMap<String, ReportedSourceAvailability> = prior
        .iter()
        .cloned()
        .map(|report| (report.source.clone(), report))
        .collect();
    for report in incoming {
        by_source.insert(report.source.clone(), report);
    }
    let mut merged: Vec<_> = by_source.into_values().collect();
    merged.sort_by(|left, right| left.source.cmp(&right.source));
    merged
}

/// Applies Browser Grant eligibility: ineligible planned sources cannot stay Available.
pub(crate) fn enforce_browser_grant_on_reports(
    reports: Vec<ReportedSourceAvailability>,
    eligible: Option<&[String]>,
) -> Vec<ReportedSourceAvailability> {
    reports
        .into_iter()
        .map(|mut report| {
            if report.state.is_available()
                && !source_is_browser_grant_eligible(&report.source, eligible)
            {
                report.state = SourceAvailabilityState::BrowserGrantIneligible;
                if report.reason.is_empty() {
                    report.reason = "outside Browser Grant eligibility".into();
                }
            }
            report
        })
        .collect()
}

/// Whether this Personal Discovery task is scheduled (never waits for authentication).
pub(crate) fn task_is_scheduled(task: &DiscoveryTask) -> bool {
    matches!(task.origin, DiscoveryTaskOrigin::PersonalScheduled { .. })
}

/// Builds batch availability reasons from normalized reports and run mode.
pub(crate) fn availability_reasons_for_batch(
    reports: &[ReportedSourceAvailability],
    scheduled: bool,
) -> Vec<DiscoveryResultAvailabilityReason> {
    let mut reasons = Vec::new();
    for report in reports {
        if report.state.is_available() {
            continue;
        }
        let reason = if report.reason.is_empty() {
            report.state.fingerprint_label().to_string()
        } else {
            report.reason.clone()
        };
        match report.state {
            SourceAvailabilityState::BrowserGrantIneligible => {
                reasons.push(DiscoveryResultAvailabilityReason::BrowserGrantIneligible {
                    source: report.source.clone(),
                    reason,
                });
            }
            SourceAvailabilityState::AuthenticationRequired
            | SourceAvailabilityState::SessionExpired
                if scheduled =>
            {
                reasons.push(
                    DiscoveryResultAvailabilityReason::AuthenticationSkippedScheduled {
                        source: report.source.clone(),
                        reason,
                    },
                );
            }
            SourceAvailabilityState::AuthenticationRequired
            | SourceAvailabilityState::SessionExpired => {
                reasons.push(
                    DiscoveryResultAvailabilityReason::AuthenticationAssistanceRequested {
                        source: report.source.clone(),
                        reason,
                    },
                );
            }
            SourceAvailabilityState::Inaccessible | SourceAvailabilityState::Available => {
                reasons.push(DiscoveryResultAvailabilityReason::SourceUnavailable {
                    source: report.source.clone(),
                    reason,
                });
            }
        }
    }
    reasons
}

/// Clears one-shot notice suppression when a source becomes available again.
pub(crate) fn clear_notices_for_restored_sources(
    store: &mut InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    reports: &[ReportedSourceAvailability],
) {
    let restored: HashSet<String> = reports
        .iter()
        .filter(|report| report.state.is_available())
        .map(|report| report.source.clone())
        .collect();
    if restored.is_empty() {
        return;
    }
    store.authentication_needed_notices.retain(|notice| {
        !(notice.user_id == user_id
            && notice.tenant_id == tenant_id
            && restored.contains(&notice.source))
    });
}

/// Evaluates authentication-needed notice emission for reports on one task.
///
/// Scheduled runs never emit authentication-needed notices. On-demand runs emit
/// at most once per unavailable source state fingerprint until availability changes.
pub(crate) fn evaluate_authentication_notices(
    store: &mut InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    task_id: DiscoveryTaskId,
    scheduled: bool,
    reports: &[ReportedSourceAvailability],
    now: DateTime<Utc>,
) -> Vec<AuthenticationNeededNoticeOutcome> {
    clear_notices_for_restored_sources(store, user_id, tenant_id, reports);
    let mut outcomes = Vec::new();
    for report in reports {
        if !report.authentication_required() {
            if !report.state.is_available() {
                outcomes.push(AuthenticationNeededNoticeOutcome::NotApplicable {
                    source: report.source.clone(),
                });
            }
            continue;
        }
        if scheduled {
            outcomes.push(AuthenticationNeededNoticeOutcome::ScheduledSkip {
                source: report.source.clone(),
            });
            continue;
        }
        let fingerprint = report.state_fingerprint();
        if let Some(existing) = store.authentication_needed_notices.iter().find(|notice| {
            notice.user_id == user_id
                && notice.tenant_id == tenant_id
                && notice.source == report.source
                && notice.state_fingerprint == fingerprint
        }) {
            outcomes.push(AuthenticationNeededNoticeOutcome::Suppressed {
                notice: existing.clone(),
            });
            continue;
        }
        // Different fingerprint for the same source replaces prior suppression.
        store.authentication_needed_notices.retain(|notice| {
            !(notice.user_id == user_id
                && notice.tenant_id == tenant_id
                && notice.source == report.source)
        });
        let notice = AuthenticationNeededNotice {
            id: Uuid::now_v7(),
            user_id,
            tenant_id,
            source: report.source.clone(),
            state_fingerprint: fingerprint,
            task_id,
            first_emitted_at: now,
            delivery_pending: true,
        };
        store.authentication_needed_notices.push(notice.clone());
        outcomes.push(AuthenticationNeededNoticeOutcome::ShouldNotify { notice });
    }
    outcomes
}

/// Identity fields for a lease-scoped availability upsert.
pub(crate) struct TaskAvailabilityIdentity {
    pub(crate) task_id: DiscoveryTaskId,
    pub(crate) user_id: UserId,
    pub(crate) tenant_id: Option<TenantId>,
    pub(crate) reported_by: AgentHarnessId,
}

/// Upserts the lease-scoped private availability snapshot for a task.
pub(crate) fn upsert_task_source_availability(
    store: &mut InMemoryStore,
    identity: TaskAvailabilityIdentity,
    reports: Vec<ReportedSourceAvailability>,
    browser_grant_eligible_sources: Option<Vec<String>>,
    now: DateTime<Utc>,
) -> DiscoveryTaskSourceAvailability {
    let prior = store
        .discovery_task_source_availability
        .get(&identity.task_id);
    let merged_reports = merge_reports(
        prior.map(|entry| entry.reports.as_slice()).unwrap_or(&[]),
        reports,
    );
    let eligible = browser_grant_eligible_sources
        .or_else(|| prior.and_then(|entry| entry.browser_grant_eligible_sources.clone()));
    let reports = enforce_browser_grant_on_reports(merged_reports, eligible.as_deref());
    let availability = DiscoveryTaskSourceAvailability {
        task_id: identity.task_id,
        user_id: identity.user_id,
        tenant_id: identity.tenant_id,
        reported_by: identity.reported_by,
        reports,
        browser_grant_eligible_sources: eligible,
        updated_at: now,
    };
    store
        .discovery_task_source_availability
        .insert(identity.task_id, availability.clone());
    record_watch_availability(
        store,
        identity.user_id,
        identity.tenant_id,
        &availability.reports,
    );
    availability
}

/// Persists the latest availability fact on matching User watches.
///
/// Reports are matched by watch URL domain. Facts only: never auth material.
pub(crate) fn record_watch_availability(
    store: &mut InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    reports: &[ReportedSourceAvailability],
) {
    for watch in store.user_watches.values_mut() {
        if watch.user_id != user_id || watch.tenant_id != tenant_id {
            continue;
        }
        let Some(domain) = url::Url::parse(&watch.url)
            .ok()
            .and_then(|url| url.domain().map(str::to_lowercase))
        else {
            continue;
        };
        if let Some(report) = reports
            .iter()
            .find(|report| report.source.eq_ignore_ascii_case(&domain))
        {
            watch.last_availability = Some(report.clone());
        }
    }
}

/// Resolves the final report set used when completing a batch.
pub(crate) fn resolve_completion_reports(
    store: &InMemoryStore,
    task_id: DiscoveryTaskId,
    request_reports: Vec<ReportedSourceAvailability>,
    request_eligible: Option<Vec<String>>,
) -> Result<(Vec<ReportedSourceAvailability>, Option<Vec<String>>), String> {
    let request_reports = normalize_reports(request_reports)?;
    let request_eligible = normalize_browser_grant_eligibility(request_eligible)?;
    let prior = store.discovery_task_source_availability.get(&task_id);
    let eligible = request_eligible
        .or_else(|| prior.and_then(|entry| entry.browser_grant_eligible_sources.clone()));
    let merged = merge_reports(
        prior.map(|entry| entry.reports.as_slice()).unwrap_or(&[]),
        request_reports,
    );
    let reports = enforce_browser_grant_on_reports(merged, eligible.as_deref());
    Ok((reports, eligible))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_credential_bearing_availability_text() {
        let err = normalize_reports(vec![ReportedSourceAvailability {
            source: "x.com".into(),
            state: SourceAvailabilityState::AuthenticationRequired,
            reason: "cookie: session=abc".into(),
        }])
        .unwrap_err();
        assert!(err.contains("authentication material"));
    }

    #[test]
    fn eligibility_never_marks_out_of_grant_source_available() {
        let reports = enforce_browser_grant_on_reports(
            vec![ReportedSourceAvailability {
                source: "secret.example".into(),
                state: SourceAvailabilityState::Available,
                reason: String::new(),
            }],
            Some(&["open.example".into()]),
        );
        assert_eq!(
            reports[0].state,
            SourceAvailabilityState::BrowserGrantIneligible
        );
    }
}
