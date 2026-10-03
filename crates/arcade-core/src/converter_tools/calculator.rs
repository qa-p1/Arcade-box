//! Arithmetic and scientific expressions, evaluated by a small recursive
//! descent parser with a nesting limit.

use super::format_number;
use serde_json::{Value, json};

const MAX_EXPRESSION_DEPTH: usize = 128;

pub(super) fn evaluate(expression: &str) -> Result<Value, String> {
    let mut parser = ExpressionParser::new(expression);
    let value = parser.parse_expression()?;
    parser.skip_space();
    if parser.peek().is_some() {
        return Err(format!("Unexpected character at byte {}", parser.position));
    }
    if !value.is_finite() {
        return Err("The calculation did not produce a finite number".into());
    }
    Ok(json!({"expression": expression.trim(), "value": value, "formatted": format_number(value)}))
}

struct ExpressionParser<'a> {
    input: &'a str,
    position: usize,
    depth: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            depth: 0,
        }
    }
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }
    fn skip_space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
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
    fn parse_expression(&mut self) -> Result<f64, String> {
        let mut value = self.parse_product()?;
        loop {
            if self.eat(b'+') {
                value += self.parse_product()?;
            } else if self.eat(b'-') {
                value -= self.parse_product()?;
            } else {
                return Ok(value);
            }
        }
    }
    fn parse_product(&mut self) -> Result<f64, String> {
        let mut value = self.parse_unary()?;
        loop {
            if self.eat(b'*') {
                value *= self.parse_unary()?;
            } else if self.eat(b'/') {
                let rhs = self.parse_unary()?;
                if rhs == 0.0 {
                    return Err("Division by zero".into());
                }
                value /= rhs;
            } else if self.eat(b'%') {
                let rhs = self.parse_unary()?;
                if rhs == 0.0 {
                    return Err("Remainder by zero".into());
                }
                value %= rhs;
            } else {
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
            let lhs = self.parse_primary()?;
            if self.eat(b'^') {
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
                byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
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
                .parse::<f64>()
                .map_err(|_| "Enter a valid number".into());
        }
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
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
        if !self.eat(b')') {
            loop {
                args.push(self.parse_expression()?);
                if self.eat(b')') {
                    break;
                }
                if !self.eat(b',') {
                    return Err("Function arguments must be comma-separated".into());
                }
            }
        }
        apply_math_function(&name, &args)
    }
}

fn apply_math_function(name: &str, args: &[f64]) -> Result<f64, String> {
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
        "sin" => Ok(one()?.sin()),
        "cos" => Ok(one()?.cos()),
        "tan" => Ok(one()?.tan()),
        "asin" => Ok(one()?.asin()),
        "acos" => Ok(one()?.acos()),
        "atan" => Ok(one()?.atan()),
        "sinh" => Ok(one()?.sinh()),
        "cosh" => Ok(one()?.cosh()),
        "ln" => Ok(one()?.ln()),
        "log10" => Ok(one()?.log10()),
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
