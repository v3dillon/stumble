use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use stumble_core::*;

#[derive(Debug, Clone)]
pub struct CrawlerConfig {
    pub user_agent: String,
    pub per_domain_delay: Duration,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            user_agent: "stumble-crawler/0.1".to_string(),
            per_domain_delay: Duration::from_secs(30),
        }
    }
}

pub struct CautiousCrawler {
    client: reqwest::Client,
    config: CrawlerConfig,
    last_fetch_by_domain: HashMap<String, Instant>,
}

impl CautiousCrawler {
    pub fn new(config: CrawlerConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(&config.user_agent)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self {
            client,
            config,
            last_fetch_by_domain: HashMap::new(),
        })
    }

    pub async fn crawl_source(
        &mut self,
        tools: &AgentTools,
        ctx: &AuthContext,
        pod_slug: &str,
        source: &CrawlerSource,
    ) -> anyhow::Result<Vec<CrawlCandidate>> {
        if !source.enabled {
            return Ok(vec![]);
        }
        let domain = url::Url::parse(&source.url)?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        if let Some(last) = self.last_fetch_by_domain.get(&domain) {
            let elapsed = last.elapsed();
            if elapsed < self.config.per_domain_delay {
                tokio::time::sleep(self.config.per_domain_delay - elapsed).await;
            }
        }
        let response = self.client.get(&source.url).send().await?;
        self.last_fetch_by_domain.insert(domain, Instant::now());
        let text = response.text().await?;
        let candidates = discover_links_heuristic(&text)
            .into_iter()
            .take(10)
            .map(|url| {
                tools.create_crawl_candidate(
                    ctx,
                    pod_slug,
                    source.id,
                    SubmitLinkRequest {
                        url: url.clone(),
                        title: Some(url),
                        description: Some(
                            "Crawler-discovered candidate; metadata enrichment pending."
                                .to_string(),
                        ),
                        note: None,
                        tags: vec!["crawler".to_string()],
                        discovered_by_crawler: true,
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates)
    }
}

pub fn discover_links_heuristic(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>')
        .filter(|token| token.starts_with("https://") || token.starts_with("http://"))
        .map(|token| token.trim_end_matches([',', '.', ')']).to_string())
        .collect()
}
