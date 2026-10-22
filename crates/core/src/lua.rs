//! Parser for WoW SavedVariables files.
//!
//! SavedVariables are Lua assignments of literal values (`Name = { ... }`),
//! so a small recursive-descent parser covering tables, strings, numbers,
//! booleans and nil is enough. Tables become JSON arrays when their keys are
//! exactly 1..n, otherwise JSON objects (with keys converted to strings).

use serde_json::{Map, Number, Value};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at line {}", self.message, self.line)
    }
}

impl std::error::Error for ParseError {}

type Result<T> = std::result::Result<T, ParseError>;

/// Parses a whole SavedVariables file into `{ VariableName: value }`.
pub fn parse_saved_variables(src: &str) -> Result<Map<String, Value>> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut p = Parser { s: src.as_bytes(), pos: 0, depth: 0 };
    let mut vars = Map::new();
    p.skip();
    while !p.eof() {
        let name = p.identifier()?;
        p.skip();
        p.expect(b'=')?;
        let value = p.value()?;
        vars.insert(name, value);
        p.skip();
        if p.peek() == Some(b';') {
            p.pos += 1;
            p.skip();
        }
    }
    Ok(vars)
}

const MAX_DEPTH: usize = 200;

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    depth: usize,
}

enum Key {
    Int(i64),
    Str(String),
    Other(String),
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn at(&self, offset: usize) -> Option<u8> {
        self.s.get(self.pos + offset).copied()
    }

    fn fail<T>(&self, message: impl Into<String>) -> Result<T> {
        let line = self.s[..self.pos.min(self.s.len())].iter().filter(|&&b| b == b'\n').count() + 1;
        Err(ParseError { line, message: message.into() })
    }

    fn expect(&mut self, ch: u8) -> Result<()> {
        if self.peek() == Some(ch) {
            self.pos += 1;
            Ok(())
        } else {
            let found = self.peek().map(|b| (b as char).to_string()).unwrap_or_else(|| "end of file".into());
            self.fail(format!("expected '{}' but found '{}'", ch as char, found))
        }
    }

