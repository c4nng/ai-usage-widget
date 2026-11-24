//! Network side of every provider: read credentials, call the endpoint, hand
//! the body to the matching `usage_core` parser.
//!
//! Subscription providers borrow the login of the official CLI (Claude Code,
//! Codex). Their tokens are only ever sent to the provider that issued them,
//! and they are never refreshed here: refresh tokens rotate, and refreshing
//! behind the CLI's back would sign the user out of it.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::Value;
use usage_core::{claude, codex, cost, time, ErrorCode, ProviderError, ProviderKind, ProviderReport, Usage};

use crate::config::ProviderConfig;
use crate::secrets;

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// The usage endpoint rate-limits clients that don't identify as Claude Code much harder.
const CLAUDE_USER_AGENT: &str = concat!("claude-code/2.0.0 (ai-usage-widget/", env!("CARGO_PKG_VERSION"), ")");
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_USER_AGENT: &str = concat!("codex_cli_rs/0.0.0 (ai-usage-widget/", env!("CARGO_PKG_VERSION"), ")");
const ANTHROPIC_COST_URL: &str = "https://api.anthropic.com/v1/organizations/cost_report";
const OPENAI_COST_URL: &str = "https://api.openai.com/v1/organization/costs";
/// A month has at most 31 daily buckets; this only guards against a runaway cursor.
const MAX_COST_PAGES: usize = 6;

pub fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("ai-usage-widget/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("TLS backend should initialise")
}

/// Where the official CLI keeps its login, honouring the CLIs' own env overrides.
pub fn default_credentials_path(kind: ProviderKind) -> Option<PathBuf> {
    let from_env = |var: &str| std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from);
    match kind {
        ProviderKind::ClaudeSubscription => from_env("CLAUDE_CONFIG_DIR")
            .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
            .map(|d| d.join(".credentials.json")),
        ProviderKind::CodexSubscription => {
            from_env("CODEX_HOME").or_else(|| dirs::home_dir().map(|h| h.join(".codex"))).map(|d| d.join("auth.json"))
        }
        ProviderKind::AnthropicApi | ProviderKind::OpenaiApi => None,
    }
}

pub async fn fetch_all(client: &Client, providers: Vec<ProviderConfig>) -> Vec<ProviderReport> {
    let tasks: Vec<_> = providers
        .into_iter()
        .filter(|p| p.enabled)
        .map(|p| {
            let client = client.clone();
            tauri::async_runtime::spawn(async move { fetch_one(&client, p).await })
        })
        .collect();

    let mut reports = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(report) = task.await {
            reports.push(report);
        }
    }
    reports
}

async fn fetch_one(client: &Client, provider: ProviderConfig) -> ProviderReport {
    let result = match provider.kind {
        ProviderKind::ClaudeSubscription => claude_subscription(client, &provider).await,
        ProviderKind::CodexSubscription => codex_subscription(client, &provider).await,
        ProviderKind::AnthropicApi => anthropic_api(client, &provider).await,
        ProviderKind::OpenaiApi => openai_api(client, &provider).await,
    };
    let (usage, error) = match result {
        Ok(usage) => (Some(usage), None),
        Err(e) => (None, Some(e)),
    };
    ProviderReport {
        id: provider.id,
        label: provider.label,
        kind: provider.kind,
        fetched_at: now(),
        usage,
        error,
        stale: false,
    }
}

async fn claude_subscription(client: &Client, p: &ProviderConfig) -> Result<Usage, ProviderError> {
    let creds = read_credentials(p, "Claude Code", "claude")?;
    let oauth = creds.get("claudeAiOauth").ok_or_else(|| {
        ProviderError::new(
            ErrorCode::CredentialsMissing,
            "No Claude.ai login in the credentials file. Run `claude` and sign in with your subscription.",
        )
    })?;
    let token = oauth.get("accessToken").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::new(ErrorCode::CredentialsMissing, "The credentials file has no access token.")
    })?;
    if let Some(expires_ms) = oauth.get("expiresAt").and_then(Value::as_i64) {
        if expires_ms / 1000 <= now() {
            return Err(ProviderError::new(
                ErrorCode::TokenExpired,
                "Claude Code's login has expired. Open Claude Code once to refresh it.",
            ));
        }
    }
    let plan = oauth.get("subscriptionType").and_then(Value::as_str).map(str::to_string);

    let request = client
        .get(CLAUDE_USAGE_URL)
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header(reqwest::header::USER_AGENT, CLAUDE_USER_AGENT);
    let body = send(request, "Open Claude Code once to refresh its login.").await?;
    claude::parse(&body, plan)
}

async fn codex_subscription(client: &Client, p: &ProviderConfig) -> Result<Usage, ProviderError> {
    let creds = read_credentials(p, "Codex", "codex login")?;
    let tokens = creds.get("tokens").filter(|t| t.is_object()).ok_or_else(|| {
        ProviderError::new(
            ErrorCode::NotConfigured,
            "Codex is signed in with an API key. Plan limits need a ChatGPT login (`codex login`); for API spend add an OpenAI API provider instead.",
        )
    })?;
    let token = tokens.get("access_token").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::new(ErrorCode::CredentialsMissing, "The Codex auth file has no access token. Run `codex login`.")
    })?;

    if jwt_expiry(token).is_some_and(|exp| exp <= now()) {
        return Err(ProviderError::new(
            ErrorCode::TokenExpired,
            "Codex's login has expired. Run `codex` once to refresh it.",
        ));
    }

    let mut request =
        client.get(CODEX_USAGE_URL).bearer_auth(token).header(reqwest::header::USER_AGENT, CODEX_USER_AGENT);
    if let Some(account) = tokens.get("account_id").and_then(Value::as_str) {
        request = request.header("ChatGPT-Account-Id", account);
    }
    let body = send(request, "Run `codex` once to refresh its login.").await?;
    codex::parse(&body, now())
}

