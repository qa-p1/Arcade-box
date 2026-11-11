//! Arithmetic and scientific expressions, evaluated by a small recursive
//! descent parser with a nesting limit.

use super::format_significant;
use serde_json::{Value, json};

const MAX_EXPRESSION_DEPTH: usize = 128;

/// Whether search should read `text` as arithmetic: digits, at least one operator or function, and a value.
pub(super) fn is_expression(text: &str) -> bool {
    text.bytes().any(|byte| byte.is_ascii_digit())
        && (text.contains(['+', '-', '*', '/', '^', '%', '!', '(', '×', '÷'])
            || text.contains(" mod "))
        && evaluate(text, false).is_ok()
}

/// Evaluates `expression`; trigonometric functions take and give degrees when `degrees` is set.
pub(super) fn evaluate(expression: &str, degrees: bool) -> Result<Value, String> {
    let mut parser = ExpressionParser::new(expression);
    parser.degrees = degrees;
    let value = parser.parse_expression()?;
    parser.skip_space();
    if parser.peek().is_some() {
        return Err(format!("Unexpected character at byte {}", parser.position));
    }
    if !value.is_finite() {
        return Err("The calculation did not produce a finite number".into());
    }
    Ok(
        json!({"expression": expression.trim(), "value": value, "formatted": format_significant(value)}),
    )
}

struct ExpressionParser<'a> {
    input: &'a str,
    position: usize,
    depth: usize,
    degrees: bool,
    /// Inside a function's argument list a comma separates arguments and is never a thousands separator.
    call_depth: usize,
    /// The factor just parsed was a number with a postfix `%` (so `100+10%` means 100 plus 10% of 100).
    bare_percent: bool,
}

