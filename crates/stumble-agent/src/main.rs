use anyhow::{Context, Result};
use clap::Parser;
use reqwest::{header, StatusCode};
use serde_json::json;
use stumble_core::{
    CreatePodRequest, DevTokenRequest, DevTokenResponse, DiscoverRequest, DiscoveryItem,
    DiscoveryMode, GenerateBriefRequest, PodSkillPack, SubmitLinkRequest, UpdatePreferencesRequest,
    UserPreferences, Visibility,
};

const POD_SLUG: &str = "dillon-tech-ai-aliens";

#[derive(Debug, Parser)]
#[command(
    name = "interest-agent",
    about = "HTTP agent that bootstraps Dillon's tech/AI/aliens discovery pod"
)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    api: String,
    #[arg(long, default_value = "Dillon Interest Agent")]
    label: String,
    #[arg(long, default_value = POD_SLUG)]
    pod_slug: String,
    #[arg(long)]
    keep_alive: bool,
    #[arg(long, default_value_t = 300)]
    rediscover_interval_seconds: u64,
    #[arg(long)]
    seed_starter_links: bool,
    #[arg(long = "submit-link-url")]
    submit_link_urls: Vec<String>,
    #[arg(long)]
    submit_link_title: Option<String>,
    #[arg(long)]
    submit_link_note: Option<String>,
    #[arg(long, value_delimiter = ',')]
    submit_link_tags: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = reqwest::Client::new();
    let api = args.api.trim_end_matches('/').to_string();

    health_check(&client, &api).await?;
    let token = create_token(&client, &api, &args.label).await?;
    let authed = AuthedClient {
        client,
        api,
        token: token.token,
    };

    let preferences: UserPreferences = authed
        .patch_json(
            "/me/preferences",
            &UpdatePreferencesRequest {
                interests: Some(vec![
                    "tech".to_string(),
                    "ai".to_string(),
                    "aliens".to_string(),
                    "uap".to_string(),
                    "space".to_string(),
                    "interfaces".to_string(),
                ]),
                blocked_topics: Some(vec!["politics".to_string(), "generic ai hype".to_string()]),
                blocked_sources: None,
                preferred_brief_length: Some(7),
                preferred_discovery_mode: Some(DiscoveryMode::DeepMatch),
            },
        )
        .await?;

    ensure_pod(&authed, &args.pod_slug).await?;
    let pod_skill = read_pod_skill(&authed, &args.pod_slug).await?;
    let mut submitted = if args.seed_starter_links {
        submit_interest_links(&authed, &args.pod_slug).await?
    } else {
        Vec::new()
    };
    submitted.append(
        &mut submit_user_provided_links(
            &authed,
            &args.pod_slug,
            &args.submit_link_urls,
            args.submit_link_title.as_deref(),
            args.submit_link_note.as_deref(),
            &args.submit_link_tags,
        )
        .await?,
    );
    let discoveries = discover(&authed, &args.pod_slug).await?;
    let _brief_skill = read_pod_skill(&authed, &args.pod_slug).await?;
    let brief: serde_json::Value = authed
        .post_json(
            "/briefs/generate",
            &GenerateBriefRequest {
                pod_slugs: vec![args.pod_slug.clone()],
                query: Some("practical tech, AI, and aliens research trail".to_string()),
                user_id: None,
            },
        )
        .await?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "agent": args.label,
            "node": authed.api,
            "interests_stored": preferences,
            "pod_slug": args.pod_slug,
            "pod_skill_read": skill_read_receipt(&pod_skill),
            "submitted_links": submitted,
            "submission_policy": "No agent-generated links are submitted unless --seed-starter-links is explicitly set or --submit-link-url provides a user-approved URL.",
            "discoveries": discoveries,
            "brief": brief,
            "status": "agent communicated with local node, created/used pod, submitted links, and discovered items"
        }))?
    );

    if args.keep_alive {
        eprintln!(
            "interest-agent keep-alive enabled; rediscovering every {} seconds",
            args.rediscover_interval_seconds
        );
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(
                args.rediscover_interval_seconds,
            ))
            .await;
            match discover(&authed, &args.pod_slug).await {
                Ok(items) => {
                    let titles = items
                        .iter()
                        .map(|item| item.title.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "interest-agent rediscovered {} items: {titles}",
                        items.len()
                    );
                }
                Err(error) => eprintln!("interest-agent rediscovery failed: {error:#}"),
            }
        }
    }

    Ok(())
}

async fn health_check(client: &reqwest::Client, api: &str) -> Result<()> {
    let status = client
        .get(format!("{api}/health"))
        .send()
        .await
        .context("local node did not respond to /health")?
        .error_for_status()
        .context("local node /health returned an error")?
        .text()
        .await?;
    eprintln!("local node health: {status}");
    Ok(())
}

