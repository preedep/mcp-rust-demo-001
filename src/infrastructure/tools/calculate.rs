use serde_json::{json, Value};

use crate::domain::{DomainError, Tool, ToolAnnotations, ToolDescriptor, ToolOutput};

pub struct Calculate;

impl Tool for Calculate {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "calculate",
            description: "Evaluate an arithmetic expression on the server and return the \
                          numeric result. Use this for any calculation rather than working \
                          it out yourself, so the answer is checked. Supports + - * / %, \
                          parentheses, unary minus and decimals, for example \
                          '(2 + 3) * 4.5'. Safe, instant and read-only.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "The arithmetic expression to evaluate."
                    }
                },
                "required": ["expression"],
                "additionalProperties": false
            }),
            annotations: ToolAnnotations::read_only(),
        }
    }

    fn invoke(&self, args: &Value) -> Result<ToolOutput, DomainError> {
        let expr = args
            .get("expression")
            .and_then(Value::as_str)
            .ok_or_else(|| DomainError::InvalidArgument("'expression' must be a string".into()))?;

        Ok(match eval(expr) {
            Ok(v) => ToolOutput::ok(format_number(v)),
            Err(e) => ToolOutput::failed(format!("cannot evaluate '{expr}': {e}")),
        })
    }
}

fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Recursive-descent parser over the byte slice. Evaluates untrusted input, so it must
/// never panic: every failure is a typed `Err`.
fn eval(input: &str) -> Result<f64, String> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        pos: 0,
        depth: 0,
    };
    let v = p.expr()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("unexpected character at position {}", p.pos));
    }
    if !v.is_finite() {
        return Err("result is not a finite number".into());
    }
    Ok(v)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: u32,
}

const MAX_DEPTH: u32 = 64;

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut acc = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.pos += 1;
            let rhs = self.term()?;
            acc = if op == b'+' { acc + rhs } else { acc - rhs };
        }
        Ok(acc)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut acc = self.unary()?;
        while let Some(op @ (b'*' | b'/' | b'%')) = self.peek() {
            self.pos += 1;
            let rhs = self.unary()?;
            if matches!(op, b'/' | b'%') && rhs == 0.0 {
                return Err("division by zero".into());
            }
            acc = match op {
                b'*' => acc * rhs,
                b'/' => acc / rhs,
                _ => acc % rhs,
            };
        }
        Ok(acc)
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                Ok(-self.unary()?)
            }
            Some(b'+') => {
                self.pos += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'(') => {
                if self.depth + 1 > MAX_DEPTH {
                    return Err("expression nested too deeply".into());
                }
                self.pos += 1;
                self.depth += 1;
                let v = self.expr()?;
                self.depth -= 1;
                if self.peek() != Some(b')') {
                    return Err("unclosed parenthesis".into());
                }
                self.pos += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(c) => Err(format!("unexpected character '{}'", c as char)),
            None => Err("unexpected end of expression".into()),
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while matches!(self.bytes.get(self.pos), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            while matches!(self.bytes.get(self.pos), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "invalid UTF-8 in number".to_string())?;
        text.parse::<f64>()
            .map_err(|_| format!("invalid number '{text}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("2+3*4").unwrap(), 14.0);
        assert_eq!(eval("(2+3)*4.5").unwrap(), 22.5);
        assert_eq!(eval("-3 + 10 % 4").unwrap(), -1.0);
        assert_eq!(eval("10 / 4").unwrap(), 2.5);
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        for bad in ["", "2+", "(1", "1)", "2**3", "abc", "1/0", "1 2", "."] {
            assert!(eval(bad).is_err(), "expected error for {bad:?}");
        }
    }

    #[test]
    fn rejects_deep_nesting() {
        let deep = format!("{}1{}", "(".repeat(500), ")".repeat(500));
        assert!(eval(&deep).is_err());
    }
}
