use crate::domain::{
    CreatePodRequest, ExportedSkillPack, Pod, PodSkillPack, SkillPackPatch, ValidationReport,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

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
        pod_yaml,
        skill_md,
        sources_yaml: "sources: []\n".to_string(),
        filters_yaml: "blocked_topics: []\nblocked_domains: []\ndownrank:\n  - generic AI hype\n  - VC announcement\n  - product launch without artifact\nauto_promote_crawler_candidates: false\n".to_string(),
        examples_good_md: "# Good examples\n\n- Links with artifacts, demos, clear notes, or durable ideas.\n".to_string(),
        examples_bad_md: "# Bad examples\n\n- Engagement bait, politics, generic hype, or thin launches.\n".to_string(),
        created_at: now,
        updated_at: now,
    }
}

pub fn validate_skill_pack(pack: &PodSkillPack) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if pack.pod_yaml.trim().is_empty() {
        errors.push("pod.yaml is empty".to_string());
    } else if serde_yaml::from_str::<serde_yaml::Value>(&pack.pod_yaml).is_err() {
        errors.push("pod.yaml is not valid YAML".to_string());
    }
    if pack.sources_yaml.trim().is_empty() {
        warnings.push("sources.yaml is empty; crawler will have no approved sources".to_string());
    } else if serde_yaml::from_str::<serde_yaml::Value>(&pack.sources_yaml).is_err() {
        errors.push("sources.yaml is not valid YAML".to_string());
    }
    if pack.filters_yaml.trim().is_empty() {
        warnings.push("filters.yaml is empty; pod-level filters will be weak".to_string());
    } else if serde_yaml::from_str::<serde_yaml::Value>(&pack.filters_yaml).is_err() {
        errors.push("filters.yaml is not valid YAML".to_string());
    }
    if !pack.skill_md.contains("#") {
        warnings.push("SKILL.md has no headings".to_string());
    }
    ValidationReport {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

pub fn export_skill_pack(pack: &PodSkillPack, events_jsonl: String) -> ExportedSkillPack {
    let mut files = BTreeMap::new();
    files.insert("pod.yaml".to_string(), pack.pod_yaml.clone());
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
    if let Some(value) = files.get("pod.yaml") {
        pack.pod_yaml = value.clone();
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

pub fn fork_skill_pack(source: &PodSkillPack, target_pod: &Pod) -> PodSkillPack {
    let now = Utc::now();
    let mut pack = source.clone();
    pack.id = Uuid::now_v7();
    pack.pod_id = target_pod.id;
    pack.version = 1;
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
