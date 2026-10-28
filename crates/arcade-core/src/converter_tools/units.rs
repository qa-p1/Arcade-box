//! Local unit conversion across common physical and data-size dimensions.

use super::format_significant;
use crate::tool_kit::{option_f64, option_str};
use arcade_contract::ToolRequest;
use serde_json::{Value, json};

/// Convert `amount from to`, typed as text ("14 GB to MB", "12 in to cm",
/// "5 ft 11 in to cm", "72F in C") or set through the Amount / From / To
/// controls.
pub(super) fn convert(request: &ToolRequest, text: &str) -> Result<Value, String> {
    let tokens = tokenize(text);
    // The connector is the last `to`/`in`/`->`/`=` whose right side is a unit,
    // so "in" still works as the inch unit ("12 in to cm").
    let split = (1..tokens.len()).rev().find(|&index| {
        is_connector(&tokens[index])
            && index + 1 < tokens.len()
            && unit_spec(&tokens[index + 1..].join(" ")).is_some()
    });
    let (left, typed_target) = match split {
        Some(index) => (&tokens[..index], Some(tokens[index + 1..].join(" "))),
        None => (&tokens[..], None),
    };
    let control_from = option_str(request, "from", "").trim().to_owned();
    let to = typed_target.unwrap_or_else(|| option_str(request, "to", "").trim().to_owned());
    // Typed text wins over the controls, which act as defaults.
    let parts = amount_parts(left, &control_from, option_f64(request, "amount"))?;
    if parts.is_empty() || to.is_empty() {
        return Err("Provide source and destination units, for example `14 GB to MB`".into());
    }
    let target = unit_spec(&to).ok_or_else(|| format!("Unknown unit `{to}`"))?;
    let mut base = 0.0;
    for (value, unit, spec) in &parts {
        if spec.dimension != target.dimension {
            return Err(format!(
                "Cannot convert {unit} to {to}; they measure different things ({} and {})",
                spec.dimension, target.dimension
            ));
        }
        if spec.dimension == "temperature" && parts.len() > 1 {
            return Err(
                "Temperatures cannot be added together; convert one value at a time".into(),
            );
        }
        base += value * spec.scale + spec.offset;
    }
    let converted = (base - target.offset) / target.scale;
    if !converted.is_finite() {
        return Err("Conversion is outside the supported numeric range".into());
    }
    let described = parts
        .iter()
        .map(|(value, unit, _)| format!("{} {unit}", format_significant(*value)))
        .collect::<Vec<_>>()
        .join(" ");
    let (amount, from) = if parts.len() == 1 {
        (parts[0].0, parts[0].1.clone())
    } else {
        // A compound amount ("5 ft 11 in") is reported in its smallest unit.
        let (_, unit, spec) = parts.last().expect("non-empty");
        ((base - spec.offset) / spec.scale, unit.clone())
    };
    Ok(json!({
        "input": amount,
        "from": from,
        "value": converted,
        "to": to,
        "dimension": target.dimension,
        "formatted": format_significant(converted),
        "headline": format!("{described} = {} {to}", format_significant(converted)),
    }))
}

fn is_connector(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "to" | "in" | "into" | "as" | "->" | "→" | "="
    )
}

/// Split text into words, separating a number glued to its unit ("72F",
/// "2.5kWh", "5'11\"") into two tokens.
fn tokenize(text: &str) -> Vec<String> {
    let text = text.replace('′', "'").replace('″', "\"");
    let mut tokens = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_end_matches([',', '?', '.']).to_owned();
        let mut rest = word.as_str();
        while !rest.is_empty() {
            let number_end = rest
                .char_indices()
                .take_while(|(index, character)| {
                    character.is_ascii_digit()
                        || matches!(character, '.' | ',')
                        || (*index > 0 && *character == '/')
                        || (*index == 0 && matches!(character, '-' | '+'))
                })
                .map(|(index, character)| index + character.len_utf8())
                .last()
                .unwrap_or(0);
            if number_end > 0
                && number_end < rest.len()
                && rest[..number_end].chars().any(|c| c.is_ascii_digit())
            {
                tokens.push(rest[..number_end].to_owned());
                rest = &rest[number_end..];
                // Feet/inch marks are units of their own: 5'11"
                if let Some(mark) = rest.strip_prefix('\'') {
                    tokens.push("ft".into());
                    rest = mark;
                } else if let Some(mark) = rest.strip_prefix('"') {
                    tokens.push("in".into());
                    rest = mark;
                } else {
                    tokens.push(rest.to_owned());
                    rest = "";
                }
            } else {
                tokens.push(rest.to_owned());
                rest = "";
            }
        }
    }
    tokens
}

