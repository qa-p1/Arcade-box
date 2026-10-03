//! Colour conversion between HEX, RGB, HSL, HSV, and CMYK, plus the WCAG 2
//! contrast ratio against a second colour.

use crate::tool_kit::option_str;
use arcade_contract::ToolRequest;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

pub(super) fn convert(request: &ToolRequest, text: &str) -> Result<Value, String> {
    let color = parse(text)?;
    let against_text = match option_str(request, "against", "").trim() {
        "" => "#ffffff",
        value => value,
    };
    let against = parse(against_text).map_err(|error| format!("Contrast colour: {error}"))?;
    let ratio = contrast(color, against);
    let grade = |minimum: f64| if ratio >= minimum { "Pass" } else { "Fail" };
    let (h, s, l) = to_hsl(color);
    let (hv, sv, v) = to_hsv(color);
    let (c, m, y, k) = to_cmyk(color);
    let readable_on = if contrast(color, Rgb { r: 0, g: 0, b: 0 })
        >= contrast(
            color,
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
        ) {
        "black"
    } else {
        "white"
    };
    Ok(json!({
        "hex": hex(color),
        "rgb": format!("rgb({}, {}, {})", color.r, color.g, color.b),
        "hsl": format!("hsl({h}, {s}%, {l}%)"),
        "hsv": format!("hsv({hv}, {sv}%, {v}%)"),
        "cmyk": format!("cmyk({c}%, {m}%, {y}%, {k}%)"),
        "name": NAMED.iter().find(|(_, value)| parse_hex(value) == Some(color)).map(|(name, _)| *name),
        "bestTextColor": readable_on,
        "contrast": {
            "against": hex(against),
            "ratio": format!("{:.2}:1", (ratio * 100.0).floor() / 100.0),
            "normalTextAA": grade(4.5),
            "normalTextAAA": grade(7.0),
            "largeTextAA": grade(3.0),
            "largeTextAAA": grade(4.5),
            "uiComponents": grade(3.0),
        },
    }))
}

fn parse(text: &str) -> Result<Rgb, String> {
    let value = text.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err(
            "Enter a colour such as #ff8800, rgb(255, 136, 0), hsl(32, 100%, 50%), or orange"
                .into(),
        );
    }
    if let Some(color) = parse_hex(&value) {
        return Ok(color);
    }
    if let Some((_, hex_value)) = NAMED.iter().find(|(name, _)| *name == value) {
        return Ok(parse_hex(hex_value).expect("named colours are valid"));
    }
    let (function, body) = match value.split_once('(') {
        Some((name, rest)) => (name.trim(), rest.trim_end_matches(')')),
        None => ("rgb", value.as_str()),
    };
    let parts = body
        .split([',', ' ', '/'])
        .map(|part| part.trim().trim_end_matches('%').trim_end_matches("deg"))
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<f64>()
                .map_err(|_| format!("`{part}` is not a number"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() < 3 || parts.len() > 4 {
        return Err(format!("Could not read `{text}` as a colour"));
    }
    let (a, b, c) = (parts[0], parts[1], parts[2]);
    match function {
        "rgb" | "rgba" => {
            let channel = |v: f64| {
                if (0.0..=255.0).contains(&v) {
                    Ok(v.round() as u8)
                } else {
                    Err("RGB values must be 0 to 255".to_owned())
                }
            };
            Ok(Rgb {
                r: channel(a)?,
                g: channel(b)?,
                b: channel(c)?,
            })
        }
        "hsl" | "hsla" => check_hs(b, c).map(|_| from_hsl(a, b / 100.0, c / 100.0)),
        "hsv" | "hsb" => check_hs(b, c).map(|_| from_hsv(a, b / 100.0, c / 100.0)),
        other => Err(format!("Unsupported colour format `{other}`")),
    }
}

fn check_hs(s: f64, l: f64) -> Result<(), String> {
    if (0.0..=100.0).contains(&s) && (0.0..=100.0).contains(&l) {
        Ok(())
    } else {
        Err("Saturation and lightness must be 0 to 100%".into())
    }
}

fn parse_hex(value: &str) -> Option<Rgb> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = match digits.len() {
        3 | 4 => digits
            .chars()
            .take(3)
            .flat_map(|c| [c, c])
            .collect::<String>(),
        6 | 8 => digits[..6].to_owned(),
        _ => return None,
    };
    let channel = |index: usize| u8::from_str_radix(&expanded[index..index + 2], 16).ok();
    Some(Rgb {
        r: channel(0)?,
        g: channel(2)?,
        b: channel(4)?,
    })
}

fn hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

