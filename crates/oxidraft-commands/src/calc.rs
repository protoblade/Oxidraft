//! CAL and QuickCalc's arithmetic: numbers, + - * / ^, brackets, and
//! AutoCAD CAL's functions (angles in degrees).

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Op(char),
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // An exponent: 1.5e3, 2E-4.
            if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                let mut j = i + 1;
                if j < chars.len() && matches!(chars[j], '+' | '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let s: String = chars[start..i].iter().collect();
            out.push(Token::Number(s.parse().map_err(|_| format!("Not a number: {s}"))?));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect::<String>().to_ascii_lowercase()));
        } else if "+-*/^(),".contains(c) {
            out.push(Token::Op(c));
            i += 1;
        } else {
            return Err(format!("Unexpected \"{c}\"."));
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: char) -> bool {
        if self.peek() == Some(&Token::Op(op)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// sum: product (+|- product)*
    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            if self.eat('+') {
                v += self.product()?;
            } else if self.eat('-') {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }

    /// product: unary (*|/ unary)*
    fn product(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') {
                v *= self.unary()?;
            } else if self.eat('/') {
                let d = self.unary()?;
                if d == 0.0 {
                    return Err("Division by zero.".into());
                }
                v /= d;
            } else {
                return Ok(v);
            }
        }
    }

    /// unary: (+|-) unary | power
    fn unary(&mut self) -> Result<f64, String> {
        if self.eat('-') {
            return Ok(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    /// power: atom (^ unary)?, right to left.
    fn power(&mut self) -> Result<f64, String> {
        let base = self.atom()?;
        if self.eat('^') { Ok(base.powf(self.unary()?)) } else { Ok(base) }
    }

    fn atom(&mut self) -> Result<f64, String> {
        match self.peek().cloned() {
            Some(Token::Number(v)) => {
                self.at += 1;
                Ok(v)
            }
            Some(Token::Op('(')) => {
                self.at += 1;
                let v = self.sum()?;
                if !self.eat(')') {
                    return Err("Missing \")\".".into());
                }
                Ok(v)
            }
            Some(Token::Name(name)) => {
                self.at += 1;
                if name == "pi" {
                    return Ok(std::f64::consts::PI);
                }
                if !self.eat('(') {
                    return Err(format!("Unknown name \"{name}\"."));
                }
                let mut args = vec![self.sum()?];
                while self.eat(',') {
                    args.push(self.sum()?);
                }
                if !self.eat(')') {
                    return Err("Missing \")\".".into());
                }
                call(&name, &args)
            }
            _ => Err("Incomplete expression.".into()),
        }
    }
}

/// CAL's functions; angles in degrees.
fn call(name: &str, args: &[f64]) -> Result<f64, String> {
    let one = || match args {
        [x] => Ok(*x),
        _ => Err(format!("{name} takes one value.")),
    };
    let inverse =
        |v: f64| if (-1.0..=1.0).contains(&v) { Ok(v) } else { Err(format!("{name} needs a value from -1 to 1.")) };
    Ok(match name {
        "sin" => one()?.to_radians().sin(),
        "cos" => one()?.to_radians().cos(),
        "tang" | "tan" => one()?.to_radians().tan(),
        "asin" => inverse(one()?)?.asin().to_degrees(),
        "acos" => inverse(one()?)?.acos().to_degrees(),
        "atan" => match args {
            [y, x] => y.atan2(*x).to_degrees(),
            _ => one()?.atan().to_degrees(),
        },
        "sqrt" if one()? < 0.0 => return Err("sqrt needs a value of 0 or more.".into()),
        "sqrt" => one()?.sqrt(),
        "sqr" => one()?.powi(2),
        "abs" => one()?.abs(),
        "round" => one()?.round(),
        "trunc" => one()?.trunc(),
        "ln" | "log" | "exp10" if name != "exp10" && one()? <= 0.0 => {
            return Err(format!("{name} needs a positive value."));
        }
        "ln" => one()?.ln(),
        "log" => one()?.log10(),
        "exp" => one()?.exp(),
        "exp10" => 10f64.powf(one()?),
        "r2d" => one()?.to_degrees(),
        "d2r" => one()?.to_radians(),
        "min" if !args.is_empty() => args.iter().copied().fold(f64::INFINITY, f64::min),
        "max" if !args.is_empty() => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => return Err(format!("Unknown function \"{name}\".")),
    })
}

/// The value of an expression like `2*(3+4)^2`, `sqrt(2)`, `sin(30)`.
pub fn evaluate(text: &str) -> Result<f64, String> {
    let mut p = Parser { tokens: tokens(text)?, at: 0 };
    let v = p.sum()?;
    if p.at < p.tokens.len() {
        return Err("Unexpected text after the expression.".into());
    }
    if !v.is_finite() {
        return Err("The result is too large.".into());
    }
    Ok(v)
}

/// A result as CAL prints it: up to 6 decimals, no trailing zeros.
pub fn show(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// CAL: evaluates an expression at the command line.
#[derive(Default)]
pub struct Cal;

impl Command for Cal {
    fn prompt(&self) -> Prompt {
        Prompt::new(">> Expression")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Text(t) if !t.trim().is_empty() => {
                ctx.messages.push(show(evaluate(&t)?));
                Ok(Step::Done)
            }
            _ => Ok(Step::Done),
        }
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_functions() {
        let e = |t: &str| evaluate(t).unwrap();
        assert_eq!(e("2*(3+4)^2"), 98.0);
        assert_eq!(e("-2^2"), -4.0);
        assert_eq!(e("2^3^2"), 512.0, "right to left");
        assert_eq!(e("10/4 - 1"), 1.5);
        assert!((e("sin(30)") - 0.5).abs() < 1e-12);
        assert!((e("atan(1,1)") - 45.0).abs() < 1e-12);
        assert_eq!(e("sqrt(16) + abs(-2)"), 6.0);
        assert!((e("2*pi") - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(e("1.5e3"), 1500.0);
        assert_eq!(e("max(1, 7, 3)"), 7.0);
        assert!(evaluate("1/0").is_err() && evaluate("2+").is_err() && evaluate("foo(1)").is_err());
        assert_eq!(show(2.0 / 3.0), "0.666667");
        assert_eq!(show(98.0), "98");
    }
}

#[cfg(test)]
mod engine_tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::Document;

    #[test]
    fn cal_prints_the_value() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        e.dynamic_input = false;
        for t in ["cal", "2 * (3 + 4)"] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        assert_eq!(e.history().last().map(String::as_str), Some("14"));
    }
}
