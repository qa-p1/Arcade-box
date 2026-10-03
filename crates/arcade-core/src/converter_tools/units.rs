//! Local unit conversion across common physical and data-size dimensions.

use super::format_number;
use crate::tool_kit::{option_f64, option_str};
use arcade_contract::ToolRequest;
use serde_json::{Value, json};

/// Convert `amount from to`, typed as text ("14 GB to MB") or set through the
/// Amount / From / To controls.
pub(super) fn convert(request: &ToolRequest, text: &str) -> Result<Value, String> {
    let pieces = text
        .split_whitespace()
        .filter(|part| !part.eq_ignore_ascii_case("to") && !part.eq_ignore_ascii_case("in"))
        .collect::<Vec<_>>();
    // Typed text wins over the controls, which act as defaults.
    let amount = pieces
        .first()
        .and_then(|value| value.replace(',', "").parse().ok())
        .or_else(|| option_f64(request, "amount"))
        .ok_or("Enter a numeric amount")?;
    let from = pieces
        .get(1)
        .copied()
        .unwrap_or_else(|| option_str(request, "from", "").trim());
    let to = pieces
        .get(2)
        .copied()
        .unwrap_or_else(|| option_str(request, "to", "").trim());
    if from.is_empty() || to.is_empty() {
        return Err("Provide source and destination units, for example `14 GB to MB`".into());
    }
    let source = unit_spec(from).ok_or_else(|| format!("Unknown unit `{from}`"))?;
    let target = unit_spec(to).ok_or_else(|| format!("Unknown unit `{to}`"))?;
    if source.dimension != target.dimension {
        return Err(format!(
            "Cannot convert {from} to {to}; they measure different things ({} and {})",
            source.dimension, target.dimension
        ));
    }
    let converted = (amount * source.scale + source.offset - target.offset) / target.scale;
    if !converted.is_finite() {
        return Err("Conversion is outside the supported numeric range".into());
    }
    Ok(json!({
        "input": amount,
        "from": from,
        "value": converted,
        "to": to,
        "dimension": source.dimension,
        "formatted": format_number(readable(converted)),
        "headline": format!("{} {from} = {} {to}", format_number(amount), format_number(readable(converted))),
    }))
}

/// Six significant decimals, enough for everyday conversions.
fn readable(value: f64) -> f64 {
    if value == 0.0 || value.abs() >= 1e6 {
        return value.round();
    }
    let digits = 6 - value.abs().log10().floor() as i32;
    let scale = 10f64.powi(digits.max(0));
    (value * scale).round() / scale
}

struct UnitSpec {
    dimension: &'static str,
    scale: f64,
    offset: f64,
}

fn unit_spec(raw: &str) -> Option<UnitSpec> {
    let unit = raw
        .trim()
        .to_ascii_lowercase()
        .replace('²', "2")
        .replace('³', "3");
    let item = match unit.as_str() {
        "m" | "meter" | "meters" => ("length", 1.0, 0.0),
        "km" | "kilometer" | "kilometers" => ("length", 1000.0, 0.0),
        "cm" => ("length", 0.01, 0.0),
        "mm" => ("length", 0.001, 0.0),
        "mi" | "mile" | "miles" => ("length", 1609.344, 0.0),
        "ft" | "foot" | "feet" => ("length", 0.3048, 0.0),
        "in" | "inch" | "inches" => ("length", 0.0254, 0.0),
        "yd" | "yard" | "yards" => ("length", 0.9144, 0.0),
        "kg" | "kilogram" | "kilograms" => ("mass", 1.0, 0.0),
        "g" | "gram" | "grams" => ("mass", 0.001, 0.0),
        "mg" => ("mass", 0.000001, 0.0),
        "lb" | "lbs" | "pound" | "pounds" => ("mass", 0.45359237, 0.0),
        "oz" | "ounce" | "ounces" => ("mass", 0.028349523125, 0.0),
        "ton" | "tonne" | "tonnes" => ("mass", 1000.0, 0.0),
        "c" | "°c" | "celsius" => ("temperature", 1.0, 273.15),
        "f" | "°f" | "fahrenheit" => ("temperature", 5.0 / 9.0, 255.3722222222222),
        "k" | "kelvin" => ("temperature", 1.0, 0.0),
        "m2" | "sqm" => ("area", 1.0, 0.0),
        "km2" => ("area", 1_000_000.0, 0.0),
        "cm2" => ("area", 0.0001, 0.0),
        "ft2" | "sqft" => ("area", 0.09290304, 0.0),
        "acre" | "acres" => ("area", 4046.8564224, 0.0),
        "ha" | "hectare" => ("area", 10000.0, 0.0),
        "l" | "liter" | "litre" => ("volume", 0.001, 0.0),
        "ml" => ("volume", 0.000001, 0.0),
        "m3" => ("volume", 1.0, 0.0),
        "gal" | "gallon" | "gallons" => ("volume", 0.003785411784, 0.0),
        "qt" | "quart" => ("volume", 0.000946352946, 0.0),
        "cup" | "cups" => ("volume", 0.0002365882365, 0.0),
        "floz" => ("volume", 0.0000295735295625, 0.0),
        "m/s" | "mps" => ("speed", 1.0, 0.0),
        "km/h" | "kph" => ("speed", 1.0 / 3.6, 0.0),
        "mph" => ("speed", 0.44704, 0.0),
        "kn" | "knot" | "knots" => ("speed", 0.514444444444, 0.0),
        "pa" => ("pressure", 1.0, 0.0),
        "kpa" => ("pressure", 1000.0, 0.0),
        "bar" => ("pressure", 100000.0, 0.0),
        "psi" => ("pressure", 6894.757293, 0.0),
        "atm" => ("pressure", 101325.0, 0.0),
        "j" => ("energy", 1.0, 0.0),
        "kj" => ("energy", 1000.0, 0.0),
        "cal" => ("energy", 4.184, 0.0),
        "kcal" => ("energy", 4184.0, 0.0),
        "wh" => ("energy", 3600.0, 0.0),
        "kwh" => ("energy", 3_600_000.0, 0.0),
        "w" => ("power", 1.0, 0.0),
        "kw" => ("power", 1000.0, 0.0),
        "mw" => ("power", 1_000_000.0, 0.0),
        "hp" => ("power", 745.6998716, 0.0),
        "bit" | "bits" => ("data", 0.125, 0.0),
        "b" | "byte" | "bytes" => ("data", 1.0, 0.0),
        "kb" => ("data", 1000.0, 0.0),
        "mb" => ("data", 1_000_000.0, 0.0),
        "gb" => ("data", 1_000_000_000.0, 0.0),
        "tb" => ("data", 1e12, 0.0),
        "kib" => ("data", 1024.0, 0.0),
        "mib" => ("data", 1_048_576.0, 0.0),
        "gib" => ("data", 1_073_741_824.0, 0.0),
        "tib" => ("data", 1_099_511_627_776.0, 0.0),
        _ => return None,
    };
    Some(UnitSpec {
        dimension: item.0,
        scale: item.1,
        offset: item.2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions_cover_temperature_and_data() {
        let source = unit_spec("F").unwrap();
        let target = unit_spec("C").unwrap();
        assert!(
            ((32.0 * source.scale + source.offset - target.offset) / target.scale).abs() < 1e-10
        );
        assert_eq!(unit_spec("GiB").unwrap().scale, 1_073_741_824.0);
    }
}
