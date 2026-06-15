//! Thin authenticated adapter for Google Search Console REST APIs.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gcp_auth::{CustomServiceAccount, TokenProvider};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Client, Method, RequestBuilder, Url};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::{OnceCell, RwLock};

use crate::config::Settings;
use crate::error::SearchConsoleError;

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b':')
    .add(b'[')
    .add(b']')
    .add(b'@')
    .add(b'!')
    .add(b'$')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b';')
    .add(b'=');

#[derive(Debug, Clone, Deserialize)]
struct OAuthClientSecretFile {
    #[serde(default)]
    installed: Option<OAuthClientConfig>,
    #[serde(default)]
    web: Option<OAuthClientConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthClientConfig {
    client_id: String,
    client_secret: String,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Debug, Clone)]
struct OAuthRefreshConfig {
    token_uri: String,
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthRefreshResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedAccessToken {
    value: String,
    refresh_after: Instant,
}

#[derive(Clone)]
enum UpstreamAuthMode {
    Adc,
    ServiceAccount {
        provider: Arc<dyn TokenProvider>,
        source: AuthSource,
    },
    OAuthRefresh(Arc<OAuthRefreshConfig>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    GoogleDefaultProviderChain,
    ServiceAccountJsonPath,
    ServiceAccountJsonEnv,
    OAuthRefreshToken,
}

impl AuthSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoogleDefaultProviderChain => "google_default_provider_chain",
            Self::ServiceAccountJsonPath => "service_account_json_path",
            Self::ServiceAccountJsonEnv => "service_account_json_env",
            Self::OAuthRefreshToken => "oauth_refresh_token",
        }
    }
}

#[derive(Clone)]
pub struct SearchConsoleClient {
    http: Client,
    auth_mode: UpstreamAuthMode,
    token_provider: Arc<OnceCell<Arc<dyn TokenProvider>>>,
    cached_oauth_token: Arc<RwLock<Option<CachedAccessToken>>>,
    scope: Arc<str>,
    api_base_url: Arc<str>,
    inspection_base_url: Arc<str>,
    quota_project: Option<Arc<str>>,
    max_row_limit: u32,
}

#[derive(Debug, Clone)]
pub struct SearchAnalyticsRequest {
    pub site_url: String,
    pub start_date: String,
    pub end_date: String,
    pub dimensions: Vec<String>,
    pub search_type: Option<String>,
    pub dimension_filter_groups: Option<Value>,
    pub aggregation_type: Option<String>,
    pub row_limit: Option<u32>,
    pub start_row: Option<u32>,
    pub data_state: Option<String>,
}

impl SearchConsoleClient {
    pub async fn from_settings(settings: &Settings) -> Result<Self, SearchConsoleError> {
        let mut headers = HeaderMap::new();
        let user_agent = HeaderValue::from_str(&settings.user_agent).map_err(|err| {
            SearchConsoleError::AuthBootstrap(format!("invalid user-agent value: {err}"))
        })?;
        headers.insert(USER_AGENT, user_agent);

        let http = Client::builder()
            .timeout(settings.http_timeout)
            .default_headers(headers)
            .build()
            .map_err(SearchConsoleError::Transport)?;

        let auth_mode = select_auth_mode(settings)?;

        Ok(Self {
            http,
            auth_mode,
            token_provider: Arc::new(OnceCell::new()),
            cached_oauth_token: Arc::new(RwLock::new(None)),
            scope: settings.scope.clone().into(),
            api_base_url: settings.api_base_url.clone().into(),
            inspection_base_url: settings.inspection_base_url.clone().into(),
            quota_project: settings
                .quota_project
                .as_ref()
                .map(|value| Arc::<str>::from(value.as_str())),
            max_row_limit: settings.max_row_limit,
        })
    }

    pub async fn list_sites(&self) -> Result<Value, SearchConsoleError> {
        self.get_json(&format!("{}/sites", self.api_base_url), &[])
            .await
    }