fn unit(color: Rgb) -> (f64, f64, f64) {
    (
        f64::from(color.r) / 255.0,
        f64::from(color.g) / 255.0,
        f64::from(color.b) / 255.0,
    )
}

fn hue(r: f64, g: f64, b: f64, max: f64, delta: f64) -> f64 {
    if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    }
}

fn to_hsl(color: Rgb) -> (i64, i64, i64) {
    let (r, g, b) = unit(color);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    let l = (max + min) / 2.0;
    let s = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * l - 1.0).abs())
    };
    (
        hue(r, g, b, max, delta).round() as i64 % 360,
        (s * 100.0).round() as i64,
        (l * 100.0).round() as i64,
    )
}

fn to_hsv(color: Rgb) -> (i64, i64, i64) {
    let (r, g, b) = unit(color);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    let s = if max == 0.0 { 0.0 } else { delta / max };
    (
        hue(r, g, b, max, delta).round() as i64 % 360,
        (s * 100.0).round() as i64,
        (max * 100.0).round() as i64,
    )
}

fn to_cmyk(color: Rgb) -> (i64, i64, i64, i64) {
    let (r, g, b) = unit(color);
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 {
        return (0, 0, 0, 100);
    }
    let part = |v: f64| ((1.0 - v - k) / (1.0 - k) * 100.0).round() as i64;
    (part(r), part(g), part(b), (k * 100.0).round() as i64)
}

fn from_chroma(h: f64, c: f64, m: f64) -> Rgb {
    let h = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let channel = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb {
        r: channel(r),
        g: channel(g),
        b: channel(b),
    }
}

fn from_hsl(h: f64, s: f64, l: f64) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    from_chroma(h, c, l - c / 2.0)
}

fn from_hsv(h: f64, s: f64, v: f64) -> Rgb {
    let c = v * s;
    from_chroma(h, c, v - c)
}