/// `1.5`, `1,024` or a fraction such as `1/3`.
fn parse_amount(token: &str) -> Option<f64> {
    let token = token.replace(',', "");
    let value = match token.split_once('/') {
        Some((numerator, denominator)) => {
            let denominator: f64 = denominator.parse().ok().filter(|d: &f64| *d != 0.0)?;
            numerator.parse::<f64>().ok()? / denominator
        }
        None => token.parse().ok()?,
    };
    Some(value).filter(|value| value.is_finite())
}

/// Read `number unit [number unit]…`; a bare number takes the From control.
fn amount_parts(
    tokens: &[String],
    control_from: &str,
    control_amount: Option<f64>,
) -> Result<Vec<(f64, String, UnitSpec)>, String> {
    let mut parts = Vec::new();
    let mut index = 0;
    if tokens.is_empty() {
        if let Some(amount) = control_amount {
            if let Some(spec) = unit_spec(control_from) {
                parts.push((amount, control_from.to_owned(), spec));
            }
        }
        return if parts.is_empty() {
            Err("Enter a numeric amount".into())
        } else {
            Ok(parts)
        };
    }
    while index < tokens.len() {
        let value = parse_amount(&tokens[index]).ok_or("Enter a numeric amount")?;
        index += 1;
        // Units can be several words ("fl oz", "square feet"); take the longest match.
        let longest = (index..tokens.len())
            .rev()
            .find(|&end| unit_spec(&tokens[index..=end].join(" ")).is_some());
        let unit = match longest {
            Some(end) => {
                let unit = tokens[index..=end].join(" ");
                index = end + 1;
                unit
            }
            None if index < tokens.len() => {
                return Err(format!("Unknown unit `{}`", tokens[index]));
            }
            None if !control_from.is_empty() => control_from.to_owned(),
            None => {
                return Err(
                    "Provide source and destination units, for example `14 GB to MB`".into(),
                );
            }
        };
        let spec = unit_spec(&unit).ok_or_else(|| format!("Unknown unit `{unit}`"))?;
        parts.push((value, unit, spec));
    }
    Ok(parts)
}

struct UnitSpec {
    dimension: &'static str,
    scale: f64,
    offset: f64,
}

/// SI prefixes: symbol, spelled name, factor.
const PREFIXES: &[(&str, &str, f64)] = &[
    ("p", "pico", 1e-12),
    ("n", "nano", 1e-9),
    ("µ", "micro", 1e-6),
    ("u", "micro", 1e-6),
    ("m", "milli", 1e-3),
    ("c", "centi", 1e-2),
    ("h", "hecto", 1e2),
    ("k", "kilo", 1e3),
    ("M", "mega", 1e6),
    ("G", "giga", 1e9),
    ("T", "tera", 1e12),
    ("P", "peta", 1e15),
];
/// Binary prefixes, for data only.
const BINARY: &[(&str, &str, f64)] = &[
    ("Ki", "kibi", 1024.0),
    ("Mi", "mebi", 1_048_576.0),
    ("Gi", "gibi", 1_073_741_824.0),
    ("Ti", "tebi", 1_099_511_627_776.0),
    ("Pi", "pebi", 1_125_899_906_842_624.0),
];
/// Which prefixes a unit takes: everyday SI ones, `c` and `h` where they
/// are used (cm, cL, hPa), and only multiples for data (`mb` stays megabyte).
#[derive(Clone, Copy)]
enum Takes {
    Si,
    SiWithCenti,
    SiWithHecto,
    Data,
    /// `b` for bits only after an upper-case multiple (`Mb`, `Gb`): a bare
    /// `b` and the everyday `kb` keep meaning bytes.
    Bits,
}
/// SI units: symbol, spelled names, dimension, scale to the base unit, prefixes.
/// Longer symbols come first so `kWh` isn't read as `kW` + `h`.
const SI_UNITS: &[(&str, &[&str], &str, f64, Takes)] = &[
    ("bit", &["bit"], "data", 0.125, Takes::Data),
    (
        "Wh",
        &["watt hour", "watt-hour", "watthour"],
        "energy",
        3600.0,
        Takes::Si,
    ),
    ("Pa", &["pascal"], "pressure", 1.0, Takes::SiWithHecto),
    ("m", &["meter", "metre"], "length", 1.0, Takes::SiWithCenti),
    ("g", &["gram"], "mass", 0.001, Takes::Si),
    ("J", &["joule"], "energy", 1.0, Takes::Si),
    ("W", &["watt"], "power", 1.0, Takes::Si),
    (
        "L",
        &["liter", "litre"],
        "volume",
        0.001,
        Takes::SiWithCenti,
    ),
    ("l", &[], "volume", 0.001, Takes::SiWithCenti),
    ("B", &["byte"], "data", 1.0, Takes::Data),
    ("b", &[], "data", 0.125, Takes::Bits),
];