    pub fn auth_source(&self) -> AuthSource {
        match &self.auth_mode {
            UpstreamAuthMode::Adc => AuthSource::GoogleDefaultProviderChain,
            UpstreamAuthMode::ServiceAccount { source, .. } => *source,
            UpstreamAuthMode::OAuthRefresh(_) => AuthSource::OAuthRefreshToken,
        }
    }

    pub fn scope(&self) -> &str {
        self.scope.as_ref()
    }

    pub fn quota_project_configured(&self) -> bool {
        self.quota_project.is_some()
    }

    pub async fn verify_token(&self) -> Result<(), SearchConsoleError> {
        self.list_sites().await.map(|_| ())
    }

    pub async fn get_site(&self, site_url: &str) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        self.get_json(
            &format!(
                "{}/sites/{}",
                self.api_base_url,
                encode_path_segment(site_url)
            ),
            &[],
        )
        .await
    }

    pub async fn add_site(&self, site_url: &str) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        self.empty_request(
            Method::PUT,
            &format!(
                "{}/sites/{}",
                self.api_base_url,
                encode_path_segment(site_url)
            ),
        )
        .await
    }

    pub async fn delete_site(&self, site_url: &str) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        self.empty_request(
            Method::DELETE,
            &format!(
                "{}/sites/{}",
                self.api_base_url,
                encode_path_segment(site_url)
            ),
        )
        .await
    }

    pub async fn search_analytics_query(
        &self,
        request: SearchAnalyticsRequest,
    ) -> Result<Value, SearchConsoleError> {
        validate_site_url(&request.site_url)?;
        validate_iso_date("start_date", &request.start_date)?;
        validate_iso_date("end_date", &request.end_date)?;
        validate_dimensions(&request.dimensions)?;
        let row_limit = request.row_limit.unwrap_or(1_000);
        if row_limit == 0 || row_limit > self.max_row_limit {
            return Err(SearchConsoleError::invalid(
                "row_limit",
                format!("must be between 1 and {}", self.max_row_limit),
            ));
        }

        let mut body = Map::new();
        body.insert("startDate".to_string(), Value::String(request.start_date));
        body.insert("endDate".to_string(), Value::String(request.end_date));
        if !request.dimensions.is_empty() {
            body.insert(
                "dimensions".to_string(),
                Value::Array(request.dimensions.into_iter().map(Value::String).collect()),
            );
        }
        if let Some(search_type) = non_empty_opt(request.search_type) {
            body.insert("type".to_string(), Value::String(search_type));
        }
        if let Some(filters) = request.dimension_filter_groups {
            body.insert(
                "dimensionFilterGroups".to_string(),
                snake_to_camel_json(filters),
            );
        }
        if let Some(aggregation_type) = non_empty_opt(request.aggregation_type) {
            body.insert(
                "aggregationType".to_string(),
                Value::String(aggregation_type),
            );
        }
        body.insert("rowLimit".to_string(), json!(row_limit));
        if let Some(start_row) = request.start_row {
            body.insert("startRow".to_string(), json!(start_row));
        }
        if let Some(data_state) = non_empty_opt(request.data_state) {
            body.insert("dataState".to_string(), Value::String(data_state));
        }

        self.post_json(
            &format!(
                "{}/sites/{}/searchAnalytics/query",
                self.api_base_url,
                encode_path_segment(&request.site_url)
            ),
            Value::Object(body),
        )
        .await
    }

    pub async fn inspect_url(
        &self,
        site_url: &str,
        inspection_url: &str,
        language_code: Option<String>,
    ) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        validate_absolute_http_url("inspection_url", inspection_url)?;
        let mut body = Map::new();
        body.insert(
            "inspectionUrl".to_string(),
            Value::String(inspection_url.trim().to_string()),
        );
        body.insert(
            "siteUrl".to_string(),
            Value::String(site_url.trim().to_string()),
        );
        if let Some(language_code) = non_empty_opt(language_code) {
            body.insert("languageCode".to_string(), Value::String(language_code));
        }

        self.post_json(
            &format!("{}/urlInspection/index:inspect", self.inspection_base_url),
            Value::Object(body),
        )
        .await
    }

    pub async fn list_sitemaps(
        &self,
        site_url: &str,
        sitemap_index: Option<String>,
    ) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        let mut query = Vec::new();
        if let Some(index) = non_empty_opt(sitemap_index) {
            validate_absolute_http_url("sitemap_index", &index)?;
            query.push(("sitemapIndex", index));
        }
        self.get_json(
            &format!(
                "{}/sites/{}/sitemaps",
                self.api_base_url,
                encode_path_segment(site_url)
            ),
            &query,
        )
        .await
    }

    pub async fn get_sitemap(
        &self,
        site_url: &str,
        feed_path: &str,
    ) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        validate_absolute_http_url("feed_path", feed_path)?;
        self.get_json(
            &format!(
                "{}/sites/{}/sitemaps/{}",
                self.api_base_url,
                encode_path_segment(site_url),
                encode_path_segment(feed_path)
            ),
            &[],
        )
        .await
    }

    pub async fn submit_sitemap(
        &self,
        site_url: &str,
        feed_path: &str,
    ) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        validate_absolute_http_url("feed_path", feed_path)?;
        self.empty_request(
            Method::PUT,
            &format!(
                "{}/sites/{}/sitemaps/{}",
                self.api_base_url,
                encode_path_segment(site_url),
                encode_path_segment(feed_path)
            ),
        )
        .await
    }

    pub async fn delete_sitemap(
        &self,
        site_url: &str,
        feed_path: &str,
    ) -> Result<Value, SearchConsoleError> {
        validate_site_url(site_url)?;
        validate_absolute_http_url("feed_path", feed_path)?;
        self.empty_request(
            Method::DELETE,
            &format!(
                "{}/sites/{}/sitemaps/{}",
                self.api_base_url,
                encode_path_segment(site_url),
                encode_path_segment(feed_path)
            ),
        )
        .await
    }

    async fn get_json(
        &self,
        url: &str,
        query: &[(&str, String)],
    ) -> Result<Value, SearchConsoleError> {
        let token = self.access_token().await?;
        let url = parse_https_upstream_url(url)?;
        let mut request = self.http.request(Method::GET, url).bearer_auth(token);
        if let Some(quota_project) = &self.quota_project {
            request = request.header("x-goog-user-project", quota_project.as_ref());
        }
        if !query.is_empty() {
            request = request.query(query);
        }
        self.send_json(request).await
    }

    async fn post_json(&self, url: &str, body: Value) -> Result<Value, SearchConsoleError> {
        let token = self.access_token().await?;
        let url = parse_https_upstream_url(url)?;
        let mut request = self
            .http
            .request(Method::POST, url)
            .bearer_auth(token)
            .json(&body);
        if let Some(quota_project) = &self.quota_project {
            request = request.header("x-goog-user-project", quota_project.as_ref());
        }
        self.send_json(request).await
    }

    async fn empty_request(&self, method: Method, url: &str) -> Result<Value, SearchConsoleError> {
        let token = self.access_token().await?;
        let url = parse_https_upstream_url(url)?;
        let mut request = self.http.request(method, url).bearer_auth(token);
        if let Some(quota_project) = &self.quota_project {
            request = request.header("x-goog-user-project", quota_project.as_ref());
        }
        self.send_json(request).await
    }

    async fn send_json(&self, request: RequestBuilder) -> Result<Value, SearchConsoleError> {
        let response = request
            .send()
            .await
            .map_err(SearchConsoleError::Transport)?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(SearchConsoleError::Transport)?;

        if !status.is_success() {
            let message = String::from_utf8_lossy(&bytes).trim().to_string();
            return Err(SearchConsoleError::UpstreamApi {
                status: status.as_u16(),
                message: if message.is_empty() {
                    "no upstream response body".to_string()
                } else {
                    clip_message(message)
                },
            });
        }

        if bytes.is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_slice(&bytes).map_err(SearchConsoleError::UpstreamJson)
    }

    async fn token_provider(&self) -> Result<Arc<dyn TokenProvider>, SearchConsoleError> {
        let provider = self
            .token_provider
            .get_or_try_init(|| async {
                gcp_auth::provider()
                    .await
                    .map_err(|err| SearchConsoleError::AuthBootstrap(err.to_string()))
            })
            .await?;
        Ok(provider.clone())
    }

    async fn access_token(&self) -> Result<String, SearchConsoleError> {
        match &self.auth_mode {
            UpstreamAuthMode::Adc => {
                let provider = self.token_provider().await?;
                let token = provider.token(&[self.scope.as_ref()]).await?;
                Ok(token.as_str().to_string())
            }
            UpstreamAuthMode::ServiceAccount { provider, .. } => {
                let token = provider.token(&[self.scope.as_ref()]).await?;
                Ok(token.as_str().to_string())
            }
            UpstreamAuthMode::OAuthRefresh(config) => {
                if let Some(cached) = self.cached_oauth_token.read().await.as_ref()
                    && Instant::now() < cached.refresh_after
                {
                    return Ok(cached.value.clone());
                }

                let mut writer = self.cached_oauth_token.write().await;
                if let Some(cached) = writer.as_ref()
                    && Instant::now() < cached.refresh_after
                {
                    return Ok(cached.value.clone());
                }

                let token = self.refresh_oauth_access_token(config.as_ref()).await?;
                *writer = Some(token.clone());
                Ok(token.value)
            }
        }
    }

    async fn refresh_oauth_access_token(
        &self,
        config: &OAuthRefreshConfig,
    ) -> Result<CachedAccessToken, SearchConsoleError> {
        let response = self
            .http
            .request(Method::POST, &config.token_uri)
            .form(&[
                ("client_id", config.client_id.as_str()),
                ("client_secret", config.client_secret.as_str()),
                ("refresh_token", config.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(SearchConsoleError::Transport)?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(SearchConsoleError::Transport)?;
        let parsed: OAuthRefreshResponse = serde_json::from_slice(&bytes).map_err(|err| {
            SearchConsoleError::AuthBootstrap(format!(
                "failed to parse OAuth token response: {err}"
            ))
        })?;

        if !status.is_success() {
            let error = parsed.error.as_deref().unwrap_or("unknown_error");
            let detail = parsed
                .error_description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    clip_message(String::from_utf8_lossy(&bytes).trim().to_string())
                });
            return Err(SearchConsoleError::AuthBootstrap(format!(
                "oauth refresh exchange failed with status {} ({error}): {detail}",
                status.as_u16()
            )));
        }

        let Some(access_token) = parsed.access_token else {
            return Err(SearchConsoleError::AuthBootstrap(
                "oauth refresh exchange succeeded without access_token".to_string(),
            ));
        };
        let expires_in = parsed.expires_in.unwrap_or(3600);
        let refresh_in = expires_in.saturating_sub(60).max(1);
        Ok(CachedAccessToken {
            value: access_token,
            refresh_after: Instant::now() + Duration::from_secs(refresh_in),
        })
    }
}

