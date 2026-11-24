//! Parser for `GET https://chatgpt.com/backend-api/wham/usage`, the endpoint the
//! Codex CLI reads its plan limits from. It is undocumented; observed shape:
//!
//! ```json
//! {
//!   "plan_type": "plus",
//!   "rate_limit": {
//!     "primary_window":   { "used_percent": 74, "limit_window_seconds": 18000,  "reset_after_seconds": 6198, "reset_at": 1789801452 },
//!     "secondary_window": { "used_percent": 12, "limit_window_seconds": 604800, "reset_at": 1790200000 }
//!   },
//!   "additional_rate_limits": [ { "limit_name": "GPT-5-Codex-Mini", "rate_limit": { ... } } ]
//! }
//! ```
//!
//! Field names have drifted between snake_case and camelCase and between
//! `primary_window` and `primary`, so every lookup accepts the known variants.

use serde_json::{Map, Value};

use crate::model::{Meter, ProviderError, Usage};
use crate::time::parse_any;

pub fn parse(body: &Value, now: i64) -> Result<Usage, ProviderError> {
    let obj = body.as_object().ok_or_else(|| ProviderError::bad_response("usage response is not a JSON object"))?;

    let plan = get(obj, &["plan_type", "planType"]).and_then(Value::as_str).map(str::to_string);

    let mut meters = Vec::new();
    if let Some(limits) = get(obj, &["rate_limit", "rate_limits", "rateLimit"]).and_then(Value::as_object) {
        meters.extend(windows(limits, None, now));
    }

    if let Some(extra) = get(obj, &["additional_rate_limits", "additionalRateLimits"]).and_then(Value::as_array) {
        for entry in extra.iter().filter_map(Value::as_object) {
            let name = get(entry, &["limit_name", "limitName", "metered_feature", "name"])
                .and_then(Value::as_str)
                .unwrap_or("Other");
            if let Some(limits) = get(entry, &["rate_limit", "rateLimit"]).and_then(Value::as_object) {
                meters.extend(windows(limits, Some(name), now));
            }
        }
    }

    if meters.is_empty() {
        return Err(ProviderError::bad_response("no rate-limit windows found in response"));
    }
    Ok(Usage { plan, meters })
}

fn windows(limits: &Map<String, Value>, scope: Option<&str>, now: i64) -> Vec<Meter> {
    let slots = [
        ("primary", &["primary_window", "primaryWindow", "primary"][..]),
        ("secondary", &["secondary_window", "secondaryWindow", "secondary"][..]),
    ];
    let mut out = Vec::new();
    for (slot, names) in slots {
        let Some(w) = get(limits, names).and_then(Value::as_object) else { continue };
        let Some(percent) = get(w, &["used_percent", "usedPercent"]).and_then(Value::as_f64) else { continue };

        let resets_at = get(w, &["reset_at", "resets_at", "resetAt"]).and_then(parse_any).or_else(|| {
            get(w, &["reset_after_seconds", "resetAfterSeconds", "resets_in_seconds"])
                .and_then(Value::as_f64)
                .map(|s| now + s as i64)
        });

        let window_secs =
            get(w, &["limit_window_seconds", "limitWindowSeconds", "window_seconds"]).and_then(Value::as_i64);
        let (key, label) = describe(slot, window_secs);
        let meter = match scope {
            None => Meter::window(key, label, percent, resets_at),
            Some(name) => Meter::window(format!("{key}:{name}"), format!("{label} · {name}"), percent, resets_at),
        };
        out.push(meter);
    }
    out
}

fn describe(slot: &str, window_secs: Option<i64>) -> (String, String) {
    match window_secs {
        Some(18_000) => ("five_hour".into(), "5-hour session".into()),
        Some(604_800) => ("weekly".into(), "Weekly".into()),
        Some(s) if s > 0 && s % 86_400 == 0 => (format!("window_{s}"), format!("{}-day window", s / 86_400)),
        Some(s) if s > 0 && s % 3_600 == 0 => (format!("window_{s}"), format!("{}-hour window", s / 3_600)),
        _ if slot == "primary" => ("primary".into(), "Primary limit".into()),
        _ => ("secondary".into(), "Secondary limit".into()),
    }
}

fn get<'a>(obj: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| obj.get(*k).filter(|v| !v.is_null()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_789_795_254;

    #[test]
    fn parses_snake_case_windows() {
        let body = json!({
            "plan_type": "plus",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": { "used_percent": 74, "limit_window_seconds": 18000, "reset_after_seconds": 6198, "reset_at": 1_789_801_452 },
                "secondary_window": { "used_percent": 12.5, "limit_window_seconds": 604800, "reset_after_seconds": 100 }
            },
            "credits": null
        });
        let usage = parse(&body, NOW).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.meters.len(), 2);
        assert_eq!(usage.meters[0].key, "five_hour");
        assert_eq!(usage.meters[0].percent, Some(74.0));
        assert_eq!(usage.meters[0].resets_at, Some(1_789_801_452));
        assert_eq!(usage.meters[1].key, "weekly");
        assert_eq!(usage.meters[1].resets_at, Some(NOW + 100));
    }

    #[test]
    fn parses_camel_case_and_short_slot_names() {
        let body = json!({
            "planType": "pro",
            "rateLimit": {
                "primary": { "usedPercent": 5, "limitWindowSeconds": 18000, "resetAt": "2026-09-23T15:00:00Z" },
                "secondary": null
            }
        });
        let usage = parse(&body, NOW).unwrap();
        assert_eq!(usage.meters.len(), 1);
        assert_eq!(usage.meters[0].resets_at, crate::time::parse_rfc3339("2026-09-23T15:00:00Z"));
    }

    #[test]
    fn includes_additional_limits_and_unknown_windows() {
        let body = json!({
            "rate_limit": { "primary_window": { "used_percent": 1 } },
            "additional_rate_limits": [
                { "limit_name": "Codex-Mini", "rate_limit": { "primary_window": { "used_percent": 40, "limit_window_seconds": 7200 } } }
            ]
        });
        let usage = parse(&body, NOW).unwrap();
        assert_eq!(usage.meters[0].key, "primary");
        assert_eq!(usage.meters[1].key, "window_7200:Codex-Mini");
        assert_eq!(usage.meters[1].label, "2-hour window · Codex-Mini");
    }

    #[test]
    fn rejects_responses_without_windows() {
        assert!(parse(&json!({ "plan_type": "free" }), NOW).is_err());
    }
}
