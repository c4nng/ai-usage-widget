//! Month-to-date spend from the providers' Admin cost APIs.
//!
//! * Anthropic `GET /v1/organizations/cost_report`: `data[].results[].amount` is a
//!   decimal string in the lowest currency unit (cents for USD).
//! * OpenAI `GET /v1/organization/costs`: `data[].results[].amount.value` is a
//!   number in dollars.
//!
//! Both paginate with `has_more` / `next_page`.

use serde_json::Value;

use crate::model::{Meter, ProviderError};

/// Sum of one page of an Anthropic cost report, in USD.
pub fn anthropic_page_usd(body: &Value) -> Result<f64, ProviderError> {
    let mut cents = 0.0;
    for result in results(body)? {
        if let Some(currency) = result.get("currency").and_then(Value::as_str) {
            if !currency.eq_ignore_ascii_case("usd") {
                continue;
            }
        }
        cents += match result.get("amount") {
            Some(Value::String(s)) => {
                s.parse::<f64>().map_err(|_| ProviderError::bad_response(format!("invalid amount {s:?}")))?
            }
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            _ => 0.0,
        };
    }
    Ok(cents / 100.0)
}

/// Sum of one page of an OpenAI costs response, in USD.
pub fn openai_page_usd(body: &Value) -> Result<f64, ProviderError> {
    let mut usd = 0.0;
    for result in results(body)? {
        let Some(amount) = result.get("amount") else { continue };
        if let Some(currency) = amount.get("currency").and_then(Value::as_str) {
            if !currency.eq_ignore_ascii_case("usd") {
                continue;
            }
        }
        usd += match amount.get("value") {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::String(s)) => s.parse::<f64>().unwrap_or(0.0),
            _ => 0.0,
        };
    }
    Ok(usd)
}

/// Cursor for the next page, if the response says there is one.
pub fn next_page(body: &Value) -> Option<String> {
    if body.get("has_more").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    body.get("next_page").and_then(Value::as_str).map(str::to_string)
}

pub fn month_meter(spent_usd: f64, budget_usd: Option<f64>, resets_at: i64) -> Meter {
    let budget = budget_usd.filter(|b| *b > 0.0);
    Meter {
        key: "month_cost".into(),
        label: "This month".into(),
        percent: budget.map(|b| (spent_usd / b * 100.0).clamp(0.0, 100.0)),
        resets_at: Some(resets_at),
        spent_usd: Some(spent_usd),
        budget_usd: budget,
    }
}

fn results(body: &Value) -> Result<impl Iterator<Item = &Value>, ProviderError> {
    let buckets = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::bad_response("cost response has no `data` array"))?;
    Ok(buckets.iter().filter_map(|b| b.get("results").and_then(Value::as_array)).flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sums_anthropic_cents() {
        let body = json!({
            "data": [
                { "starting_at": "2025-08-01T00:00:00Z", "ending_at": "2025-08-02T00:00:00Z",
                  "results": [ { "amount": "123.45", "currency": "USD" }, { "amount": "76.55", "currency": "USD" } ] },
                { "starting_at": "2025-08-02T00:00:00Z", "ending_at": "2025-08-03T00:00:00Z", "results": [] }
            ],
            "has_more": true,
            "next_page": "page_abc"
        });
        assert!((anthropic_page_usd(&body).unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(next_page(&body).as_deref(), Some("page_abc"));
    }

    #[test]
    fn sums_openai_dollars() {
        let body = json!({
            "object": "page",
            "data": [
                { "object": "bucket", "start_time": 1730419200, "end_time": 1730505600,
                  "results": [ { "object": "organization.costs.result", "amount": { "value": 0.06, "currency": "usd" } } ] },
                { "object": "bucket", "results": [ { "amount": { "value": 1.5, "currency": "usd" } } ] }
            ],
            "has_more": false,
            "next_page": null
        });
        assert!((openai_page_usd(&body).unwrap() - 1.56).abs() < 1e-9);
        assert_eq!(next_page(&body), None);
    }

    #[test]
    fn rejects_missing_data() {
        assert!(anthropic_page_usd(&json!({ "error": {} })).is_err());
        assert!(openai_page_usd(&json!({})).is_err());
    }

    #[test]
    fn month_meter_percent_needs_a_budget() {
        let m = month_meter(12.5, Some(50.0), 0);
        assert_eq!(m.percent, Some(25.0));
        assert_eq!(month_meter(12.5, None, 0).percent, None);
        assert_eq!(month_meter(12.5, Some(0.0), 0).budget_usd, None);
        assert_eq!(month_meter(80.0, Some(50.0), 0).percent, Some(100.0));
    }
}