fn select_auth_mode(settings: &Settings) -> Result<UpstreamAuthMode, SearchConsoleError> {
    let oauth_client_secret_json = settings
        .oauth_client_secret_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let oauth_refresh_token = settings
        .oauth_refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(raw_json) = settings
        .service_account_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let provider: Arc<dyn TokenProvider> =
            Arc::new(CustomServiceAccount::from_json(raw_json).map_err(|err| {
                SearchConsoleError::AuthBootstrap(format!(
                    "invalid service account JSON in GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON: {err}"
                ))
            })?);
        return Ok(UpstreamAuthMode::ServiceAccount {
            provider,
            source: AuthSource::ServiceAccountJsonEnv,
        });
    }

    if let Some(path) = settings
        .service_account_json_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let provider: Arc<dyn TokenProvider> =
            Arc::new(CustomServiceAccount::from_file(path).map_err(|err| {
                SearchConsoleError::AuthBootstrap(format!(
                    "failed to load service account JSON at '{path}': {err}"
                ))
            })?);
        return Ok(UpstreamAuthMode::ServiceAccount {
            provider,
            source: AuthSource::ServiceAccountJsonPath,
        });
    }

    match (oauth_client_secret_json, oauth_refresh_token) {
        (Some(client_secret_path), Some(refresh_token)) => Ok(UpstreamAuthMode::OAuthRefresh(
            Arc::new(parse_oauth_refresh_config(client_secret_path, refresh_token)?),
        )),
        (None, None) => Ok(UpstreamAuthMode::Adc),
        _ => Err(SearchConsoleError::AuthBootstrap(
            "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON and GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN must both be set or both be unset; refusing to fall back to ADC with partial OAuth configuration".to_string(),
        )),
    }
}