fn prefix_factor(prefix: &str, takes: Takes, spelled: bool) -> Option<f64> {
    if prefix.is_empty() {
        return (!matches!(takes, Takes::Bits)).then_some(1.0);
    }
    let name = |entry: &(&'static str, &'static str, f64)| if spelled { entry.1 } else { entry.0 };
    let allowed = |symbol: &str| match takes {
        Takes::Si => !matches!(symbol, "c" | "h"),
        Takes::SiWithCenti => symbol != "h",
        Takes::SiWithHecto => symbol != "c",
        Takes::Data => matches!(symbol, "k" | "M" | "G" | "T" | "P"),
        Takes::Bits => matches!(symbol, "M" | "G" | "T" | "P"),
    };
    PREFIXES
        .iter()
        .filter(|entry| allowed(entry.0))
        .chain(
            BINARY
                .iter()
                .filter(|_| matches!(takes, Takes::Data | Takes::Bits)),
        )
        .find(|entry| name(entry) == prefix)
        .map(|entry| entry.2)
}

/// `mW`, `MB`, `Mb`, `KiB`, `kWh`: case matters, because SI prefixes and
/// bit/byte collide once lowercased (B2).
fn si_symbol(raw: &str) -> Option<UnitSpec> {
    SI_UNITS
        .iter()
        .find_map(|&(symbol, _, dimension, scale, takes)| {
            let factor = prefix_factor(raw.strip_suffix(symbol)?, takes, false)?;
            Some(UnitSpec {
                dimension,
                scale: scale * factor,
                offset: 0.0,
            })
        })
}

/// `megawatt`, `Kilobytes`, `kilowatt hours`: spelled out, in any case.
fn si_name(raw: &str) -> Option<UnitSpec> {
    let lower = raw.to_lowercase();
    let singular = lower.strip_suffix('s').unwrap_or(&lower);
    SI_UNITS
        .iter()
        .find_map(|&(_, names, dimension, scale, takes)| {
            names.iter().find_map(|name| {
                let factor = prefix_factor(singular.strip_suffix(name)?, takes, true)?;
                Some(UnitSpec {
                    dimension,
                    scale: scale * factor,
                    offset: 0.0,
                })
            })
        })
}