    /// Skips whitespace and comments (`-- line`, `--[[ block ]]`, `--[==[ ]==]`).
    fn skip(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'-') if self.at(1) == Some(b'-') => {
                    self.pos += 2;
                    if let Some(level) = self.long_bracket_level() {
                        let _ = self.long_string(level);
                    } else {
                        while let Some(b) = self.peek() {
                            self.pos += 1;
                            if b == b'\n' {
                                break;
                            }
                        }
                    }
                }
                _ => return,
            }
        }
    }

    /// If at `[` followed by `=`* and `[`, returns the number of `=`.
    fn long_bracket_level(&self) -> Option<usize> {
        if self.peek() != Some(b'[') {
            return None;
        }
        let mut i = 1;
        while self.at(i) == Some(b'=') {
            i += 1;
        }
        (self.at(i) == Some(b'[')).then_some(i - 1)
    }

    fn long_string(&mut self, level: usize) -> Result<String> {
        self.pos += level + 2;
        if self.peek() == Some(b'\r') {
            self.pos += 1;
        }
        if self.peek() == Some(b'\n') {
            self.pos += 1;
        }
        let mut close = Vec::with_capacity(level + 2);
        close.push(b']');
        close.extend(std::iter::repeat_n(b'=', level));
        close.push(b']');
        let start = self.pos;
        while self.pos + close.len() <= self.s.len() {
            if self.s[self.pos..].starts_with(&close) {
                let text = String::from_utf8_lossy(&self.s[start..self.pos]).into_owned();
                self.pos += close.len();
                return Ok(text);
            }
            self.pos += 1;
        }
        self.fail("unterminated long string")
    }

    fn identifier(&mut self) -> Result<String> {
        let start = self.pos;
        match self.peek() {
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => self.pos += 1,
            _ => return self.fail("expected a name"),
        }
        while matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
            self.pos += 1;
        }
        Ok(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned())
    }

    fn value(&mut self) -> Result<Value> {
        self.skip();
        match self.peek() {
            Some(b'{') => self.table(),
            Some(b'"' | b'\'') => Ok(Value::String(self.string()?)),
            Some(b'[') => match self.long_bracket_level() {
                Some(level) => Ok(Value::String(self.long_string(level)?)),
                None => self.fail("unexpected '['"),
            },
            Some(b'-' | b'.' | b'0'..=b'9') => self.number(),
            Some(_) => {
                let word = self.identifier()?;
                match word.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    "nil" => Ok(Value::Null),
                    _ => self.fail(format!("unexpected word '{word}'")),
                }
            }
            None => self.fail("unexpected end of file"),
        }
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.pos;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.pos += 1;
        }
        // Hexadecimal.
        if self.peek() == Some(b'0') && matches!(self.at(1), Some(b'x' | b'X')) {
            self.pos += 2;
            let hs = self.pos;
            while matches!(self.peek(), Some(b) if b.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let digits = std::str::from_utf8(&self.s[hs..self.pos]).unwrap_or("");
            return match i64::from_str_radix(digits, 16) {
                Ok(v) => Ok(Value::from(if neg { -v } else { v })),
                Err(_) => self.fail("bad hexadecimal number"),
            };
        }
        // inf / nan as written by %.17g. JSON can't hold them, so they become null.
        for word in ["inf", "nan", "INF", "NAN"] {
            if self.s[self.pos..].starts_with(word.as_bytes()) {
                self.pos += word.len();
                return Ok(Value::Null);
            }
        }
        let mut is_float = false;
        while let Some(b) = self.peek() {
            match b {
                b'0'..=b'9' => self.pos += 1,
                b'.' => {
                    is_float = true;
                    self.pos += 1;
                }
                b'e' | b'E' => {
                    is_float = true;
                    self.pos += 1;
                    if matches!(self.peek(), Some(b'+' | b'-')) {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or("");
        if !is_float {
            if let Ok(v) = text.parse::<i64>() {
                return Ok(Value::from(v));
            }
        }
        match text.parse::<f64>() {
            Ok(v) => Ok(Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null)),
            Err(_) => self.fail(format!("bad number '{text}'")),
        }
    }

    fn string(&mut self) -> Result<String> {
        let quote = self.s[self.pos];
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("unterminated string") };
            self.pos += 1;
            if c == quote {
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let Some(e) = self.peek() else { return self.fail("unterminated string") };
            self.pos += 1;
            match e {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'a' => out.push(7),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'v' => out.push(11),
                b'\\' | b'"' | b'\'' => out.push(e),
                b'\n' => out.push(b'\n'),
                b'\r' => {
                    out.push(b'\n');
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                }
                b'x' => {
                    let hex = std::str::from_utf8(self.s.get(self.pos..self.pos + 2).unwrap_or(b"")).unwrap_or("");
                    match u8::from_str_radix(hex, 16) {
                        Ok(v) => {
                            out.push(v);
                            self.pos += 2;
                        }
                        Err(_) => return self.fail("bad \\x escape"),
                    }
                }
                b'z' => {
                    while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                        self.pos += 1;
                    }
                }
                b'0'..=b'9' => {
                    let mut v: u32 = (e - b'0') as u32;
                    for _ in 0..2 {
                        match self.peek() {
                            Some(d @ b'0'..=b'9') => {
                                v = v * 10 + (d - b'0') as u32;
                                self.pos += 1;
                            }
                            _ => break,
                        }
                    }
                    out.push(v.min(255) as u8);
                }
                other => out.push(other),
            }
        }
    }

    fn table(&mut self) -> Result<Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("tables nested too deeply");
        }
        self.expect(b'{')?;
        let mut entries: Vec<(Key, Value)> = Vec::new();
        let mut next_index: i64 = 1;
        loop {
            self.skip();
            if self.peek() == Some(b'}') {
                self.pos += 1;
                break;
            }
            let key = if self.peek() == Some(b'[') && self.long_bracket_level().is_none() {
                self.pos += 1;
                let k = self.value()?;
                self.skip();
                self.expect(b']')?;
                self.skip();
                self.expect(b'=')?;
                match k {
                    Value::Number(n) if n.as_i64().is_some() => Key::Int(n.as_i64().unwrap()),
                    Value::String(s) => Key::Str(s),
                    other => Key::Other(other.to_string()),
                }
            } else if self.is_named_field() {
                let name = self.identifier()?;
                self.skip();
                self.expect(b'=')?;
                Key::Str(name)
            } else {
                let k = Key::Int(next_index);
                next_index += 1;
                k
            };
            let v = self.value()?;
            entries.push((key, v));
            self.skip();
            match self.peek() {
                Some(b',' | b';') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return self.fail("expected ',' or '}' in table"),
            }
        }
        self.depth -= 1;
        Ok(to_value(entries))
    }

    /// `name =` (but not `name ==`) starts a named field.
    fn is_named_field(&self) -> bool {
        let mut i = self.pos;
        match self.s.get(i) {
            Some(b) if b.is_ascii_alphabetic() || *b == b'_' => i += 1,
            _ => return false,
        }
        while matches!(self.s.get(i), Some(b) if b.is_ascii_alphanumeric() || *b == b'_') {
            i += 1;
        }
        while matches!(self.s.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            i += 1;
        }
        self.s.get(i) == Some(&b'=') && self.s.get(i + 1) != Some(&b'=')
    }
}