pub fn encode_path_segment(value: &str) -> String {
    utf8_percent_encode(value.trim(), PATH_SEGMENT_ENCODE_SET).to_string()
}

pub fn validate_site_url(value: &str) -> Result<(), SearchConsoleError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SearchConsoleError::invalid("site_url", "must not be empty"));
    }
    if let Some(domain) = trimmed.strip_prefix("sc-domain:") {
        if domain.trim().is_empty()
            || domain.contains('/')
            || domain.chars().any(char::is_whitespace)
        {
            return Err(SearchConsoleError::invalid(
                "site_url",
                "domain properties must look like sc-domain:example.com",
            ));
        }
        return Ok(());
    }
    let parsed = Url::parse(trimmed).map_err(|err| {
        SearchConsoleError::invalid("site_url", format!("must be an absolute URL: {err}"))
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(SearchConsoleError::invalid(
                "site_url",
                "must use http or https scheme",
            ));
        }
    }
    if !trimmed.ends_with('/') || !parsed.path().ends_with('/') {
        return Err(SearchConsoleError::invalid(
            "site_url",
            "URL-prefix properties must include a trailing slash, for example https://www.example.com/",
        ));
    }
    Ok(())
}

pub fn validate_absolute_http_url(
    field: &'static str,
    value: &str,
) -> Result<(), SearchConsoleError> {
    let parsed = Url::parse(value.trim()).map_err(|err| {
        SearchConsoleError::invalid(field, format!("must be an absolute URL: {err}"))
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(SearchConsoleError::invalid(
            field,
            "must use http or https scheme",
        )),
    }
}

pub fn validate_iso_date(field: &'static str, value: &str) -> Result<(), SearchConsoleError> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(idx, byte)| idx == 4 || idx == 7 || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(SearchConsoleError::invalid(
            field,
            "must use YYYY-MM-DD format",
        ))
    }
}

