//! Currency conversion with indicative rates from open.er-api.com, cached for
//! six hours per base currency. This is the only converter that uses the network.

use super::format_number;
use crate::{
    network,
    process::{self, ProcessSpec},
    tool_kit::{check_cancelled, option_f64, option_str},
};
use arcade_contract::ToolRequest;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, atomic::AtomicBool},
    time::{Duration, Instant},
};

/// Convert `amount FROM TO`, typed as text ("100 usd to inr") or set through the
/// Amount / From / To controls.
pub(super) fn convert(
    request: &ToolRequest,
    text: &str,
    cancelled: &AtomicBool,
) -> Result<Value, String> {
    let pieces = text
        .split_whitespace()
        .filter(|item| !item.eq_ignore_ascii_case("to") && !item.eq_ignore_ascii_case("in"))
        .collect::<Vec<_>>();
    let amount = pieces
        .first()
        .and_then(|value| value.replace(',', "").parse().ok())
        .or_else(|| option_f64(request, "amount"))
        .ok_or("Enter an amount to convert")?;
    let from = currency_code(
        pieces
            .get(1)
            .copied()
            .unwrap_or(option_str(request, "from", "")),
    )?;
    let to = currency_code(
        pieces
            .get(2)
            .copied()
            .unwrap_or(option_str(request, "to", "")),
    )?;
    let snapshot = get_rates(&from, cancelled)?;
    let rate = snapshot
        .rates
        .get(&to)
        .copied()
        .ok_or_else(|| format!("The rate provider did not return a rate for {to}"))?;
    let converted = amount * rate;
    if !converted.is_finite() {
        return Err("Converted amount is outside the supported numeric range".into());
    }
    Ok(json!({
        "headline": format!("{} {from} = {} {to}", money(amount), money(converted)),
        "amount": amount,
        "from": from,
        "convertedAmount": converted,
        "to": to,
        "rate": format!("1 {from} = {} {to}", format_number(rate)),
        "provider": "open.er-api.com",
        "rateUpdated": snapshot.updated,
    }))
}

use super::format_money as money;

#[derive(Clone)]
struct RateSnapshot {
    rates: HashMap<String, f64>,
    updated: String,
    fetched: Instant,
}
static RATE_CACHE: OnceLock<Mutex<HashMap<String, RateSnapshot>>> = OnceLock::new();

fn currency_code(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(
            "Currency codes must be three-letter ISO-style codes, such as USD or EUR".into(),
        );
    }
    Ok(value)
}

fn get_rates(base: &str, cancelled: &AtomicBool) -> Result<RateSnapshot, String> {
    let cache = RATE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(snapshot) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(base)
        .cloned()
        && snapshot.fetched.elapsed() < Duration::from_secs(6 * 60 * 60)
    {
        return Ok(snapshot);
    }
    check_cancelled(cancelled)?;
    let url = format!("https://open.er-api.com/v6/latest/{base}");
    let curl = network::curl_provider()?;
    let mut args = network::curl_common_args();
    args.extend([
        "--fail".into(),
        "--location".into(),
        "--max-redirs".into(),
        "3".into(),
        "--max-time".into(),
        "20".into(),
        "--max-filesize".into(),
        "1048576".into(),
        "--".into(),
        url.into(),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: curl.executable_path,
            args,
            current_dir: None,
            timeout: Duration::from_secs(25),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Currency rate request failed: {error}"))?;
    if !output.status.success() {
        return Err(network::process_error(
            &output.stderr,
            "Currency rate provider returned an error",
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Currency rate provider returned invalid JSON: {error}"))?;
    if value.get("result").and_then(Value::as_str) != Some("success") {
        return Err("Currency rate provider did not return a successful rate set".into());
    }
    let updated = value
        .get("time_last_update_utc")
        .and_then(Value::as_str)
        .unwrap_or("Provider timestamp unavailable")
        .to_owned();
    let rates = value
        .get("rates")
        .and_then(Value::as_object)
        .ok_or("Currency rate response has no rates")?
        .iter()
        .filter_map(|(code, value)| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| (code.to_ascii_uppercase(), value))
        })
        .collect::<HashMap<_, _>>();
    if rates.is_empty() {
        return Err("Currency rate response contained no usable rates".into());
    }
    let snapshot = RateSnapshot {
        rates,
        updated,
        fetched: Instant::now(),
    };
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(base.to_owned(), snapshot.clone());
    Ok(snapshot)
}
