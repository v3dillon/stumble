use crate::agent_tools::AgentTools;
use crate::domain::*;
use crate::signing::{create_node_identity, hash_api_token, new_plaintext_api_token};
#[cfg(test)]
use crate::skill_pack::{
    default_skill_pack, pod_package_contents_from_files, pod_request_from_template,
};
use crate::store::InMemoryStore;
#[cfg(test)]
use chrono::Duration;
use chrono::Utc;
use uuid::Uuid;

/// Minimal Home Node seed: local identity, one owner User, and default
/// bootstrap / discovery-peer configuration. Used by `node init` by default.
pub fn empty_home_node_store() -> InMemoryStore {
    let mut store = InMemoryStore::default();
    // Sponsored Bootstrap is ordinary removable Home Node config, not protocol authority.
    crate::bootstrap::ensure_default_bootstrap_endpoint(&mut store, Utc::now());
    // Automatic Discovery Peer gossip is enabled by default (outbound only).
    crate::discovery_peer::ensure_discovery_peer_gossip_config(&mut store);
    let local_node = create_node_identity("local stumble node", None);
    store
        .node_identities
        .insert(local_node.id, local_node.clone());

    let user = User {
        id: Uuid::now_v7(),
        display_name: "Local Owner".to_string(),
        created_at: Utc::now(),
    };
    store.user_preferences.insert(
        (user.id, None),
        UserPreferences {
            user_id: user.id,
            tenant_id: None,
            interests: vec![],
            blocked_topics: vec![],
            blocked_sources: vec![],
            blocked_source_affinities: vec![],
            preferred_brief_length: 7,
            preferred_discovery_mode: DiscoveryMode::DeepMatch,
            recurrence_penalty_days: RecurrencePenaltyDays::default(),
        },
    );
    store.users.insert(user.id, user);
    store
}

/// Demo seed data for tests and `node init --demo`.
pub fn seed_store() -> InMemoryStore {
    let mut store = empty_home_node_store();
    // Replace the minimal owner with richer multi-user demo fixtures.
    store.users.clear();
    store.user_preferences.clear();

    let hosted_tenant = Tenant {
        id: Uuid::now_v7(),
        name: "Default Hosted Tenant".to_string(),
        slug: "default-hosted".to_string(),
        created_at: Utc::now(),
    };
    store
        .tenants
        .insert(hosted_tenant.id, hosted_tenant.clone());
    let hosted_node = create_node_identity("default hosted managed node", Some(hosted_tenant.id));
    store
        .node_identities
        .insert(hosted_node.id, hosted_node.clone());

    for idx in 1..=3 {
        let user = User {
            id: Uuid::now_v7(),
            display_name: format!("Seed User {idx}"),
            created_at: Utc::now(),
        };
        store.tenant_users.push(TenantUser {
            tenant_id: hosted_tenant.id,
            user_id: user.id,
            role: if idx == 1 {
                TenantRole::Owner
            } else {
                TenantRole::Member
            },
            created_at: Utc::now(),
        });
        store.user_preferences.insert(
            (user.id, None),
            UserPreferences {
                user_id: user.id,
                tenant_id: None,
                interests: vec![
                    "interfaces".to_string(),
                    "tools".to_string(),
                    "agents".to_string(),
                ],
                blocked_topics: vec!["politics".to_string()],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            },
        );
        store.users.insert(user.id, user);
    }

    let peer_one = create_node_identity("trusted design lab", None);
    let peer_two = create_node_identity("hosted relay example", None);
    let peer_one_id = Uuid::now_v7();
    store.trusted_peers.insert(
        peer_one_id,
        TrustedPeer {
            id: peer_one_id,
            node_id: peer_one.id,
            tenant_id: None,
            display_name: "Trusted Design Lab".to_string(),
            base_url: "https://design-lab.example".to_string(),
            public_key: peer_one.public_key,
            trust_level: TrustLevel::ReadWrite,
            enabled: true,
            created_at: Utc::now(),
        },
    );
    let peer_two_id = Uuid::now_v7();
    store.trusted_peers.insert(
        peer_two_id,
        TrustedPeer {
            id: peer_two_id,
            node_id: peer_two.id,
            tenant_id: Some(hosted_tenant.id),
            display_name: "Hosted Relay Example".to_string(),
            base_url: "https://relay.example".to_string(),
            public_key: peer_two.public_key,
            trust_level: TrustLevel::ReadOnly,
            enabled: true,
            created_at: Utc::now(),
        },
    );

    let user_ids: Vec<_> = store.users.keys().copied().collect();

    let token = new_plaintext_api_token();
    let token_hash = hash_api_token(&token);
    if let Some(user_id) = user_ids.first().copied() {
        let api_token_id = Uuid::now_v7();
        store.api_tokens.insert(
            api_token_id,
            ApiToken {
                id: api_token_id,
                user_id,
                tenant_id: None,
                token_hash,
                label: "seed-dev-token".to_string(),
                created_at: Utc::now(),
                last_used_at: None,
                revoked_at: None,
                harness_id: None,
            },
        );
    }

    store
}

pub fn seed_agent_tools() -> AgentTools {
    AgentTools::new(seed_store())
}

#[cfg(test)]
fn insert_seed_pod(
    store: &mut InMemoryStore,
    node: &NodeIdentity,
    name: &str,
    slug: &str,
    description: &str,
) {
    let request = pod_request_from_template(name, slug);
    let pod = Pod {
        id: Uuid::now_v7(),
        tenant_id: None,
        name: request.name,
        slug: request.slug,
        description: description.to_string(),
        visibility: Visibility::Public,
        created_by: None,
        created_at: Utc::now(),
        origin_node_id: Some(node.id),
    };
    store.pod_rules.insert(
        pod.id,
        PodRules {
            pod_id: pod.id,
            blocked_topics: vec!["politics".to_string()],
            blocked_domains: vec![],
            auto_promote_crawler_candidates: false,
            federate_sources: true,
        },
    );
    let package = default_skill_pack(&pod);
    let _ = store.insert_pod_package_version(package.clone());
    store.pod_skill_packs.insert(pod.id, package.clone());
    if let Ok(event) = crate::signing::sign_public_event(
        node,
        "pod_created",
        &pod.slug,
        serde_json::json!({"pod": pod.clone(), "package": package}),
        store.latest_event_hash(&pod.slug),
    ) {
        store.event_log.push(event);
    }
    store.pods.insert(pod.id, pod);
}

#[cfg(test)]
mod tests;