fn validate_dimensions(dimensions: &[String]) -> Result<(), SearchConsoleError> {
    let mut seen = Vec::new();
    for dimension in dimensions {
        let trimmed = dimension.trim();
        if trimmed.is_empty() {
            return Err(SearchConsoleError::invalid(
                "dimensions",
                "dimension names must not be empty",
            ));
        }
        if seen.contains(&trimmed) {
            return Err(SearchConsoleError::invalid(
                "dimensions",
                format!("dimension '{trimmed}' appears more than once"),
            ));
        }
        seen.push(trimmed);
    }
    Ok(())
}

fn parse_https_upstream_url(raw: &str) -> Result<Url, SearchConsoleError> {
    let url = Url::parse(raw).map_err(|err| {
        SearchConsoleError::invalid("upstream_url", format!("invalid upstream URL: {err}"))
    })?;
    if url.scheme() != "https" {
        return Err(SearchConsoleError::invalid(
            "upstream_url",
            "upstream Google API URL must use https",
        ));
    }
    Ok(url)
}

fn parse_oauth_refresh_config(
    client_secret_json_path: &str,
    refresh_token: &str,
) -> Result<OAuthRefreshConfig, SearchConsoleError> {
    let raw = fs::read_to_string(client_secret_json_path).map_err(|err| {
        SearchConsoleError::AuthBootstrap(format!(
            "failed to read OAuth client secret JSON at '{client_secret_json_path}': {err}"
        ))
    })?;
    let parsed: OAuthClientSecretFile = serde_json::from_str(&raw).map_err(|err| {
        SearchConsoleError::AuthBootstrap(format!(
            "invalid OAuth client secret JSON at '{client_secret_json_path}': {err}"
        ))
    })?;
    let client = parsed.installed.or(parsed.web).ok_or_else(|| {
        SearchConsoleError::AuthBootstrap(
            "OAuth client secret JSON must contain either 'installed' or 'web' object".to_string(),
        )
    })?;

    let client_id = client.client_id.trim();
    if client_id.is_empty() {
        return Err(SearchConsoleError::AuthBootstrap(
            "OAuth client secret JSON is missing client_id".to_string(),
        ));
    }
    let client_secret = client.client_secret.trim();
    if client_secret.is_empty() {
        return Err(SearchConsoleError::AuthBootstrap(
            "OAuth client secret JSON is missing client_secret".to_string(),
        ));
    }
    let token_uri = client
        .token_uri
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("https://oauth2.googleapis.com/token");
    validate_oauth_token_uri(token_uri)?;

    Ok(OAuthRefreshConfig {
        token_uri: token_uri.to_string(),
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        refresh_token: refresh_token.to_string(),
    })
}