impl<'a> ExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            depth: 0,
            degrees: false,
            call_depth: 0,
            bare_percent: false,
        }
    }
    /// `1,000` and `12,345,678`: a comma, exactly three digits and no digit after them, after an integer part that
    /// is one to three digits (then more groups), and outside a function's arguments.
    fn thousands_separator_here(&self, start: usize) -> bool {
        let bytes = self.input.as_bytes();
        if self.call_depth > 0 {
            return false;
        }
        let integer = &bytes[start..self.position];
        let groups = integer.split(|byte| *byte == b',').collect::<Vec<_>>();
        let first = groups[0];
        let well_formed = !first.is_empty()
            && first.len() <= 3
            && first.iter().all(u8::is_ascii_digit)
            && groups[1..]
                .iter()
                .all(|group| group.len() == 3 && group.iter().all(u8::is_ascii_digit));
        let after = &bytes[self.position + 1..];
        well_formed
            && after.len() >= 3
            && after[..3].iter().all(u8::is_ascii_digit)
            && !after.get(3).is_some_and(u8::is_ascii_digit)
    }
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }
    fn skip_space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }
    fn starts_implicit_factor(&mut self) -> bool {
        self.skip_space();
        self.peek()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'(')
    }
    fn eat(&mut self, expected: u8) -> bool {
        self.skip_space();
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    /// Whether an operand (a number, name, bracket or sign) follows, after spaces, at `offset` bytes from here.
    fn operand_follows(&self, offset: usize) -> bool {
        self.input.as_bytes()[(self.position + offset).min(self.input.len())..]
            .iter()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'(' | b'.' | b'-' | b'+')
            })
    }
    fn at_word(&mut self, word: &str) -> bool {
        self.skip_space();
        let rest = &self.input.as_bytes()[self.position..];
        rest.len() >= word.len()
            && rest[..word.len()].eq_ignore_ascii_case(word.as_bytes())
            && !rest
                .get(word.len())
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    }
    fn parse_expression(&mut self) -> Result<f64, String> {
        let mut value = self.parse_product()?;
        loop {
            let negative = if self.eat(b'+') {
                false
            } else if self.eat(b'-') {
                true
            } else {
                self.bare_percent = false;
                return Ok(value);
            };
            let mut term = self.parse_product()?;
            if self.bare_percent {
                // `100 + 10%` adds ten percent *of the left side*, not 0.1.
                term *= value;
            }
            if negative {
                value -= term;
            } else {
                value += term;
            }
        }
    }
    fn parse_product(&mut self) -> Result<f64, String> {
        let mut value = self.parse_unary()?;
        let mut bare = self.bare_percent;
        loop {
            self.skip_space();
            let doubled_star = self.input.as_bytes()[self.position..].starts_with(b"**");
            if !doubled_star && self.eat(b'*') {
                bare = false;
                value *= self.parse_unary()?;
            } else if self.eat(b'/') {
                bare = false;
                let rhs = self.parse_unary()?;
                if rhs == 0.0 {
                    return Err("Division by zero".into());
                }
                value /= rhs;
            } else if self.at_word("mod") || (self.peek() == Some(b'%') && self.operand_follows(1))
            {
                // `10 mod 3` and `10 % 3`; a `%` with nothing after it is a percentage and is read by the factor.
                bare = false;
                self.position += if self.peek() == Some(b'%') { 1 } else { 3 };
                let rhs = self.parse_unary()?;
                if rhs == 0.0 {
                    return Err("Remainder by zero".into());
                }
                value %= rhs;
            } else if self.starts_implicit_factor() {
                // `2pi`, `2(3)`, `2log10(100)`: a name or bracket right
                // after a value multiplies it. `2 3` stays an error.
                bare = false;
                value *= self.parse_unary()?;
            } else {
                self.bare_percent = bare;
                return Ok(value);
            }
        }
    }
    fn parse_unary(&mut self) -> Result<f64, String> {
        let mut negative = false;
        loop {
            if self.eat(b'+') {
                continue;
            }
            if self.eat(b'-') {
                negative = !negative;
                continue;
            }
            break;
        }
        let value = self.parse_power()?;
        Ok(if negative { -value } else { value })
    }
    fn parse_power(&mut self) -> Result<f64, String> {
        self.depth += 1;
        if self.depth > MAX_EXPRESSION_DEPTH {
            self.depth -= 1;
            return Err(format!(
                "Expression nesting exceeds the {MAX_EXPRESSION_DEPTH} level limit"
            ));
        }
        let result = (|| {
            let mut lhs = self.parse_primary()?;
            self.bare_percent = false;
            loop {
                self.skip_space();
                if self.peek() == Some(b'!')
                    && self.input.as_bytes().get(self.position + 1) != Some(&b'=')
                {
                    self.position += 1;
                    lhs = factorial(lhs)?;
                } else if self.peek() == Some(b'%') && !self.operand_follows(1) {
                    self.position += 1;
                    lhs /= 100.0;
                    self.bare_percent = true;
                } else {
                    break;
                }
            }
            let doubled_star = self.input.as_bytes()[self.position..].starts_with(b"**");
            if doubled_star {
                self.position += 2;
            }
            if doubled_star || self.eat(b'^') {
                self.bare_percent = false;
                Ok(lhs.powf(self.parse_unary()?))
            } else {
                Ok(lhs)
            }
        })();
        self.depth -= 1;
        result
    }
    fn parse_primary(&mut self) -> Result<f64, String> {
        self.skip_space();
        if self.eat(b'(') {
            let value = self.parse_expression()?;
            if !self.eat(b')') {
                return Err("Missing closing parenthesis".into());
            }
            return Ok(value);
        }
        let start = self.position;
        if self
            .peek()
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            while self.peek().is_some_and(|byte| {
                byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
                    || (byte == b',' && self.thousands_separator_here(start))
            }) {
                if matches!(self.peek(), Some(b'+') | Some(b'-'))
                    && self.position > start
                    && !matches!(self.input.as_bytes()[self.position - 1], b'e' | b'E')
                {
                    break;
                }
                self.position += 1;
            }
            return self.input[start..self.position]
                .replace(',', "")
                .parse::<f64>()
                .map_err(|_| "Enter a valid number".into());
        }
        // Names start with a letter and may contain digits after it (log10, log2).
        while self.peek().is_some_and(|byte| {
            byte.is_ascii_alphabetic()
                || byte == b'_'
                || (self.position > start && byte.is_ascii_digit())
        }) {
            self.position += 1;
        }
        if self.position == start {
            return Err(format!(
                "Expected a number or function at byte {}",
                self.position
            ));
        }
        let name = self.input[start..self.position].to_ascii_lowercase();
        if !self.eat(b'(') {
            return match name.as_str() {
                "pi" => Ok(std::f64::consts::PI),
                "e" => Ok(std::f64::consts::E),
                _ => Err(format!("Unknown constant `{name}`")),
            };
        }
        let mut args = Vec::new();
        self.call_depth += 1;
        let parsed = (|| {
            if !self.eat(b')') {
                loop {
                    args.push(self.parse_expression()?);
                    if self.eat(b')') {
                        break;
                    }
                    if !self.eat(b',') {
                        return Err("Function arguments must be comma-separated".to_owned());
                    }
                }
            }
            Ok(())
        })();
        self.call_depth -= 1;
        parsed?;
        self.bare_percent = false;
        apply_math_function(&name, &args, self.degrees)
    }
}

