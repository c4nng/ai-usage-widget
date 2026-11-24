//! Parser for `GET https://api.anthropic.com/api/oauth/usage`, the endpoint
//! Claude Code's `/usage` screen reads. It is undocumented; observed shape:
//!
//! ```json
//! {
//!   "five_hour":   { "utilization": 3.0,  "resets_at": "2026-09-23T15:40:00+00:00", ... },
//!   "seven_day":   { "utilization": 13.0, "resets_at": "2026-09-27T04:00:00+00:00", ... },
//!   "seven_day_opus": null,
//!   "nimbus_quill": { "utilization": 0.0, "resets_at": null, ... },
//!   "extra_usage": { "is_enabled": false, "utilization": null, ... },
//!   "limits": [
//!     { "kind": "session",       "group": "session", "percent": 3,  "resets_at": "...", "scope": null },
//!     { "kind": "weekly_all",    "group": "weekly",  "percent": 13, "resets_at": "...", "scope": null },
//!     { "kind": "weekly_scoped", "group": "weekly",  "percent": 0,  "resets_at": "...",
//!       "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null } }
//!   ]
//! }
//! ```
//!
//! `limits` is what the Claude apps render, so it wins when present. Older
//! responses only have the `five_hour` / `seven_day*` objects, which are the
//! fallback. Other top-level objects with a `utilization` use internal code
//! names and are ignored.

use serde_json::{Map, Value};

use crate::model::{Meter, ProviderError, Usage};
use crate::time::parse_any;

pub fn parse(body: &Value, plan: Option<String>) -> Result<Usage, ProviderError> {
    let obj = body.as_object().ok_or_else(|| ProviderError::bad_response("usage response is not a JSON object"))?;

    let mut meters = obj.get("limits").and_then(Value::as_array).map(|l| from_limits(l)).unwrap_or_default();
    if meters.is_empty() {
        meters = from_windows(obj);
    }
    if let Some(extra) = obj.get("extra_usage").and_then(window_percent) {
        meters.push(Meter::window("extra_usage", "Extra usage", extra.0, extra.1));
    }

    if meters.is_empty() {
        return Err(ProviderError::bad_response("no usage windows found in response"));
    }
    Ok(Usage { plan, meters })
}

fn from_limits(limits: &[Value]) -> Vec<Meter> {
    let mut meters: Vec<(u32, Meter)> = Vec::new();
    for entry in limits.iter().filter_map(Value::as_object) {
        let Some(percent) = number(entry, &["percent", "utilization", "used_percent"]) else { continue };
        let kind = string(entry, &["kind", "id", "key"]).unwrap_or_default();
        let group = string(entry, &["group"]).unwrap_or_default();
        let scope = scope_name(entry.get("scope"));

        let (rank, base, label) = match (kind.as_str(), group.as_str()) {
            ("session", _) | (_, "session") => (0, "five_hour".to_string(), "5-hour session".to_string()),
            ("weekly_all", _) => (1, "weekly".into(), "Weekly · all models".into()),
            (_, "weekly") => (2, "weekly".into(), "Weekly".into()),
            _ => (3, kind.clone(), title_case(if kind.is_empty() { "limit" } else { &kind })),
        };
        // Scoped limits share the base key; the UI translates the part before `:`.
        let meter = match &scope {
            Some(name) => Meter::window(format!("{base}:{name}"), format!("{label} · {name}"), percent, reset(entry)),
            None => Meter::window(base, label, percent, reset(entry)),
        };
        if !meters.iter().any(|(_, m)| m.key == meter.key) {
            meters.push((rank, meter));
        }
    }
    meters.sort_by_key(|(rank, _)| *rank);
    meters.into_iter().map(|(_, m)| m).collect()
}

fn from_windows(obj: &Map<String, Value>) -> Vec<Meter> {
    let mut meters: Vec<(u32, Meter)> = Vec::new();
    for (name, value) in obj {
        let (rank, key, label) = match name.as_str() {
            "five_hour" => (0, "five_hour".to_string(), "5-hour session".to_string()),
            "seven_day" => (1, "weekly".into(), "Weekly · all models".into()),
            "seven_day_opus" => (2, "weekly:Opus".into(), "Weekly · Opus".into()),
            "seven_day_sonnet" => (2, "weekly:Sonnet".into(), "Weekly · Sonnet".into()),
            _ => continue,
        };
        if let Some((percent, resets_at)) = window_percent(value) {
            meters.push((rank, Meter::window(key, label, percent, resets_at)));
        }
    }
    meters.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.key.cmp(&b.1.key)));
    meters.into_iter().map(|(_, m)| m).collect()
}