fn clip_message(message: String) -> String {
    const MAX_LEN: usize = 1_024;
    if message.len() <= MAX_LEN {
        return message;
    }
    let mut end = MAX_LEN;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &message[..end])
}

fn validate_oauth_token_uri(token_uri: &str) -> Result<(), SearchConsoleError> {
    let parsed = Url::parse(token_uri).map_err(|err| {
        SearchConsoleError::AuthBootstrap(format!(
            "invalid OAuth token_uri '{token_uri}' in GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON: {err}"
        ))
    })?;
    if parsed.scheme() != "https" {
        return Err(SearchConsoleError::AuthBootstrap(format!(
            "OAuth token_uri '{token_uri}' must use https"
        )));
    }

    let host = parsed.host_str().unwrap_or("");
    let path = parsed.path();
    let allowed = (host == "oauth2.googleapis.com" && path == "/token")
        || (host == "accounts.google.com" && path == "/o/oauth2/token");
    if !allowed {
        return Err(SearchConsoleError::AuthBootstrap(format!(
            "OAuth token_uri '{token_uri}' must be one of https://oauth2.googleapis.com/token or https://accounts.google.com/o/oauth2/token"
        )));
    }
    Ok(())
}

fn non_empty_opt(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn snake_to_camel_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let converted = map
                .into_iter()
                .map(|(key, value)| (snake_key_to_camel(&key), snake_to_camel_json(value)))
                .collect::<Map<String, Value>>();
            Value::Object(converted)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(snake_to_camel_json).collect()),
        other => other,
    }
}

