use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Claude Pro/Max plan limits, read with the Claude Code login.
    ClaudeSubscription,
    /// ChatGPT Plus/Pro/Team plan limits, read with the Codex CLI login.
    CodexSubscription,
    /// Month-to-date spend from the Anthropic Admin API.
    AnthropicApi,
    /// Month-to-date spend from the OpenAI Admin API.
    OpenaiApi,
}

/// One bar in the widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meter {
    /// Stable key the UI translates, e.g. `five_hour`, `weekly`, `month_cost`.
    pub key: String,
    /// English fallback label for keys the UI doesn't know.
    pub label: String,
    /// 0–100. `None` when there is nothing to compare against (spend without a budget).
    pub percent: Option<f64>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
    pub spent_usd: Option<f64>,
    pub budget_usd: Option<f64>,
}

impl Meter {
    pub fn window(key: impl Into<String>, label: impl Into<String>, percent: f64, resets_at: Option<i64>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            percent: Some(percent.clamp(0.0, 100.0)),
            resets_at,
            spent_usd: None,
            budget_usd: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub plan: Option<String>,
    pub meters: Vec<Meter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The provider needs setup (e.g. no API key saved).
    NotConfigured,
    /// The CLI credentials file doesn't exist or can't be read.
    CredentialsMissing,
    /// The CLI's access token is past its expiry; running the CLI refreshes it.
    TokenExpired,
    Unauthorized,
    RateLimited,
    Network,
    BadResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderError {
    pub code: ErrorCode,
    pub message: String,
}

impl ProviderError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn bad_response(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadResponse, message)
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReport {
    pub id: String,
    pub label: String,
    pub kind: ProviderKind,
    /// Unix seconds.
    pub fetched_at: i64,
    pub usage: Option<Usage>,
    pub error: Option<ProviderError>,
    /// `usage` is from an earlier successful fetch because the latest one failed.
    #[serde(default)]
    pub stale: bool,
}
