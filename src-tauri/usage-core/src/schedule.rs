//! When to fetch each provider again. The goal is to touch the network only
//! when the numbers can have changed, and to back off from a service that is
//! pushing back instead of hammering it.

use crate::model::{ErrorCode, ProviderKind, ProviderReport};

/// Spend reports are aggregated by the providers well behind real time, so
/// polling them more often than this only costs requests.
pub const MIN_COST_INTERVAL: i64 = 30 * 60;
/// Upper bound for any back-off.
pub const MAX_BACKOFF: i64 = 60 * 60;
/// Quick retry after a network blip, doubled per consecutive failure.
const NETWORK_RETRY: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Next {
    /// Seconds until the next fetch.
    pub delay: i64,
    /// Refetch as soon as the provider's login file changes, even before `delay`.
    pub watch_credentials: bool,
}

/// Regular interval for a provider when things are fine.
pub fn interval(kind: ProviderKind, refresh_minutes: u32) -> i64 {
    let base = i64::from(refresh_minutes.max(1)) * 60;
    match kind {
        ProviderKind::AnthropicApi | ProviderKind::OpenaiApi => base.max(MIN_COST_INTERVAL),
        ProviderKind::ClaudeSubscription | ProviderKind::CodexSubscription => base,
    }
}

/// `failures` counts consecutive failed fetches including this one (0 on success).
pub fn next(kind: ProviderKind, refresh_minutes: u32, error: Option<ErrorCode>, failures: u32) -> Next {
    let regular = interval(kind, refresh_minutes);
    let doubling = |base: i64| base.saturating_mul(1 << failures.saturating_sub(1).min(10));
    let (delay, watch_credentials) = match error {
        None => (regular, false),
        Some(ErrorCode::RateLimited) => (doubling(regular.max(5 * 60)).min(MAX_BACKOFF), false),
        Some(ErrorCode::Network | ErrorCode::BadResponse) => (doubling(NETWORK_RETRY).min(regular), false),
        // Nothing will change until the user signs in again or edits Settings;
        // a slow hourly check is a safety net, the file watch does the real work.
        Some(
            ErrorCode::TokenExpired
            | ErrorCode::CredentialsMissing
            | ErrorCode::Unauthorized
            | ErrorCode::NotConfigured,
        ) => (MAX_BACKOFF, true),
    };
    Next { delay, watch_credentials }
}

/// Keeps the last good numbers visible through a temporary failure.
pub fn merge(previous: Option<&ProviderReport>, mut fresh: ProviderReport) -> ProviderReport {
    let transient = matches!(
        fresh.error.as_ref().map(|e| e.code),
        Some(ErrorCode::RateLimited | ErrorCode::Network | ErrorCode::BadResponse)
    );
    if transient && fresh.usage.is_none() {
        if let Some(usage) = previous.and_then(|p| p.usage.clone()) {
            fresh.usage = Some(usage);
            fresh.stale = true;
        }
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Meter, ProviderError, Usage};

    #[test]
    fn cost_providers_poll_at_most_every_half_hour() {
        assert_eq!(interval(ProviderKind::OpenaiApi, 5), MIN_COST_INTERVAL);
        assert_eq!(interval(ProviderKind::AnthropicApi, 60), 3600);
        assert_eq!(interval(ProviderKind::ClaudeSubscription, 5), 300);
    }

    #[test]
    fn rate_limits_back_off_exponentially_up_to_an_hour() {
        let delays: Vec<_> =
            (1..=6).map(|n| next(ProviderKind::ClaudeSubscription, 5, Some(ErrorCode::RateLimited), n).delay).collect();
        assert_eq!(delays, [300, 600, 1200, 2400, 3600, 3600]);
    }

    #[test]
    fn network_errors_retry_fast_but_never_slower_than_normal() {
        let delays: Vec<_> =
            (1..=5).map(|n| next(ProviderKind::CodexSubscription, 5, Some(ErrorCode::Network), n).delay).collect();
        assert_eq!(delays, [60, 120, 240, 300, 300]);
    }

    #[test]
    fn login_problems_wait_for_the_credentials_file() {
        let n = next(ProviderKind::ClaudeSubscription, 5, Some(ErrorCode::TokenExpired), 1);
        assert!(n.watch_credentials);
        assert_eq!(n.delay, MAX_BACKOFF);
        assert!(!next(ProviderKind::ClaudeSubscription, 5, None, 0).watch_credentials);
    }

    fn report(usage: Option<Usage>, code: Option<ErrorCode>) -> ProviderReport {
        ProviderReport {
            id: "p".into(),
            label: "P".into(),
            kind: ProviderKind::ClaudeSubscription,
            fetched_at: 0,
            usage,
            error: code.map(|c| ProviderError::new(c, "x")),
            stale: false,
        }
    }

    #[test]
    fn merge_keeps_last_numbers_only_for_transient_errors() {
        let good = report(Some(Usage { plan: None, meters: vec![Meter::window("five_hour", "5h", 10.0, None)] }), None);

        let merged = merge(Some(&good), report(None, Some(ErrorCode::RateLimited)));
        assert!(merged.stale);
        assert_eq!(merged.usage, good.usage);

        let expired = merge(Some(&good), report(None, Some(ErrorCode::TokenExpired)));
        assert!(!expired.stale);
        assert!(expired.usage.is_none());

        assert!(!merge(None, report(None, Some(ErrorCode::Network))).stale);
    }
}