fn snake_key_to_camel(key: &str) -> String {
    if !key.contains('_') {
        return key.to_string();
    }
    let mut out = String::with_capacity(key.len());
    let mut uppercase_next = false;
    for ch in key.chars() {
        if ch == '_' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            out.push(ch.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_search_console_path_segments() {
        assert_eq!(
            encode_path_segment("https://www.example.com/"),
            "https%3A%2F%2Fwww.example.com%2F"
        );
        assert_eq!(
            encode_path_segment("sc-domain:example.com"),
            "sc-domain%3Aexample.com"
        );
    }

    #[test]
    fn validates_site_url_forms() {
        validate_site_url("https://www.example.com/").expect("url-prefix property");
        validate_site_url("https://www.example.com/path/").expect("nested url-prefix property");
        assert!(validate_site_url("https://www.example.com").is_err());
        assert!(validate_site_url("https://www.example.com/path").is_err());
        validate_site_url("sc-domain:example.com").expect("domain property");
        assert!(validate_site_url("").is_err());
        assert!(validate_site_url("sc-domain:bad/path").is_err());
    }

    #[test]
    fn validates_iso_dates_by_shape() {
        validate_iso_date("start_date", "2026-06-14").expect("date shape");
        assert!(validate_iso_date("start_date", "14-06-2026").is_err());
    }

    #[test]
    fn rejects_partial_oauth_configuration() {
        let base = test_settings();

        let mut with_secret_only = base.clone();
        with_secret_only.oauth_client_secret_json = Some("/tmp/client.json".to_string());
        with_secret_only.oauth_refresh_token = None;
        match select_auth_mode(&with_secret_only) {
            Err(SearchConsoleError::AuthBootstrap(message)) => {
                assert!(message.contains("must both be set or both be unset"))
            }
            Ok(_) => panic!("unexpected auth mode result: Ok"),
            Err(err) => panic!("unexpected auth mode result: {err}"),
        }

        let mut with_token_only = base;
        with_token_only.oauth_client_secret_json = None;
        with_token_only.oauth_refresh_token = Some("refresh-token".to_string());
        match select_auth_mode(&with_token_only) {
            Err(SearchConsoleError::AuthBootstrap(message)) => {
                assert!(message.contains("must both be set or both be unset"))
            }
            Ok(_) => panic!("unexpected auth mode result: Ok"),
            Err(err) => panic!("unexpected auth mode result: {err}"),
        }
    }

    #[test]
    fn rejects_non_google_oauth_token_uri() {
        let client_secret = tempfile::NamedTempFile::new().expect("temp file");
        std::fs::write(
            client_secret.path(),
            r#"{"installed":{"client_id":"client","client_secret":"secret","token_uri":"http://evil.test/token"}}"#,
        )
        .expect("write secret json");
        let err = parse_oauth_refresh_config(
            client_secret.path().to_str().expect("utf-8 path"),
            "refresh-token",
        )
        .expect_err("invalid token uri");
        assert!(
            matches!(err, SearchConsoleError::AuthBootstrap(message) if message.contains("must use https"))
        );

        let client_secret = tempfile::NamedTempFile::new().expect("temp file");
        std::fs::write(
            client_secret.path(),
            r#"{"installed":{"client_id":"client","client_secret":"secret","token_uri":"https://evil.test/token"}}"#,
        )
        .expect("write secret json");
        let err = parse_oauth_refresh_config(
            client_secret.path().to_str().expect("utf-8 path"),
            "refresh-token",
        )
        .expect_err("invalid token uri");
        assert!(
            matches!(err, SearchConsoleError::AuthBootstrap(message) if message.contains("must be one of"))
        );
    }

    #[test]
    fn clips_multibyte_messages_without_panicking() {
        let message = format!("{}a", "é".repeat(512));
        let clipped = clip_message(message);
        assert!(clipped.ends_with("..."));
        assert!(clipped.len() <= 1_027);
        assert!(clipped.chars().all(|ch| ch == 'é' || ch == '.'));
    }

    fn test_settings() -> Settings {
        Settings {
            profile: crate::config::CapabilityProfile::ReadOnly,
            scope: "https://www.googleapis.com/auth/webmasters.readonly".to_string(),
            api_base_url: "https://www.googleapis.com/webmasters/v3".to_string(),
            inspection_base_url: "https://searchconsole.googleapis.com/v1".to_string(),
            http_timeout: Duration::from_secs(15),
            user_agent: "test-agent".to_string(),
            oauth_client_secret_json: None,
            oauth_refresh_token: None,
            service_account_json_path: None,
            service_account_json: None,
            quota_project: None,
            max_row_limit: 25_000,
            print_tools: false,
            print_tool_schema: false,
            command: None,
        }
    }
}
