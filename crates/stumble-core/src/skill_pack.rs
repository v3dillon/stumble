use crate::domain::{
    CreatePodRequest, ExportedSkillPack, Pod, PodPackageContents, PodSkillPack, SkillPackPatch,
    ValidationReport,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Typed validation failure for portable Pod Package contents.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PodPackageValidationError {
    /// A required portable component has no content.
    #[error("{component} is empty")]
    EmptyComponent { component: &'static str },
    /// Pod Context contains operational instruction language.
    #[error("CONTEXT.md must describe subject scope and boundaries, not agent instructions")]
    ContextContainsInstructions,
    /// The Source Rule document is absent or malformed.
    #[error("sources.yaml must contain declarative non-executable Source Rules without credentials: {reason}")]
    InvalidSourceRules { reason: String },
    /// The filter document is malformed.
    #[error("filters.yaml is not valid YAML: {reason}")]
    InvalidFilters { reason: String },
}

/// Collection of typed Pod Package validation failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))]
pub struct PodPackageValidationErrors(Vec<PodPackageValidationError>);

impl PodPackageValidationErrors {
    /// Returns every typed validation failure.
    #[must_use]
    pub fn errors(&self) -> &[PodPackageValidationError] {
        &self.0
    }
}

pub fn default_skill_pack(pod: &Pod) -> PodSkillPack {
    let pod_yaml = format!(
        "name: {}\nslug: {}\ndescription: {}\nvisibility: {:?}\n\ndefault_modes:\n  - deep_match\n  - adjacent\n  - old_gem\n  - human_pick\n  - rabbit_hole\n\npositive_signals:\n  - working demo\n  - visual artifact\n  - implementation detail\n  - independent research\n  - unusual interaction pattern\n\nnegative_signals:\n  - politics\n  - generic AI hype\n  - VC announcement\n  - no artifact\n  - engagement bait\n\nbrief_style:\n  tone: concise\n  max_items: 7\n  include_why_it_matters: true\n  include_user_fit: true\n",
        pod.name, pod.slug, pod.description, pod.visibility
    );
    let skill_md = format!(
        "---\nname: {}-pod\ndescription: Use when discovering, submitting, summarizing, or curating links for the {} pod.\n---\n\n# {} Pod\n\n## Purpose\n\n{}\n\n## Prefer\n\n- Interactive demos\n- Independent research\n- Clear artifacts\n- Specific implementation details\n- Weird but practical experiments\n- Older links that still feel alive\n\n## Avoid\n\n- Political discourse\n- VC announcements\n- Engagement bait\n- Generic AI hype\n- Product launches with no artifact\n\n## Submission Guidance\n\nInclude why the link belongs, what idea it unlocks, and whether it is practical, speculative, aesthetic, or technical.\n",
        pod.slug, pod.name, pod.name, pod.description
    );
    let now = Utc::now();
    PodSkillPack {
        id: Uuid::now_v7(),
        pod_id: pod.id,
        version: 1,
        context_md: format!(
            "# {}\n\n## Scope\n\n{}\n\n## Boundaries\n\nThis context defines the Pod subject; curation instructions belong in SKILL.md.\n",
            pod.name, pod.description
        ),
        pod_yaml,
        skill_md,
        sources_yaml: "source_rules:\n  - inspect:\n      kind: publication\n      name: sources relevant to this Pod subject\n    seek:\n      description: durable material within the Pod scope\n    schedule:\n      cadence: on_demand\n".to_string(),
        filters_yaml: "blocked_topics: []\nblocked_domains: []\ndownrank:\n  - generic AI hype\n  - VC announcement\n  - product launch without artifact\nauto_promote_crawler_candidates: false\n".to_string(),
        examples_good_md: "# Good examples\n\n- Links with artifacts, demos, clear notes, or durable ideas.\n".to_string(),
        examples_bad_md: "# Bad examples\n\n- Engagement bait, politics, generic hype, or thin launches.\n".to_string(),
        owner_id: pod.created_by,
        proposer_harness_id: None,
        created_at: now,
        updated_at: now,
    }
}

pub fn validate_skill_pack(pack: &PodSkillPack) -> ValidationReport {
    validate_pod_package_contents(&PodPackageContents {
        context_md: pack.context_md.clone(),
        skill_md: pack.skill_md.clone(),
        sources_yaml: pack.sources_yaml.clone(),
        filters_yaml: pack.filters_yaml.clone(),
        examples_good_md: pack.examples_good_md.clone(),
        examples_bad_md: pack.examples_bad_md.clone(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRulesDocument {
    source_rules: Vec<SourceRuleSuggestion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRuleSuggestion {
    inspect: InspectionKind,
    seek: DiscoveryObjective,
    schedule: ScheduleSuggestion,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum InspectionKind {
    Publication { name: CredentialFreeText },
    Website { url: CredentialFreeUrl },
    Domain { domain: DomainName },
    SearchTopic { topic: CredentialFreeText },
}

impl InspectionKind {
    fn is_empty(&self) -> bool {
        match self {
            Self::Publication { name } => name.0.trim().is_empty(),
            Self::Website { url } => url.0.as_str().is_empty(),
            Self::Domain { domain } => domain.0.is_empty(),
            Self::SearchTopic { topic } => topic.0.trim().is_empty(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryObjective {
    description: CredentialFreeText,
}

#[derive(Debug)]
struct CredentialFreeText(String);

#[derive(Debug)]
struct CredentialFreeUrl(url::Url);

#[derive(Debug)]
struct DomainName(String);

impl<'de> Deserialize<'de> for CredentialFreeText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_credential_free_text(&value).map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

fn validate_credential_free_text(value: &str) -> Result<(), &'static str> {
    let normalized = value.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(relative_start) = next_http_scheme(&normalized[cursor..]) {
        let start = cursor + relative_start;
        let candidate = &value[start..];
        let end = candidate
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(
                        character,
                        ')' | ']' | '}' | ',' | ';' | '<' | '>' | '"' | '\''
                    )
            })
            .unwrap_or(candidate.len());
        let url = url::Url::parse(&candidate[..end]).map_err(|_| "invalid embedded URL")?;
        validate_credential_free_url(&url)?;
        cursor = start + end;
    }
    if has_credential_assignment(&normalized) {
        return Err("embedded credentials are not allowed");
    }
    Ok(())
}

fn next_http_scheme(value: &str) -> Option<usize> {
    [value.find("http://"), value.find("https://")]
        .into_iter()
        .flatten()
        .min()
}

impl<'de> Deserialize<'de> for CredentialFreeUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let url = url::Url::parse(&value).map_err(serde::de::Error::custom)?;
        validate_credential_free_url(&url).map_err(serde::de::Error::custom)?;
        Ok(Self(url))
    }
}

fn validate_credential_free_url(url: &url::Url) -> Result<(), &'static str> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("website URL must use HTTP or HTTPS");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL user information is not allowed");
    }
    if url.query().is_some() {
        return Err("URL query parameters are not allowed");
    }
    if url.fragment().is_some() {
        return Err("URL fragments are not allowed");
    }
    Ok(())
}

fn has_credential_assignment(value: &str) -> bool {
    CREDENTIAL_NAMES
        .into_iter()
        .any(|name| value.contains(&format!("{name}=")) || value.contains(&format!("{name}:")))
        || value.contains("authorization:")
}

impl<'de> Deserialize<'de> for DomainName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let parsed =
            url::Url::parse(&format!("https://{value}")).map_err(serde::de::Error::custom)?;
        let labels_are_valid = value.contains('.')
            && value.len() <= 253
            && value.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
            });
        let is_domain = labels_are_valid
            && matches!(parsed.host(), Some(url::Host::Domain(domain)) if domain == value)
            && parsed.port().is_none()
            && parsed.path() == "/"
            && parsed.query().is_none()
            && parsed.fragment().is_none();
        if !is_domain {
            return Err(serde::de::Error::custom(
                "invalid credential-free domain name",
            ));
        }
        Ok(Self(value))
    }
}

const CREDENTIAL_NAMES: [&str; 11] = [
    "api_key",
    "apikey",
    "access_token",
    "refresh_token",
    "id_token",
    "auth_token",
    "bearer_token",
    "token",
    "client_secret",
    "secret",
    "password",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleSuggestion {
    cadence: SourceRuleCadence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceRuleCadence {
    OnDemand,
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

impl SourceRuleCadence {
    pub(crate) fn period_start(
        self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> chrono::DateTime<chrono::Utc> {
        use chrono::{Datelike, Timelike};
        match self {
            Self::OnDemand => now,
            Self::Hourly => now
                .with_minute(0)
                .and_then(|value| value.with_second(0))
                .and_then(|value| value.with_nanosecond(0))
                .expect("BUG: zero is a valid time component"),
            Self::Daily => now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .expect("BUG: midnight is valid")
                .and_utc(),
            Self::Weekly => {
                let monday = now.date_naive()
                    - chrono::Duration::days(i64::from(now.weekday().num_days_from_monday()));
                monday
                    .and_hms_opt(0, 0, 0)
                    .expect("BUG: midnight is valid")
                    .and_utc()
            }
            Self::Monthly => chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .expect("BUG: first day of a valid month is valid")
                .and_utc(),
        }
    }
}

/// Returns the validated cadence of each Source Rule in package order.
pub(crate) fn source_rule_cadences(
    sources_yaml: &str,
) -> Result<Vec<SourceRuleCadence>, PodPackageValidationError> {
    let document = serde_yaml::from_str::<SourceRulesDocument>(sources_yaml).map_err(|error| {
        PodPackageValidationError::InvalidSourceRules {
            reason: error.to_string(),
        }
    })?;
    Ok(document
        .source_rules
        .into_iter()
        .map(|rule| rule.schedule.cadence)
        .collect())
}

/// Validates all required portable package components and their trust boundary.
#[must_use]
pub fn validate_pod_package_contents(contents: &PodPackageContents) -> ValidationReport {
    let errors = pod_package_validation_errors(contents);
    let mut warnings = Vec::new();
    if !contents.skill_md.trim().is_empty() && !contents.skill_md.contains('#') {
        warnings.push("SKILL.md has no headings".to_string());
    }
    ValidationReport {
        valid: errors.is_empty(),
        errors: errors.iter().map(ToString::to_string).collect(),
        warnings,
    }
}

fn pod_package_validation_errors(contents: &PodPackageContents) -> Vec<PodPackageValidationError> {
    let mut errors = Vec::new();
    for (component, value) in [
        ("CONTEXT.md", contents.context_md.as_str()),
        ("SKILL.md", contents.skill_md.as_str()),
        ("sources.yaml", contents.sources_yaml.as_str()),
        ("filters.yaml", contents.filters_yaml.as_str()),
        ("examples.good.md", contents.examples_good_md.as_str()),
        ("examples.bad.md", contents.examples_bad_md.as_str()),
    ] {
        if value.trim().is_empty() {
            errors.push(PodPackageValidationError::EmptyComponent { component });
        }
    }
    let context = contents.context_md.to_lowercase();
    if context.contains("## instructions")
        || context.contains("ignore harness")
        || context.contains("you must")
    {
        errors.push(PodPackageValidationError::ContextContainsInstructions);
    }
    if !contents.sources_yaml.trim().is_empty() {
        match serde_yaml::from_str::<SourceRulesDocument>(&contents.sources_yaml) {
            Ok(document) if document.source_rules.is_empty() => {
                errors.push(PodPackageValidationError::InvalidSourceRules {
                    reason: "at least one Source Rule is required".to_string(),
                })
            }
            Ok(document) => {
                for rule in document.source_rules {
                    if rule.inspect.is_empty() || rule.seek.description.0.trim().is_empty() {
                        errors.push(PodPackageValidationError::InvalidSourceRules {
                            reason: "each rule requires inspect, seek, and schedule".to_string(),
                        });
                    }
                    let _ = &rule.schedule.cadence;
                }
            }
            Err(error) => errors.push(PodPackageValidationError::InvalidSourceRules {
                reason: error.to_string(),
            }),
        }
    }
    if !contents.filters_yaml.trim().is_empty() {
        if let Err(error) = serde_yaml::from_str::<serde_yaml::Value>(&contents.filters_yaml) {
            errors.push(PodPackageValidationError::InvalidFilters {
                reason: error.to_string(),
            });
        }
    }
    errors
}

/// Returns typed failures for invalid package contents.
///
/// # Errors
///
/// Returns every structural or trust-boundary validation failure.
pub fn validate_pod_package_contents_typed(
    contents: &PodPackageContents,
) -> Result<(), PodPackageValidationErrors> {
    let errors = pod_package_validation_errors(contents);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(PodPackageValidationErrors(errors))
    }
}

impl TryFrom<crate::domain::RawPodPackageContents> for PodPackageContents {
    type Error = PodPackageValidationErrors;

    fn try_from(raw: crate::domain::RawPodPackageContents) -> Result<Self, Self::Error> {
        let contents = Self {
            context_md: raw.context_md,
            skill_md: raw.skill_md,
            sources_yaml: raw.sources_yaml,
            filters_yaml: raw.filters_yaml,
            examples_good_md: raw.examples_good_md,
            examples_bad_md: raw.examples_bad_md,
        };
        validate_pod_package_contents_typed(&contents)?;
        Ok(contents)
    }
}

pub fn export_skill_pack(pack: &PodSkillPack, events_jsonl: String) -> ExportedSkillPack {
    let mut files = BTreeMap::new();
    files.insert("CONTEXT.md".to_string(), pack.context_md.clone());
    files.insert("SKILL.md".to_string(), pack.skill_md.clone());
    files.insert("sources.yaml".to_string(), pack.sources_yaml.clone());
    files.insert("filters.yaml".to_string(), pack.filters_yaml.clone());
    files.insert(
        "examples.good.md".to_string(),
        pack.examples_good_md.clone(),
    );
    files.insert("examples.bad.md".to_string(), pack.examples_bad_md.clone());
    files.insert("events.jsonl".to_string(), events_jsonl);
    ExportedSkillPack { files }
}

pub fn import_skill_pack(
    existing: &PodSkillPack,
    files: &BTreeMap<String, String>,
) -> PodSkillPack {
    let mut pack = existing.clone();
    if let Some(value) = files.get("CONTEXT.md") {
        pack.context_md = value.clone();
    }
    if let Some(value) = files.get("SKILL.md") {
        pack.skill_md = value.clone();
    }
    if let Some(value) = files.get("sources.yaml") {
        pack.sources_yaml = value.clone();
    }
    if let Some(value) = files.get("filters.yaml") {
        pack.filters_yaml = value.clone();
    }
    if let Some(value) = files.get("examples.good.md") {
        pack.examples_good_md = value.clone();
    }
    if let Some(value) = files.get("examples.bad.md") {
        pack.examples_bad_md = value.clone();
    }
    pack.version += 1;
    pack.updated_at = Utc::now();
    pack
}

pub fn patch_skill_pack(existing: &PodSkillPack, patch: SkillPackPatch) -> PodSkillPack {
    let mut pack = existing.clone();
    if let Some(value) = patch.context_md {
        pack.context_md = value;
    }
    if let Some(value) = patch.pod_yaml {
        pack.pod_yaml = value;
    }
    if let Some(value) = patch.skill_md {
        pack.skill_md = value;
    }
    if let Some(value) = patch.sources_yaml {
        pack.sources_yaml = value;
    }
    if let Some(value) = patch.filters_yaml {
        pack.filters_yaml = value;
    }
    if let Some(value) = patch.examples_good_md {
        pack.examples_good_md = value;
    }
    if let Some(value) = patch.examples_bad_md {
        pack.examples_bad_md = value;
    }
    pack.version += 1;
    pack.updated_at = Utc::now();
    pack
}

/// Exact portable Pod Package directory entries, including signed history.
pub const PORTABLE_PACKAGE_FILES: [&str; 7] = [
    "CONTEXT.md",
    "SKILL.md",
    "sources.yaml",
    "filters.yaml",
    "examples.good.md",
    "examples.bad.md",
    "events.jsonl",
];

/// Rejects incomplete directories and any file that could represent local authority.
///
/// # Errors
///
/// Returns an error for a missing required file or any unsupported extra file.
pub fn validate_portable_package_files(
    files: &BTreeMap<String, String>,
) -> Result<(), crate::store::StoreError> {
    for required in PORTABLE_PACKAGE_FILES {
        if !files.contains_key(required) {
            return Err(crate::store::StoreError::Validation(format!(
                "portable Pod Package is missing {required}"
            )));
        }
    }
    for name in files.keys() {
        if !PORTABLE_PACKAGE_FILES.contains(&name.as_str()) {
            let message = if name.to_lowercase().contains("grant")
                || name.to_lowercase().contains("credential")
                || name.to_lowercase().contains("permission")
            {
                format!("portable Pod Package cannot contain node-local authority file {name}")
            } else {
                format!("portable Pod Package contains unsupported file {name}")
            };
            return Err(crate::store::StoreError::Validation(message));
        }
    }
    Ok(())
}

/// Parses an allowlisted portable directory map into complete package contents.
///
/// # Errors
///
/// Returns an error if any required file is absent or an unsupported file is present.
pub fn pod_package_contents_from_files(
    files: &BTreeMap<String, String>,
) -> Result<PodPackageContents, crate::store::StoreError> {
    validate_portable_package_files(files)?;
    let required = |name: &str| {
        files.get(name).cloned().ok_or_else(|| {
            crate::store::StoreError::Validation(format!("portable Pod Package is missing {name}"))
        })
    };
    Ok(PodPackageContents {
        context_md: required("CONTEXT.md")?,
        skill_md: required("SKILL.md")?,
        sources_yaml: required("sources.yaml")?,
        filters_yaml: required("filters.yaml")?,
        examples_good_md: required("examples.good.md")?,
        examples_bad_md: required("examples.bad.md")?,
    })
}

pub fn fork_skill_pack(source: &PodSkillPack, target_pod: &Pod) -> PodSkillPack {
    let now = Utc::now();
    let mut pack = source.clone();
    pack.id = Uuid::now_v7();
    pack.pod_id = target_pod.id;
    pack.version = 1;
    pack.owner_id = target_pod.created_by;
    pack.proposer_harness_id = None;
    pack.created_at = now;
    pack.updated_at = now;
    pack.pod_yaml = pack.pod_yaml.replace(
        "slug:",
        &format!("# forked_from_skill_pack: {}\nslug:", source.id),
    );
    pack
}

pub fn pod_request_from_template(name: &str, slug: &str) -> CreatePodRequest {
    CreatePodRequest {
        name: name.to_string(),
        slug: slug.to_string(),
        description: format!("Shared discovery pod for {name}."),
        visibility: crate::domain::Visibility::Public,
    }
}

pub fn skill_pack_payload(pack: &PodSkillPack) -> serde_json::Value {
    json!({
        "id": pack.id,
        "pod_id": pack.pod_id,
        "version": pack.version,
        "updated_at": pack.updated_at,
    })
}

pub fn extract_yaml_list(yaml: &str, key: &str) -> Vec<String> {
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml) else {
        return Vec::new();
    };
    value
        .get(key)
        .and_then(|v| v.as_sequence())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_lowercase()))
                .collect()
        })
        .unwrap_or_default()
}