fn luminance(color: Rgb) -> f64 {
    let linear = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = unit(color);
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// CSS named colours.
const NAMED: &[(&str, &str)] = &[
    ("aliceblue", "f0f8ff"),
    ("antiquewhite", "faebd7"),
    ("aqua", "00ffff"),
    ("aquamarine", "7fffd4"),
    ("azure", "f0ffff"),
    ("beige", "f5f5dc"),
    ("bisque", "ffe4c4"),
    ("black", "000000"),
    ("blanchedalmond", "ffebcd"),
    ("blue", "0000ff"),
    ("blueviolet", "8a2be2"),
    ("brown", "a52a2a"),
    ("burlywood", "deb887"),
    ("cadetblue", "5f9ea0"),
    ("chartreuse", "7fff00"),
    ("chocolate", "d2691e"),
    ("coral", "ff7f50"),
    ("cornflowerblue", "6495ed"),
    ("cornsilk", "fff8dc"),
    ("crimson", "dc143c"),
    ("darkblue", "00008b"),
    ("darkcyan", "008b8b"),
    ("darkgoldenrod", "b8860b"),
    ("darkgray", "a9a9a9"),
    ("darkgreen", "006400"),
    ("darkkhaki", "bdb76b"),
    ("darkmagenta", "8b008b"),
    ("darkolivegreen", "556b2f"),
    ("darkorange", "ff8c00"),
    ("darkorchid", "9932cc"),
    ("darkred", "8b0000"),
    ("darksalmon", "e9967a"),
    ("darkseagreen", "8fbc8f"),
    ("darkslateblue", "483d8b"),
    ("darkslategray", "2f4f4f"),
    ("darkturquoise", "00ced1"),
    ("darkviolet", "9400d3"),
    ("deeppink", "ff1493"),
    ("deepskyblue", "00bfff"),
    ("dimgray", "696969"),
    ("dodgerblue", "1e90ff"),
    ("firebrick", "b22222"),
    ("floralwhite", "fffaf0"),
    ("forestgreen", "228b22"),
    ("gainsboro", "dcdcdc"),
    ("ghostwhite", "f8f8ff"),
    ("gold", "ffd700"),
    ("goldenrod", "daa520"),
    ("gray", "808080"),
    ("green", "008000"),
    ("greenyellow", "adff2f"),
    ("honeydew", "f0fff0"),
    ("hotpink", "ff69b4"),
    ("indianred", "cd5c5c"),
    ("indigo", "4b0082"),
    ("ivory", "fffff0"),
    ("khaki", "f0e68c"),
    ("lavender", "e6e6fa"),
    ("lavenderblush", "fff0f5"),
    ("lawngreen", "7cfc00"),
    ("lemonchiffon", "fffacd"),
    ("lightblue", "add8e6"),
    ("lightcoral", "f08080"),
    ("lightcyan", "e0ffff"),
    ("lightgoldenrodyellow", "fafad2"),
    ("lightgray", "d3d3d3"),
    ("lightgreen", "90ee90"),
    ("lightpink", "ffb6c1"),
    ("lightsalmon", "ffa07a"),
    ("lightseagreen", "20b2aa"),
    ("lightskyblue", "87cefa"),
    ("lightslategray", "778899"),
    ("lightsteelblue", "b0c4de"),
    ("lightyellow", "ffffe0"),
    ("lime", "00ff00"),
    ("limegreen", "32cd32"),
    ("linen", "faf0e6"),
    ("magenta", "ff00ff"),
    ("maroon", "800000"),
    ("mediumaquamarine", "66cdaa"),
    ("mediumblue", "0000cd"),
    ("mediumorchid", "ba55d3"),
    ("mediumpurple", "9370db"),
    ("mediumseagreen", "3cb371"),
    ("mediumslateblue", "7b68ee"),
    ("mediumspringgreen", "00fa9a"),
    ("mediumturquoise", "48d1cc"),
    ("mediumvioletred", "c71585"),
    ("midnightblue", "191970"),
    ("mintcream", "f5fffa"),
    ("mistyrose", "ffe4e1"),
    ("moccasin", "ffe4b5"),
    ("navajowhite", "ffdead"),
    ("navy", "000080"),
    ("oldlace", "fdf5e6"),
    ("olive", "808000"),
    ("olivedrab", "6b8e23"),
    ("orange", "ffa500"),
    ("orangered", "ff4500"),
    ("orchid", "da70d6"),
    ("palegoldenrod", "eee8aa"),
    ("palegreen", "98fb98"),
    ("paleturquoise", "afeeee"),
    ("palevioletred", "db7093"),
    ("papayawhip", "ffefd5"),
    ("peachpuff", "ffdab9"),
    ("peru", "cd853f"),
    ("pink", "ffc0cb"),
    ("plum", "dda0dd"),
    ("powderblue", "b0e0e6"),
    ("purple", "800080"),
    ("rebeccapurple", "663399"),
    ("red", "ff0000"),
    ("rosybrown", "bc8f8f"),
    ("royalblue", "4169e1"),
    ("saddlebrown", "8b4513"),
    ("salmon", "fa8072"),
    ("sandybrown", "f4a460"),
    ("seagreen", "2e8b57"),
    ("seashell", "fff5ee"),
    ("sienna", "a0522d"),
    ("silver", "c0c0c0"),
    ("skyblue", "87ceeb"),
    ("slateblue", "6a5acd"),
    ("slategray", "708090"),
    ("snow", "fffafa"),
    ("springgreen", "00ff7f"),
    ("steelblue", "4682b4"),
    ("tan", "d2b48c"),
    ("teal", "008080"),
    ("thistle", "d8bfd8"),
    ("tomato", "ff6347"),
    ("turquoise", "40e0d0"),
    ("violet", "ee82ee"),
    ("wheat", "f5deb3"),
    ("white", "ffffff"),
    ("whitesmoke", "f5f5f5"),
    ("yellow", "ffff00"),
    ("yellowgreen", "9acd32"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_round_trip_and_contrast_matches_wcag() {
        let orange = Rgb {
            r: 255,
            g: 136,
            b: 0,
        };
        assert_eq!(parse("#ff8800").unwrap(), orange);
        assert_eq!(parse("#f80").unwrap(), orange);
        assert_eq!(parse("rgb(255 136 0)").unwrap(), orange);
        assert_eq!(parse("255, 136, 0").unwrap(), orange);
        assert_eq!(
            parse(&format!("hsl({}, {}%, {}%)", 32, 100, 50)).unwrap(),
            orange
        );
        assert_eq!(
            parse("RebeccaPurple").unwrap(),
            Rgb {
                r: 0x66,
                g: 0x33,
                b: 0x99
            }
        );
        assert!(
            (contrast(
                Rgb { r: 0, g: 0, b: 0 },
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255
                }
            ) - 21.0)
                .abs()
                < 1e-9
        );
        let request = ToolRequest {
            tool_id: "t".into(),
            inputs: vec![],
            options: json!({"against": "#777777"}),
        };
        let report = convert(&request, "white").unwrap();
        assert_eq!(report["contrast"]["ratio"], "4.47:1");
        assert_eq!(report["contrast"]["normalTextAA"], "Fail");
        assert_eq!(report["name"], "white");
    }
}