async fn anthropic_api(client: &Client, p: &ProviderConfig) -> Result<Usage, ProviderError> {
    let key = api_key(p, "an Anthropic Admin API key (sk-ant-admin…)")?;
    let (month_start, next_month) = time::month_bounds(now());
    let starting_at = time::format_rfc3339(month_start);

    let mut spent = 0.0;
    let mut page: Option<String> = None;
    for _ in 0..MAX_COST_PAGES {
        let mut query =
            vec![("starting_at", starting_at.clone()), ("bucket_width", "1d".into()), ("limit", "31".into())];
        if let Some(cursor) = page.take() {
            query.push(("page", cursor));
        }
        let request = client
            .get(ANTHROPIC_COST_URL)
            .query(&query)
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01");
        let body = send(request, "Check that the key is an Admin API key.").await?;
        spent += cost::anthropic_page_usd(&body)?;
        page = cost::next_page(&body);
        if page.is_none() {
            break;
        }
    }
    Ok(Usage { plan: None, meters: vec![cost::month_meter(spent, p.monthly_budget, next_month)] })
}

async fn openai_api(client: &Client, p: &ProviderConfig) -> Result<Usage, ProviderError> {
    let key = api_key(p, "an OpenAI Admin API key (sk-admin-…)")?;
    let (month_start, next_month) = time::month_bounds(now());

    let mut spent = 0.0;
    let mut page: Option<String> = None;
    for _ in 0..MAX_COST_PAGES {
        let mut query =
            vec![("start_time", month_start.to_string()), ("bucket_width", "1d".into()), ("limit", "31".into())];
        if let Some(cursor) = page.take() {
            query.push(("page", cursor));
        }
        let request = client.get(OPENAI_COST_URL).query(&query).bearer_auth(&key);
        let body = send(request, "Check that the key is an Admin API key.").await?;
        spent += cost::openai_page_usd(&body)?;
        page = cost::next_page(&body);
        if page.is_none() {
            break;
        }
    }
    Ok(Usage { plan: None, meters: vec![cost::month_meter(spent, p.monthly_budget, next_month)] })
}

/// The login file a subscription provider reads: the Settings override or the CLI's default.
pub fn credentials_path(p: &ProviderConfig) -> Option<PathBuf> {
    p.credentials_path.as_ref().map(PathBuf::from).or_else(|| default_credentials_path(p.kind))
}

fn read_credentials(p: &ProviderConfig, cli: &str, login_cmd: &str) -> Result<Value, ProviderError> {
    let path = credentials_path(p)
        .ok_or_else(|| ProviderError::new(ErrorCode::CredentialsMissing, "Could not locate your home directory."))?;

    let text = std::fs::read_to_string(&path).map_err(|e| {
        ProviderError::new(
            ErrorCode::CredentialsMissing,
            format!(
                "{cli} login not found at {} ({e}). Run `{login_cmd}` or set the path in Settings.",
                path.display()
            ),
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        ProviderError::new(ErrorCode::CredentialsMissing, format!("{} is not valid JSON: {e}", path.display()))
    })
}

/// `exp` claim of a JWT, without verifying it: only used to skip requests that would fail anyway.
fn jwt_expiry(token: &str) -> Option<i64> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice::<Value>(&bytes).ok()?.get("exp")?.as_i64()
}

fn api_key(p: &ProviderConfig, what: &str) -> Result<String, ProviderError> {
    match secrets::get(&p.id) {
        Ok(Some(key)) if !key.trim().is_empty() => Ok(key.trim().to_string()),
        Ok(_) => Err(ProviderError::new(ErrorCode::NotConfigured, format!("Add {what} in Settings."))),
        Err(e) => Err(ProviderError::new(ErrorCode::NotConfigured, format!("Could not read the saved key: {e}"))),
    }
}

async fn send(request: RequestBuilder, auth_hint: &str) -> Result<Value, ProviderError> {
    let response = request
        .send()
        .await
        .map_err(|e| ProviderError::new(ErrorCode::Network, format!("Request failed: {}", without_url(e))))?;

    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.ok().map(|t| error_detail(&t)).unwrap_or_default();
        let code = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorCode::Unauthorized,
            StatusCode::TOO_MANY_REQUESTS => ErrorCode::RateLimited,
            _ => ErrorCode::BadResponse,
        };
        let hint = if code == ErrorCode::Unauthorized { format!(" {auth_hint}") } else { String::new() };
        return Err(ProviderError::new(code, format!("HTTP {}{detail}.{hint}", status.as_u16())));
    }

    response
        .json::<Value>()
        .await
        .map_err(|e| ProviderError::bad_response(format!("Response was not JSON: {}", without_url(e))))
}

/// Pulls a short message out of an error body without echoing large or unexpected payloads.
fn error_detail(body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v.pointer("/error/message")
            .or_else(|| v.get("message"))
            .or_else(|| v.get("detail"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    match message {
        Some(m) => format!(": {}", m.chars().take(160).collect::<String>()),
        None => String::new(),
    }
}

/// reqwest errors include the URL, which for cost endpoints carries query params; keep messages short.
fn without_url(e: reqwest::Error) -> String {
    let text = e.without_url().to_string();
    text.chars().take(200).collect()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
