use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Local,
    Hosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    InviteOnly,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantRole {
    Owner,
    Admin,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodRole {
    Owner,
    Curator,
}

/// Pod workflow actions allowed by current relationship, capability, and scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodAllowedAction {
    VisibilitySet,
    Subscribe,
    Unsubscribe,
    SubscriptionSet,
    RoleList,
    RoleGrant,
    RoleRevoke,
}

/// Package material selected for a new Pod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PodCreationPackage {
    Default,
    Initial {
        package: PodPackageContents,
    },
    /// An immutable source package snapshot retained with its identity.
    Derived {
        source_package: PodSkillPack,
    },
}

/// Complete request for atomically creating a Pod and its first package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePodLifecycleRequest {
    pub pod: CreatePodRequest,
    pub package: PodCreationPackage,
}

/// Outcome of a visibility transition under the sensitive-change policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum PodVisibilityOutcome {
    Updated(Pod),
    PendingApproval(Box<PendingProposal>),
}

/// Outcome of a Trust Policy change under the sensitive-change policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum TrustPolicyChangeOutcome {
    Applied(Box<TrustPolicy>),
    PendingApproval(Box<PendingProposal>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    ReadOnly,
    ReadWrite,
}

/// One public Pod identity excluded by a User's local Trust Policy.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BlockedPod {
    /// Origin whose Pod is excluded.
    pub origin_node_id: NodeIdentityId,
    /// Origin-local public Pod slug.
    pub pod_slug: String,
}

impl BlockedPod {
    /// Creates one local public Pod exclusion.
    #[must_use]
    pub fn new(origin_node_id: NodeIdentityId, pod_slug: impl Into<String>) -> Self {
        Self {
            origin_node_id,
            pod_slug: pod_slug.into(),
        }
    }
}

/// Replaceable optional Index Node selected by a User.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct IndexNode {
    /// Human-readable local label.
    pub label: String,
    /// Base address used for outbound announcement search.
    pub base_url: String,
}

/// User-controlled local rules governing public Pod discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TrustPolicy {
    /// User whose discovery behavior this policy controls.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Optional and replaceable announcement indexes.
    pub index_nodes: Vec<IndexNode>,
    /// Public Pods excluded from Explore.
    pub blocked_pods: std::collections::BTreeSet<BlockedPod>,
    /// Origin Nodes excluded from Explore.
    pub blocked_nodes: std::collections::BTreeSet<NodeIdentityId>,
    /// Content sources excluded from Explore samples.
    pub blocked_sources: std::collections::BTreeSet<String>,
    /// Topics excluded from Pod subjects and Explore samples.
    pub blocked_topics: std::collections::BTreeSet<String>,
}

impl TrustPolicy {
    /// Creates an empty local Trust Policy for one User.
    #[must_use]
    pub const fn new(user_id: UserId, tenant_id: Option<TenantId>) -> Self {
        Self {
            user_id,
            tenant_id,
            index_nodes: Vec::new(),
            blocked_pods: std::collections::BTreeSet::new(),
            blocked_nodes: std::collections::BTreeSet::new(),
            blocked_sources: std::collections::BTreeSet::new(),
            blocked_topics: std::collections::BTreeSet::new(),
        }
    }

    /// Whether announcements received only from this Index base URL remain eligible.
    #[must_use]
    pub fn retains_index_url(&self, source: &str) -> bool {
        self.index_nodes
            .iter()
            .any(|index| index.base_url == source)
    }

    /// Whether a public Pod Announcement is excluded by node, pod, or topic blocks.
    ///
    /// Topic matching lowercases announcement text and checks `contains` against the
    /// stored blocked topic string (Explore semantics — policy topics are not re-cased).
    #[must_use]
    pub fn blocks_announcement(&self, announcement: &PodAnnouncement) -> bool {
        self.blocked_nodes.contains(&announcement.origin_node_id)
            || self.blocked_pods.iter().any(|blocked| {
                blocked.origin_node_id == announcement.origin_node_id
                    && blocked
                        .pod_slug
                        .eq_ignore_ascii_case(&announcement.pod_slug)
            })
            || self.blocked_topics.iter().any(|topic| {
                announcement.subject.to_lowercase().contains(topic)
                    || announcement.pod_name.to_lowercase().contains(topic)
                    || announcement.pod_slug.to_lowercase().contains(topic)
            })
    }