fn unit_spec(raw: &str) -> Option<UnitSpec> {
    let raw = raw.trim();
    if let Some(spec) = si_symbol(raw).or_else(|| si_name(raw)) {
        return Some(spec);
    }
    let unit = raw
        .trim()
        .to_ascii_lowercase()
        .replace('²', "2")
        .replace('³', "3")
        .replace("square ", "sq ")
        .replace("cubic ", "cu ");
    let item = match unit.as_str() {
        "m" | "meter" | "meters" => ("length", 1.0, 0.0),
        "km" | "kilometer" | "kilometers" => ("length", 1000.0, 0.0),
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => ("length", 0.01, 0.0),
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => {
            ("length", 0.001, 0.0)
        }
        "metre" | "metres" => ("length", 1.0, 0.0),
        "kilometre" | "kilometres" => ("length", 1000.0, 0.0),
        "nmi" | "nautical mile" | "nautical miles" => ("length", 1852.0, 0.0),
        "'" => ("length", 0.3048, 0.0),
        "\"" => ("length", 0.0254, 0.0),
        "mi" | "mile" | "miles" => ("length", 1609.344, 0.0),
        "ft" | "foot" | "feet" => ("length", 0.3048, 0.0),
        "in" | "inch" | "inches" => ("length", 0.0254, 0.0),
        "yd" | "yard" | "yards" => ("length", 0.9144, 0.0),
        "kg" | "kilogram" | "kilograms" => ("mass", 1.0, 0.0),
        "g" | "gram" | "grams" => ("mass", 0.001, 0.0),
        "mg" | "milligram" | "milligrams" => ("mass", 0.000001, 0.0),
        "st" | "stone" | "stones" => ("mass", 6.35029318, 0.0),
        "lb" | "lbs" | "pound" | "pounds" => ("mass", 0.45359237, 0.0),
        "oz" | "ounce" | "ounces" => ("mass", 0.028349523125, 0.0),
        "ton" | "tonne" | "tonnes" => ("mass", 1000.0, 0.0),
        "c" | "°c" | "degc" | "celsius" => ("temperature", 1.0, 273.15),
        "f" | "°f" | "degf" | "fahrenheit" => ("temperature", 5.0 / 9.0, 255.3722222222222),
        "k" | "kelvin" => ("temperature", 1.0, 0.0),
        "m2" | "sqm" | "sq m" | "sq meter" | "sq meters" | "sq metre" | "sq metres" => {
            ("area", 1.0, 0.0)
        }
        "km2" => ("area", 1_000_000.0, 0.0),
        "cm2" => ("area", 0.0001, 0.0),
        "ft2" | "sqft" | "sq ft" | "sq foot" | "sq feet" => ("area", 0.09290304, 0.0),
        "in2" | "sq in" | "sq inch" | "sq inches" => ("area", 0.00064516, 0.0),
        "mi2" | "sq mi" | "sq mile" | "sq miles" => ("area", 2_589_988.110336, 0.0),
        "acre" | "acres" => ("area", 4046.8564224, 0.0),
        "ha" | "hectare" => ("area", 10000.0, 0.0),
        "l" | "liter" | "litre" | "liters" | "litres" => ("volume", 0.001, 0.0),
        "ml" | "milliliter" | "milliliters" | "millilitre" | "millilitres" => {
            ("volume", 0.000001, 0.0)
        }
        "m3" => ("volume", 1.0, 0.0),
        "gal" | "gallon" | "gallons" => ("volume", 0.003785411784, 0.0),
        "qt" | "quart" | "quarts" => ("volume", 0.000946352946, 0.0),
        "cup" | "cups" => ("volume", 0.0002365882365, 0.0),
        "floz" | "fl oz" | "fluid ounce" | "fluid ounces" => ("volume", 0.0000295735295625, 0.0),
        "m/s" | "mps" => ("speed", 1.0, 0.0),
        "km/h" | "kph" | "kmh" => ("speed", 1.0 / 3.6, 0.0),
        "mph" => ("speed", 0.44704, 0.0),
        "kn" | "knot" | "knots" => ("speed", 0.514444444444, 0.0),
        "pa" => ("pressure", 1.0, 0.0),
        "kpa" => ("pressure", 1000.0, 0.0),
        "bar" => ("pressure", 100000.0, 0.0),
        "psi" => ("pressure", 6894.757293, 0.0),
        "atm" => ("pressure", 101325.0, 0.0),
        "j" | "joule" | "joules" => ("energy", 1.0, 0.0),
        "mj" => ("energy", 1_000_000.0, 0.0),
        "btu" => ("energy", 1055.05585262, 0.0),
        "kj" => ("energy", 1000.0, 0.0),
        "cal" => ("energy", 4.184, 0.0),
        "kcal" => ("energy", 4184.0, 0.0),
        "wh" => ("energy", 3600.0, 0.0),
        "kwh" => ("energy", 3_600_000.0, 0.0),
        "w" | "watt" | "watts" => ("power", 1.0, 0.0),
        "kw" => ("power", 1000.0, 0.0),
        "mw" | "megawatt" | "megawatts" => ("power", 1_000_000.0, 0.0),
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

    fn run(text: &str) -> Value {
        let request: ToolRequest = serde_json::from_value(json!({
            "toolId": "arcade.convert.units", "toolVersion": "1.0.0", "inputs": [], "options": {}
        }))
        .unwrap();
        convert(&request, text).unwrap()
    }

    fn value(text: &str) -> f64 {
        run(text)["value"].as_f64().unwrap()
    }

    fn close(text: &str, expected: f64) {
        let actual = value(text);
        assert!(
            (actual - expected).abs() <= expected.abs() * 1e-12,
            "{text}: {actual} != {expected}"
        );
    }

    #[test]
    fn unit_symbols_keep_their_case() {
        // B2: milli/mega and bit/byte used to collide once lowercased.
        close("1 mW to W", 0.001);
        close("1 MW to kW", 1000.0);
        close("1 MB to Mb", 8.0);
        close("1 MiB to KiB", 1024.0);
        close("1 megawatt to kW", 1000.0);
        close("1 megabyte to megabit", 8.0);
        close("1 Gb to MB", 125.0);
        close("1 kbit to B", 125.0);
        close("1 kb to B", 1000.0);
        close("100 b to bit", 800.0);
        close("1 GiB to MiB", 1024.0);
    }

    #[test]
    fn every_si_unit_takes_every_prefix() {
        // B4 asked for MJ; these all come from one prefix table now.
        close("1 GJ to MJ", 1000.0);
        close("1 GWh to MWh", 1000.0);
        close("1 kilowatt hour to megajoules", 3.6);
        close("1 µm to nm", 1000.0);
        close("1 um to nm", 1000.0);
        close("1 hPa to Pa", 100.0);
        close("2 MPa to kPa", 2000.0);
        close("250 mL to L", 0.25);
        close("1 cL to mL", 10.0);
        close("1 Mg to kg", 1000.0);
        close("3 milliwatts to W", 0.003);
        close("1 Kilobytes to bytes", 1000.0);
        // Units outside SI are unchanged.
        close("1 lb to kg", 0.45359237);
        close("1 kcal to kJ", 4.184);
        close("1 atm to kPa", 101.325);
    }

    #[test]
    fn in_is_an_inch_unless_it_joins_the_units() {
        // B3
        close("1 in to cm", 2.54);
        close("12 in in cm", 30.48);
        close("72F in C", 200.0 / 9.0);
    }

    #[test]
    fn compound_amounts_and_si_energy_prefixes() {
        // B4
        close("5 ft 11 in to cm", 180.34);
        close("5'11\" to cm", 180.34);
        close("1 MJ to kJ", 1000.0);
        close("1 kWh to MJ", 3.6);
    }

    #[test]
    fn shown_results_keep_the_number() {
        // B6: the text rounded to six significant digits, and to 0.
        assert_eq!(
            run("1000000.5 m to m")["headline"],
            "1000000.5 m = 1000000.5 m"
        );
        assert_eq!(run("0.000000001 m to km")["formatted"], "1e-12");
        assert_eq!(run("1/3 km to m")["formatted"], "333.333333333");
        assert_eq!(run("12 in in cm")["formatted"], "30.48");
        assert_eq!(value("1000000.5 m to m"), 1000000.5);
    }

    #[test]
    fn typed_conversions_keep_inches_and_compound_amounts() {
        assert!((run("12 in to cm")["value"].as_f64().unwrap() - 30.48).abs() < 1e-9);
        assert!((run("5 ft 11 in to cm")["value"].as_f64().unwrap() - 180.34).abs() < 1e-9);
        assert!((run("5'11\" to cm")["value"].as_f64().unwrap() - 180.34).abs() < 1e-9);
        assert!((run("2.5 kWh to MJ")["value"].as_f64().unwrap() - 9.0).abs() < 1e-9);
        assert!((run("72F in C")["value"].as_f64().unwrap() - 22.2222222).abs() < 1e-6);
        assert!((run("100 cm in inches")["value"].as_f64().unwrap() - 39.3700787).abs() < 1e-6);
        assert!((run("14 GB to MB")["value"].as_f64().unwrap() - 14000.0).abs() < 1e-9);
        assert!((run("1 sq ft to m2")["value"].as_f64().unwrap() - 0.09290304).abs() < 1e-12);
    }

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
