//! Calculators and converters. Everything runs locally except the currency
//! converter, which fetches indicative rates.

mod calculator;
mod color;
mod currency;
mod datetime;
mod finance;
mod spreadsheet;
mod units;

use crate::{
    Arcade,
    tool_kit::{check_cancelled, json_result, single_value},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use serde_json::Value;
use std::sync::atomic::AtomicBool;

const INPUT_LIMIT: usize = 128 * 1024;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    if manifest.id == "arcade.convert.excel-csv" {
        return spreadsheet::convert(manifest, request, runtime, cancelled);
    }
    let text = single_value(request, INPUT_LIMIT)?;
    let (value, mime): (Value, _) = match manifest.id.as_str() {
        "arcade.convert.calculator" => (calculator::evaluate(text)?, "structured/calculation"),
        "arcade.convert.percentage" => (finance::percentage(request)?, "structured/calculation"),
        "arcade.convert.loan" => (finance::loan(request)?, "structured/calculation"),
        "arcade.convert.units" => (units::convert(request, text)?, "structured/quantity"),
        "arcade.convert.currency" => (
            currency::convert(request, text, cancelled)?,
            "structured/currency-amount",
        ),
        "arcade.convert.time-zone" => (datetime::time_zones(request, text)?, "structured/datetime"),
        "arcade.convert.date-duration" => (
            datetime::date_calculator(request, text)?,
            "structured/datetime",
        ),
        "arcade.convert.color" => (color::convert(request, text)?, "structured/color"),
        _ => {
            return Err(format!(
                "No converter executor is registered for {}",
                manifest.id
            ));
        }
    };
    let mut result = json_result(manifest, value, mime);
    if manifest.id == "arcade.convert.currency" {
        result
            .warnings
            .push("Rates are indicative and not suitable for settlement or trading.".into());
    }
    Ok(result)
}

/// A number without trailing zeros or float noise, such as `1.5` or `1024`.
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value:.10}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

/// A money amount rounded to cents with thousands separators, such as `12,345.60`.
fn format_money(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let text = if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.2}")
    };
    let (sign, digits) = text
        .strip_prefix('-')
        .map_or(("", text.as_str()), |rest| ("-", rest));
    let (whole, fraction) = digits
        .split_once('.')
        .map_or((digits, None), |(w, f)| (w, Some(f)));
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match fraction {
        Some(fraction) => format!("{sign}{grouped}.{fraction}"),
        None => format!("{sign}{grouped}"),
    }
}