    /// Whether a public Pod is excluded by node or pod blocks.
    #[must_use]
    pub fn blocks_pod(&self, origin_node_id: NodeIdentityId, pod_slug: &str) -> bool {
        self.blocked_nodes.contains(&origin_node_id)
            || self.blocked_pods.iter().any(|blocked| {
                blocked.origin_node_id == origin_node_id
                    && blocked.pod_slug.eq_ignore_ascii_case(pod_slug)
            })
    }

    /// Whether a Content Reference sample is excluded by source or topic blocks.
    ///
    /// Topic matching lowercases title/summary and checks `contains` against the
    /// stored blocked topic string (Explore semantics — policy topics are not re-cased).
    #[must_use]
    pub fn blocks_content_reference(&self, reference: &FeedContentReference) -> bool {
        self.blocks_source_and_topics(
            &reference.source,
            &reference.tags,
            &reference.title,
            reference.summary.as_deref(),
        )
    }

    /// Whether a source domain and topic-bearing fields are excluded by Trust Policy.
    #[must_use]
    pub fn blocks_source_and_topics(
        &self,
        source: &str,
        tags: &[String],
        title: &str,
        summary: Option<&str>,
    ) -> bool {
        self.blocked_sources
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(source))
            || self.blocked_topics.iter().any(|topic| {
                tags.iter().any(|tag| tag.eq_ignore_ascii_case(topic))
                    || title.to_lowercase().contains(topic)
                    || summary.is_some_and(|summary| summary.to_lowercase().contains(topic))
            })
    }
}

/// Shared discovery tokenization used by Explore routing and Personal Discovery matching.
///
/// Splits on non-alphanumeric characters, drops tokens of length ≤ 3 and a fixed stop
/// list, preserves input case, and caps output at 80 tokens.
#[must_use]
pub(crate) fn discovery_tokens(text: &str) -> Vec<String> {
    let stop = [
        "the",
        "and",
        "for",
        "with",
        "pod",
        "this",
        "that",
        "from",
        "into",
        "links",
        "link",
        "discovery",
        "personal",
        "public",
        "private",
        "use",
        "when",
        "brief",
        "style",
        "good",
        "bad",
        "stuff",
        "weird",
    ];
    let mut out = Vec::new();
    for token in text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() > 3)
    {
        if !stop.contains(&token) && !out.iter().any(|existing| existing == token) {
            out.push(token.to_string());
        }
        if out.len() >= 80 {
            break;
        }
    }
    out
}

/// Sensitive local Trust Policy edit requiring independent approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TrustPolicyChange {
    /// Add a replaceable Index Node used for outbound discovery queries.
    AddIndexNode {
        /// Local operator label.
        label: String,
        /// HTTPS base address, with loopback HTTP allowed for local operation.
        base_url: String,
    },
    /// Remove one Index Node and stop considering results received only from it.
    RemoveIndexNode {
        /// Configured Index Node base address.
        base_url: String,
    },
    /// Exclude one public Pod from local discovery.
    BlockPod {
        /// Origin hosting the excluded Pod.
        origin_node_id: NodeIdentityId,
        /// Origin-local Pod slug.
        pod_slug: String,
    },
    /// Exclude every announcement from one Origin Node.
    BlockNode {
        /// Excluded Origin identity.
        node_id: NodeIdentityId,
    },
    /// Exclude Content Reference samples from one source domain.
    BlockSource {
        /// Case-insensitive source domain.
        source: String,
    },
    /// Exclude matching Pod subjects and Content Reference samples.
    BlockTopic {
        /// Case-insensitive topic phrase.
        topic: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    #[default]
    DeepMatch,
    Adjacent,
    OldGem,
    HumanPick,
    RabbitHole,
    Stumble,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerSourceType {
    Rss,
    Atom,
    Sitemap,
    Webpage,
}