/// Rounds away the last bits of a trig result: `sin(pi)` is 0, not 1.2e-16, and `sin(30°)` is 0.5.
fn exact_trig(value: f64) -> f64 {
    let rounded = (value * 1e12).round() / 1e12;
    if (value - rounded).abs() < 1e-14 {
        rounded
    } else {
        value
    }
}

/// `n!` for whole numbers up to 170 (the largest factorial a double holds).
fn factorial(value: f64) -> Result<f64, String> {
    if value < 0.0 || value.fract() != 0.0 {
        return Err("A factorial needs a whole number that is not negative".into());
    }
    if value > 170.0 {
        return Err("The calculation did not produce a finite number".into());
    }
    Ok((2..=value as u32).fold(1.0, |product, factor| product * f64::from(factor)))
}

fn apply_math_function(name: &str, args: &[f64], degrees: bool) -> Result<f64, String> {
    let to_radians = |angle: f64| if degrees { angle.to_radians() } else { angle };
    let from_radians = |angle: f64| if degrees { angle.to_degrees() } else { angle };
    let one = || {
        if args.len() == 1 {
            Ok(args[0])
        } else {
            Err(format!("{name} expects one argument"))
        }
    };
    let two = || {
        if args.len() == 2 {
            Ok((args[0], args[1]))
        } else {
            Err(format!("{name} expects two arguments"))
        }
    };
    match name {
        "abs" => Ok(one()?.abs()),
        "sqrt" => Ok(one()?.sqrt()),
        "sin" => Ok(exact_trig(to_radians(one()?).sin())),
        "cos" => Ok(exact_trig(to_radians(one()?).cos())),
        "tan" => Ok(exact_trig(to_radians(one()?).tan())),
        "asin" => Ok(from_radians(one()?.asin())),
        "acos" => Ok(from_radians(one()?.acos())),
        "atan" => Ok(from_radians(one()?.atan())),
        "sinh" => Ok(one()?.sinh()),
        "cosh" => Ok(one()?.cosh()),
        "ln" => Ok(one()?.ln()),
        "log10" => Ok(one()?.log10()),
        "log2" => Ok(one()?.log2()),
        "exp" => Ok(one()?.exp()),
        "floor" => Ok(one()?.floor()),
        "ceil" => Ok(one()?.ceil()),
        "round" => Ok(one()?.round()),
        "log" => {
            let (value, base) = two()?;
            Ok(value.log(base))
        }
        "pow" | "min" | "max" => {
            let (a, b) = two()?;
            Ok(match name {
                "pow" => a.powf(b),
                "min" => a.min(b),
                _ => a.max(b),
            })
        }
        _ => Err(format!("Unsupported function `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calculate(expression: &str) -> Result<f64, String> {
        evaluate(expression, false).map(|result| result["value"].as_f64().unwrap())
    }

    #[test]
    fn named_functions_may_contain_digits() {
        // B5: the lexer stopped names at digits, so log10 read as `log`.
        assert_eq!(calculate("log10(100)"), Ok(2.0));
        assert_eq!(calculate("log2(8)"), Ok(3.0));
        assert_eq!(calculate("2log10(100)"), Ok(4.0));
    }

    #[test]
    fn a_name_or_bracket_after_a_value_multiplies_it() {
        assert_eq!(calculate("2(3)"), Ok(6.0));
        assert_eq!(calculate("2 pi"), Ok(2.0 * std::f64::consts::PI));
        assert_eq!(calculate("(1+1)(2+2)"), Ok(8.0));
        assert_eq!(calculate("2^2pi"), Ok(4.0 * std::f64::consts::PI));
        assert!(calculate("2 3").is_err());
    }

    #[test]
    fn calculator_parses_functions_and_precedence() {
        let mut parser = ExpressionParser::new("-2^2 + sqrt(81) + sin(pi / 2)");
        assert_eq!(parser.parse_expression().unwrap(), 6.0);
    }

    #[test]
    fn calculator_rejects_excessive_recursive_nesting_without_panicking() {
        let expression = format!("{}1{}", "(".repeat(256), ")".repeat(256));
        let mut parser = ExpressionParser::new(&expression);
        assert!(parser.parse_expression().is_err());

        let unary = format!("{}1", "-".repeat(20_000));
        let mut parser = ExpressionParser::new(&unary);
        assert_eq!(parser.parse_expression().unwrap(), 1.0);
    }
}