async fn create_token(
    client: &reqwest::Client,
    api: &str,
    label: &str,
) -> Result<DevTokenResponse> {
    client
        .post(format!("{api}/auth/dev-token"))
        .json(&DevTokenRequest {
            user_id: None,
            tenant_slug: None,
            label: label.to_string(),
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("failed to create dev token")
}

async fn ensure_pod(authed: &AuthedClient, pod_slug: &str) -> Result<()> {
    let pods: serde_json::Value = authed.get_json("/pods").await?;
    let exists = pods.as_array().is_some_and(|pods| {
        pods.iter()
            .any(|pod| pod.get("slug").and_then(|v| v.as_str()) == Some(pod_slug))
    });
    if exists {
        return Ok(());
    }
    let _: serde_json::Value = authed
        .post_json(
            "/pods",
            &CreatePodRequest {
                name: "Dillon Tech AI Aliens".to_string(),
                slug: pod_slug.to_string(),
                description: "A personal discovery pod for serious tech, practical AI, alien/UAP research, space signals, and weird evidence trails without politics or generic hype.".to_string(),
                visibility: Visibility::Private,
            },
        )
        .await?;
    Ok(())
}

async fn submit_interest_links(
    authed: &AuthedClient,
    pod_slug: &str,
) -> Result<Vec<serde_json::Value>> {
    let _skill = read_pod_skill(authed, pod_slug).await?;
    let links = [
        (
            "Attention Is All You Need",
            "https://arxiv.org/abs/1706.03762",
            "Transformer architecture root text; useful for tracking durable AI ideas.",
            vec!["ai", "tech", "research", "foundational"],
        ),
        (
            "Building Effective Agents",
            "https://www.anthropic.com/research/building-effective-agents",
            "Practical agent engineering patterns, not generic hype.",
            vec!["ai", "agents", "tech", "practical"],
        ),
        (
            "NASA Exoplanets",
            "https://science.nasa.gov/exoplanets/",
            "Grounded space science for thinking about alien-life search constraints.",
            vec!["aliens", "space", "exoplanets", "science"],
        ),
        (
            "SETI Institute",
            "https://www.seti.org/",
            "Search-for-intelligence research source for the aliens side of the pod.",
            vec!["aliens", "seti", "signals", "space"],
        ),
        (
            "NASA UAP Independent Study",
            "https://science.nasa.gov/uap/",
            "Institutional UAP reference point to separate evidence from noise.",
            vec!["aliens", "uap", "research", "evidence"],
        ),
    ];

    let mut submitted = Vec::new();
    for (title, url, note, tags) in links {
        let item = authed
            .post_json_allow_bad_request(
                &format!("/pods/{pod_slug}/submit"),
                &SubmitLinkRequest {
                    url: url.to_string(),
                    title: Some(title.to_string()),
                    description: Some(note.to_string()),
                    note: Some(note.to_string()),
                    tags: tags.into_iter().map(ToString::to_string).collect(),
                    discovered_by_crawler: false,
                },
            )
            .await?;
        submitted.push(item);
    }
    Ok(submitted)
}

async fn submit_user_provided_links(
    authed: &AuthedClient,
    pod_slug: &str,
    urls: &[String],
    title: Option<&str>,
    note: Option<&str>,
    tags: &[String],
) -> Result<Vec<serde_json::Value>> {
    if urls.is_empty() {
        return Ok(Vec::new());
    }
    let _skill = read_pod_skill(authed, pod_slug).await?;
    let mut submitted = Vec::new();
    for url in urls {
        let item = authed
            .post_json_allow_bad_request(
                &format!("/pods/{pod_slug}/submit"),
                &SubmitLinkRequest {
                    url: url.to_string(),
                    title: title.map(ToString::to_string),
                    description: note.map(ToString::to_string),
                    note: note.map(ToString::to_string),
                    tags: tags.to_vec(),
                    discovered_by_crawler: false,
                },
            )
            .await?;
        submitted.push(item);
    }
    Ok(submitted)
}

async fn discover(authed: &AuthedClient, pod_slug: &str) -> Result<Vec<DiscoveryItem>> {
    let _skill = read_pod_skill(authed, pod_slug).await?;
    authed
        .post_json(
            &format!("/pods/{pod_slug}/discover"),
            &DiscoverRequest {
                query: "Find grounded, weird-but-practical links about tech, AI, aliens, UAP, and space signals".to_string(),
                avoid: vec!["politics".to_string(), "generic ai hype".to_string()],
                limit: 7,
                mode: DiscoveryMode::DeepMatch,
                user_id: None,
            },
        )
        .await
}

async fn read_pod_skill(authed: &AuthedClient, pod_slug: &str) -> Result<PodSkillPack> {
    let pack: PodSkillPack = authed
        .get_json(&format!("/pods/{pod_slug}/skill-pack"))
        .await
        .with_context(|| format!("failed to read SKILL.md for pod {pod_slug}"))?;
    if pack.skill_md.trim().is_empty() {
        anyhow::bail!("pod {pod_slug} has an empty SKILL.md");
    }
    Ok(pack)
}

fn skill_read_receipt(pack: &PodSkillPack) -> serde_json::Value {
    json!({
        "pod_id": pack.pod_id,
        "skill_pack_version": pack.version,
        "skill_md_bytes": pack.skill_md.len(),
        "valid_for_agent_context": !pack.skill_md.trim().is_empty(),
    })
}

struct AuthedClient {
    client: reqwest::Client,
    api: String,
    token: String,
}

impl AuthedClient {
    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.client
            .get(format!("{}{}", self.api, path))
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("GET {path} failed"))
    }

    async fn post_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.client
            .post(format!("{}{}", self.api, path))
            .header(header::CONTENT_TYPE, "application/json")
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("POST {path} failed"))
    }

    async fn post_json_allow_bad_request<B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<serde_json::Value> {
        let response = self
            .client
            .post(format!("{}{}", self.api, path))
            .header(header::CONTENT_TYPE, "application/json")
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        if response.status() == StatusCode::BAD_REQUEST {
            return Ok(json!({"status":"skipped_or_existing","detail": response.text().await?}));
        }
        response
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("POST {path} failed"))
    }

    async fn patch_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.client
            .patch(format!("{}{}", self.api, path))
            .header(header::CONTENT_TYPE, "application/json")
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("PATCH {path} failed"))
    }
}