/// `{ "utilization": 12.0, "resets_at": ... }` → (percent, reset), skipping disabled or empty windows.
fn window_percent(value: &Value) -> Option<(f64, Option<i64>)> {
    let obj = value.as_object()?;
    if obj.get("is_enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    Some((number(obj, &["utilization", "percent"])?, reset(obj)))
}

/// `{ "model": { "display_name": "Fable" }, "surface": ... }` → "Fable".
fn scope_name(scope: Option<&Value>) -> Option<String> {
    let scope = scope?.as_object()?;
    ["model", "surface"].iter().find_map(|k| {
        let part = scope.get(*k)?;
        part.as_str().map(str::to_string).or_else(|| string(part.as_object()?, &["display_name", "name", "id"]))
    })
}

fn title_case(s: &str) -> String {
    s.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn reset(obj: &Map<String, Value>) -> Option<i64> {
    ["resets_at", "reset_at", "resetsAt"].iter().find_map(|k| obj.get(*k).and_then(parse_any))
}

fn number(obj: &Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| obj.get(*k).and_then(Value::as_f64))
}

fn string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| obj.get(*k).and_then(Value::as_str).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Shape recorded from a live Max account in September 2026 (numbers only).
    fn current_response() -> Value {
        json!({
            "five_hour": { "utilization": 3.0, "resets_at": "2026-09-23T15:39:59.524721+00:00", "limit_dollars": null },
            "seven_day": { "utilization": 13.0, "resets_at": "2026-09-27T04:00:00.008819+00:00" },
            "seven_day_opus": null,
            "nimbus_quill": { "utilization": 0.0, "resets_at": null },
            "extra_usage": { "is_enabled": false, "utilization": null },
            "limits": [
                { "kind": "session", "group": "session", "percent": 3, "severity": "normal",
                  "resets_at": "2026-09-23T15:40:00.008799+00:00", "scope": null, "is_active": false },
                { "kind": "weekly_all", "group": "weekly", "percent": 13, "severity": "normal",
                  "resets_at": "2026-09-27T04:00:00.008819+00:00", "scope": null, "is_active": true },
                { "kind": "weekly_scoped", "group": "weekly", "percent": 0, "severity": "normal",
                  "resets_at": "2026-09-27T04:00:00+00:00",
                  "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null }, "is_active": false }
            ],
            "seven_day_breakdown": { "rows": [ { "key": "claude_code", "percent": 100 } ] }
        })
    }

    #[test]
    fn prefers_limits_array_and_ignores_code_names() {
        let usage = parse(&current_response(), Some("max".into())).unwrap();
        let keys: Vec<_> = usage.meters.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["five_hour", "weekly", "weekly:Fable"]);
        assert_eq!(usage.meters[1].percent, Some(13.0));
        assert_eq!(usage.meters[2].label, "Weekly · Fable");
        assert_eq!(usage.meters[0].resets_at, crate::time::parse_rfc3339("2026-09-23T15:40:00Z"));
        assert_eq!(usage.plan.as_deref(), Some("max"));
    }

    #[test]
    fn falls_back_to_legacy_windows() {
        let body = json!({
            "seven_day": { "utilization": 18.0, "resets_at": "2025-09-27T08:00:00.000000+00:00" },
            "five_hour": { "utilization": 42.5, "resets_at": "2025-09-23T15:00:00+00:00" },
            "seven_day_opus": null,
            "seven_day_sonnet": { "utilization": 3.0, "resets_at": null },
            "some_codename": { "utilization": 99.0 },
            "extra_usage": { "is_enabled": false, "utilization": null }
        });
        let usage = parse(&body, None).unwrap();
        let keys: Vec<_> = usage.meters.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["five_hour", "weekly", "weekly:Sonnet"]);
        assert_eq!(usage.meters[0].resets_at, Some(1_758_639_600));
        assert_eq!(usage.meters[2].resets_at, None);
    }

    #[test]
    fn shows_extra_usage_only_when_enabled() {
        let mut body = current_response();
        body["extra_usage"] = json!({ "is_enabled": true, "utilization": 40.0 });
        let usage = parse(&body, None).unwrap();
        assert_eq!(usage.meters.last().unwrap().key, "extra_usage");
    }

    #[test]
    fn keeps_unknown_limit_kinds() {
        let body = json!({ "limits": [ { "kind": "daily_burst", "group": "burst", "percent": 55.5, "scope": { "surface": "Claude Code" } } ] });
        let usage = parse(&body, None).unwrap();
        assert_eq!(usage.meters[0].key, "daily_burst:Claude Code");
        assert_eq!(usage.meters[0].label, "Daily Burst · Claude Code");
    }

    #[test]
    fn clamps_out_of_range_utilization() {
        let body = json!({ "five_hour": { "utilization": 130.0 } });
        assert_eq!(parse(&body, None).unwrap().meters[0].percent, Some(100.0));
    }

    #[test]
    fn rejects_responses_without_windows() {
        assert!(parse(&json!({ "error": "nope" }), None).is_err());
        assert!(parse(&json!({ "limits": [] }), None).is_err());
        assert!(parse(&json!([]), None).is_err());
    }
}
