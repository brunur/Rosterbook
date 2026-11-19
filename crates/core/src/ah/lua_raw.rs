//! A second, byte-preserving Lua literal parser.
//!
//! [`crate::lua`] decodes SavedVariables straight into `serde_json::Value`,
//! which can only hold text, so it reads string literals with
//! `from_utf8_lossy`. Some add-ons (Auctionator's price database) store
//! arbitrary binary blobs (CBOR) as Lua strings, and `from_utf8_lossy` would
//! corrupt every non-UTF8 byte in them. This parser keeps string literals as
//! raw bytes instead, for exactly that case. It otherwise mirrors `lua.rs`.

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

#[derive(Debug, Clone)]
#[allow(dead_code)] // Key::Int is built but only Str is read; kept for symmetry with the real grammar.
pub enum Key {
    Int(i64),
    Str(String),
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Bool/Float round out the Lua grammar; this file's data never needs them.
pub enum RawValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// A string literal's exact bytes (no UTF-8 assumption).
    Bytes(Vec<u8>),
    Table(Vec<(Key, RawValue)>),
}

impl RawValue {
    pub fn as_table(&self) -> Option<&[(Key, RawValue)]> {
        match self {
            RawValue::Table(t) => Some(t),
            _ => None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get(&self, key: &str) -> Option<&RawValue> {
        self.as_table()?.iter().find_map(|(k, v)| matches!(k, Key::Str(s) if s == key).then_some(v))
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            RawValue::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// Parses a whole SavedVariables file into `{ VariableName: value }`. Takes
/// raw bytes, not `&str`: some add-ons (Auctionator's price database) write
/// arbitrary binary straight into quoted Lua strings without escaping it, so
/// the file as a whole usually isn't valid UTF-8 even though every *other*
/// value in it is plain text.
pub fn parse_saved_variables(src: &[u8]) -> Result<Vec<(String, RawValue)>> {
    const BOM: &[u8] = "\u{feff}".as_bytes();
    let src = src.strip_prefix(BOM).unwrap_or(src);
    let mut p = Parser { s: src, pos: 0, depth: 0 };
    let mut vars = Vec::new();
    p.skip();
    while !p.eof() {
        let name = p.identifier()?;
        p.skip();
        p.expect(b'=')?;
        let value = p.value()?;
        vars.push((name, value));
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

    fn long_string(&mut self, level: usize) -> Result<Vec<u8>> {
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
                let bytes = self.s[start..self.pos].to_vec();
                self.pos += close.len();
                return Ok(bytes);
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

    fn value(&mut self) -> Result<RawValue> {
        self.skip();
        match self.peek() {
            Some(b'{') => self.table(),
            Some(b'"' | b'\'') => Ok(RawValue::Bytes(self.string()?)),
            Some(b'[') => match self.long_bracket_level() {
                Some(level) => Ok(RawValue::Bytes(self.long_string(level)?)),
                None => self.fail("unexpected '['"),
            },
            Some(b'-' | b'.' | b'0'..=b'9') => self.number(),
            Some(_) => {
                let word = self.identifier()?;
                match word.as_str() {
                    "true" => Ok(RawValue::Bool(true)),
                    "false" => Ok(RawValue::Bool(false)),
                    "nil" => Ok(RawValue::Nil),
                    _ => self.fail(format!("unexpected word '{word}'")),
                }
            }
            None => self.fail("unexpected end of file"),
        }
    }

    fn number(&mut self) -> Result<RawValue> {
        let start = self.pos;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') && matches!(self.at(1), Some(b'x' | b'X')) {
            self.pos += 2;
            let hs = self.pos;
            while matches!(self.peek(), Some(b) if b.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let digits = std::str::from_utf8(&self.s[hs..self.pos]).unwrap_or("");
            return match i64::from_str_radix(digits, 16) {
                Ok(v) => Ok(RawValue::Int(if neg { -v } else { v })),
                Err(_) => self.fail("bad hexadecimal number"),
            };
        }
        for word in ["inf", "nan", "INF", "NAN"] {
            if self.s[self.pos..].starts_with(word.as_bytes()) {
                self.pos += word.len();
                return Ok(RawValue::Nil);
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
                return Ok(RawValue::Int(v));
            }
        }
        match text.parse::<f64>() {
            Ok(v) => Ok(RawValue::Float(v)),
            Err(_) => self.fail(format!("bad number '{text}'")),
        }
    }

    fn string(&mut self) -> Result<Vec<u8>> {
        let quote = self.s[self.pos];
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("unterminated string") };
            self.pos += 1;
            if c == quote {
                return Ok(out);
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

    fn table(&mut self) -> Result<RawValue> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("tables nested too deeply");
        }
        self.expect(b'{')?;
        let mut entries: Vec<(Key, RawValue)> = Vec::new();
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
                    RawValue::Int(i) => Key::Int(i),
                    RawValue::Bytes(b) => Key::Str(String::from_utf8_lossy(&b).into_owned()),
                    _ => Key::Str(String::new()),
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
        Ok(RawValue::Table(entries))
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_raw_bytes_in_strings() {
        let vars = parse_saved_variables(b"A = { x = \"\\001\\002\\255\" }").unwrap();
        let a = vars.iter().find(|(n, _)| n == "A").unwrap();
        let x = a.1.get("x").unwrap();
        assert_eq!(x.as_bytes().unwrap(), &[1, 2, 255]);
    }

    #[test]
    fn reads_nested_tables_and_ints() {
        let vars = parse_saved_variables(br#"DB = { ["realm"] = { ["123"] = "blob", __dbversion = 8 } }"#).unwrap();
        let db = &vars[0].1;
        let realm = db.get("realm").unwrap();
        assert_eq!(realm.get("123").unwrap().as_bytes().unwrap(), b"blob");
        assert_eq!(db.get("__dbversion").is_none(), true); // __dbversion is on DB in the real file, not realm
    }
}