fn to_value(entries: Vec<(Key, Value)>) -> Value {
    let n = entries.len() as i64;
    let sequential = n > 0
        && entries.iter().all(|(k, _)| matches!(k, Key::Int(i) if *i >= 1 && *i <= n))
        && {
            let mut seen = vec![false; n as usize];
            entries.iter().all(|(k, _)| match k {
                Key::Int(i) => !std::mem::replace(&mut seen[(*i - 1) as usize], true),
                _ => false,
            })
        };
    if sequential {
        let mut arr = vec![Value::Null; n as usize];
        for (k, v) in entries {
            if let Key::Int(i) = k {
                arr[(i - 1) as usize] = v;
            }
        }
        return Value::Array(arr);
    }
    let mut obj = Map::new();
    for (k, v) in entries {
        if v.is_null() {
            continue;
        }
        let key = match k {
            Key::Int(i) => i.to_string(),
            Key::Str(s) => s,
            Key::Other(s) => s,
        };
        obj.insert(key, v);
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_typical_saved_variables() {
        let src = r#"
AltLedgerDB = {
	["schema"] = 1,
	["characters"] = {
		["Player-1"] = {
			["name"] = "Faashri \"Gale\"",
			["money"] = 9308,
			["ratio"] = 5.0625,
			["neg"] = -42,
			["resting"] = true,
			["items"] = { 6948, 2589, nil, 774, },
			["sparse"] = { [2] = "b", [5] = "e" },
		},
	},
}
-- trailing comment
Other = nil
"#;
        let v = parse_saved_variables(src).unwrap();
        let c = &v["AltLedgerDB"]["characters"]["Player-1"];
        assert_eq!(c["name"], json!("Faashri \"Gale\""));
        assert_eq!(c["money"], json!(9308));
        assert_eq!(c["ratio"], json!(5.0625));
        assert_eq!(c["neg"], json!(-42));
        assert_eq!(c["resting"], json!(true));
        assert_eq!(c["items"], json!([6948, 2589, null, 774]));
        assert_eq!(c["sparse"], json!({"2": "b", "5": "e"}));
        assert_eq!(v["Other"], Value::Null);
    }

    #[test]
    fn handles_escapes_and_long_strings() {
        let v = parse_saved_variables("A = { \"a\\nb\\065\\x41\", [==[raw ]] text]==], 0x1F, 1e3 }").unwrap();
        assert_eq!(v["A"], json!(["a\nbAA", "raw ]] text", 31, 1000.0]));
    }

    #[test]
    fn reports_errors_with_line_numbers() {
        let err = parse_saved_variables("A = {\n 1,\n 2 3 }").unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn rejects_absurd_nesting() {
        let src = format!("A = {}{}", "{".repeat(500), "}".repeat(500));
        assert!(parse_saved_variables(&src).is_err());
    }
}
