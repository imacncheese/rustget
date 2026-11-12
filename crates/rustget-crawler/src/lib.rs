//! Bounded same-origin crawler used by the site-grabber workflow.
//!
//! This is a link discovery engine, not a browser: it does not execute
//! JavaScript, submit forms or authenticate interactively. That is a deliberate
//! safety and predictability boundary.
#![forbid(unsafe_code)]

use std::collections::{HashSet, VecDeque};

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::{Position, Url};

#[derive(Debug, Error)]
pub enum CrawlError {
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub struct CrawlConfig {
    pub max_depth: usize,
    pub max_pages: usize,
    pub same_origin: bool,
    pub respect_robots: bool,
    pub extensions: Vec<String>,
    pub user_agent: String,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_pages: 100,
            same_origin: true,
            respect_robots: true,
            extensions: Vec::new(),
            user_agent: format!("RustGet/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
    pub start_url: String,
    pub pages_visited: usize,
    pub files: Vec<FoundFile>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundFile {
    pub url: String,
    pub source_page: String,
    pub extension: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RobotsRules {
    disallow: Vec<String>,
}

impl RobotsRules {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut disallow = Vec::new();
        let mut applies = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim().to_ascii_lowercase().as_str() {
                "user-agent" => {
                    applies = value.trim() == "*" || value.trim().eq_ignore_ascii_case("rustget");
                }
                "disallow" if applies && !value.trim().is_empty() => {
                    disallow.push(value.trim().to_string());
                }
                _ => {}
            }
        }
        Self { disallow }
    }

    #[must_use]
    pub fn allowed(&self, url: &Url) -> bool {
        let path = &url[Position::BeforePath..Position::AfterQuery];
        !self.disallow.iter().any(|rule| path.starts_with(rule))
    }
}

pub struct Crawler {
    client: reqwest::Client,
    config: CrawlConfig,
    link_regex: Regex,
}

impl Crawler {
    pub fn new(config: CrawlConfig) -> Result<Self, CrawlError> {
        let client = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;
        Ok(Self {
            client,
            config,
            link_regex: Regex::new(r#"(?i)(?:href|src)\s*=\s*["']([^"']+)["']"#)
                .expect("static regex"),
        })
    }

    pub async fn crawl(&self, start: Url) -> Result<CrawlResult, CrawlError> {
        let robots = if self.config.respect_robots {
            self.fetch_robots(&start).await
        } else {
            RobotsRules::default()
        };
        let start_origin = origin(&start);
        let mut queue = VecDeque::from([(start.clone(), 0_usize)]);
        let mut visited = HashSet::new();
        let mut files = Vec::new();
        let mut seen_files = HashSet::new();
        let mut errors = Vec::new();
        let mut pages = 0_usize;

        while let Some((url, depth)) = queue.pop_front() {
            if pages >= self.config.max_pages || !visited.insert(url.clone()) {
                continue;
            }
            if !robots.allowed(&url) {
                continue;
            }
            pages += 1;
            let response = match self.client.get(url.clone()).send().await {
                Ok(response) => response,
                Err(error) => {
                    errors.push(format!("{url}: {error}"));
                    continue;
                }
            };
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_html = content_type.contains("text/html") || content_type.is_empty();
            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    errors.push(format!("{url}: {error}"));
                    continue;
                }
            };
            if !is_html {
                continue;
            }
            for capture in self.link_regex.captures_iter(&body) {
                let Some(raw) = capture.get(1) else { continue };
                let Ok(next) = url.join(raw.as_str()) else {
                    continue;
                };
                if next.scheme() != "http" && next.scheme() != "https" {
                    continue;
                }
                if self.config.same_origin && origin(&next) != start_origin {
                    continue;
                }
                let extension = extension(&next);
                let is_file = extension.as_ref().is_some_and(|extension| {
                    self.config.extensions.is_empty()
                        || self
                            .config
                            .extensions
                            .iter()
                            .any(|wanted| wanted.eq_ignore_ascii_case(extension))
                });
                if is_file
                    && robots.allowed(&next)
                    && seen_files.insert(next.to_string())
                    && !next.path().ends_with('/')
                {
                    files.push(FoundFile {
                        url: next.to_string(),
                        source_page: url.to_string(),
                        extension,
                    });
                } else if depth < self.config.max_depth && !next.path().ends_with('/') {
                    queue.push_back((next, depth + 1));
                }
            }
        }
        Ok(CrawlResult {
            start_url: start.to_string(),
            pages_visited: pages,
            files,
            errors,
        })
    }

    async fn fetch_robots(&self, start: &Url) -> RobotsRules {
        let Ok(robots_url) = start.join("/robots.txt") else {
            return RobotsRules::default();
        };
        match self.client.get(robots_url).send().await {
            Ok(response) if response.status().is_success() => {
                RobotsRules::parse(&response.text().await.unwrap_or_default())
            }
            _ => RobotsRules::default(),
        }
    }
}

fn origin(url: &Url) -> String {
    format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default()
    )
}

fn extension(url: &Url) -> Option<String> {
    let name = url.path_segments()?.next_back()?;
    let (_, extension) = name.rsplit_once('.')?;
    (!extension.is_empty()).then(|| extension.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots_rules_are_prefix_based() {
        let rules = RobotsRules::parse("User-agent: *\nDisallow: /private\nAllow: /public\n");
        assert!(!rules.allowed(&Url::parse("https://x.test/private/a").unwrap()));
        assert!(rules.allowed(&Url::parse("https://x.test/public/a").unwrap()));
    }

    #[test]
    fn extensions_and_origins() {
        let url = Url::parse("https://x.test/a/B.PDF?x=1").unwrap();
        assert_eq!(extension(&url), Some("pdf".into()));
        assert_eq!(origin(&url), "https://x.test");
    }
}
