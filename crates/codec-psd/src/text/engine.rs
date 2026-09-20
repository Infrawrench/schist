//! Bounded parser for the public PSD EngineData token format.
//! Strings are UTF-16BE with byte-level escaping, not UTF-8 or JSON.
use serde_json::{Map, Value};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 200_000;

pub(super) fn parse(bytes: &[u8]) -> Option<Value> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let mut parser = Parser {
        bytes,
        pos: 0,
        nodes: 0,
    };
    let value = parser.value(0)?;
    parser.space();
    if bytes[parser.pos..].iter().any(|&b| b != 0) {
        return None;
    }
    Some(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn token(&mut self) -> Option<&str> {
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| !b.is_ascii_whitespace() && !b"[]<>()/".contains(b))
        {
            self.pos += 1;
        }
        (self.pos > start)
            .then(|| std::str::from_utf8(&self.bytes[start..self.pos]).ok())
            .flatten()
    }
    fn value(&mut self, depth: usize) -> Option<Value> {
        self.space();
        self.nodes += 1;
        if depth > 48 || self.nodes > MAX_NODES {
            return None;
        }
        match *self.bytes.get(self.pos)? {
            b'<' => {
                if self.bytes.get(self.pos..self.pos + 2)? != b"<<" {
                    return None;
                }
                self.pos += 2;
                let mut object = Map::new();
                loop {
                    self.space();
                    if self.bytes.get(self.pos..self.pos + 2) == Some(b">>") {
                        self.pos += 2;
                        break;
                    }
                    if *self.bytes.get(self.pos)? != b'/' {
                        return None;
                    }
                    self.pos += 1;
                    let key = self.token()?.to_owned();
                    object.insert(key, self.value(depth + 1)?);
                }
                Some(Value::Object(object))
            }
            b'[' => {
                self.pos += 1;
                let mut values = Vec::new();
                loop {
                    self.space();
                    if self.bytes.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        break;
                    }
                    values.push(self.value(depth + 1)?);
                }
                Some(Value::Array(values))
            }
            b'(' => {
                self.pos += 1;
                let mut raw = Vec::new();
                loop {
                    let byte = *self.bytes.get(self.pos)?;
                    self.pos += 1;
                    match byte {
                        b')' => break,
                        b'\\' => {
                            raw.push(*self.bytes.get(self.pos)?);
                            self.pos += 1;
                        }
                        _ => raw.push(byte),
                    }
                }
                if raw.is_empty() {
                    return Some(Value::String(String::new()));
                }
                if raw.get(..2)? != [0xfe, 0xff] || raw.len() % 2 != 0 {
                    return None;
                }
                let units: Vec<u16> = raw[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .collect();
                Some(Value::String(String::from_utf16(&units).ok()?))
            }
            b'/' => {
                self.pos += 1;
                let token = self.token()?;
                Some(if token == "nil" {
                    Value::Null
                } else {
                    Value::String(format!("/{token}"))
                })
            }
            _ => match self.token()? {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                number => serde_json::Number::from_f64(number.parse().ok()?).map(Value::Number),
            },
        }
    }
}

pub(super) fn encode(value: &Value) -> Vec<u8> {
    fn write(value: &Value, bytes: &mut Vec<u8>) {
        match value {
            Value::Object(values) => {
                bytes.extend_from_slice(b"<< ");
                for (key, value) in values {
                    bytes.push(b'/');
                    bytes.extend_from_slice(key.as_bytes());
                    bytes.push(b' ');
                    write(value, bytes);
                }
                bytes.extend_from_slice(b">> ");
            }
            Value::Array(values) => {
                bytes.extend_from_slice(b"[ ");
                for value in values {
                    write(value, bytes);
                }
                bytes.extend_from_slice(b"] ");
            }
            Value::String(text) => {
                bytes.extend_from_slice(b"(\xfe\xff");
                for unit in text.encode_utf16() {
                    for byte in unit.to_be_bytes() {
                        if b"()\\".contains(&byte) {
                            bytes.push(b'\\');
                        }
                        bytes.push(byte);
                    }
                }
                bytes.extend_from_slice(b") ");
            }
            Value::Bool(v) => bytes.extend_from_slice(if *v { b"true " } else { b"false " }),
            Value::Null => bytes.extend_from_slice(b"/nil "),
            Value::Number(n) => {
                // EngineData's numeric grammar has no exponent notation.
                // JSON's scientific representation would split into invalid
                // tokens in independent parsers such as ag-psd.
                let decimal = if let Some(integer) = n.as_i64() {
                    integer.to_string()
                } else {
                    format!("{:.8}", n.as_f64().unwrap_or(0.0))
                };
                bytes.extend_from_slice(decimal.as_bytes());
                bytes.push(b' ');
            }
        }
    }
    let mut bytes = Vec::new();
    write(value, &mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings_escape_bytes_even_inside_non_ascii_codepoints() {
        let data =
            serde_json::json!({"Text": "A😀(\\)\u{2900}\u{285c}", "Nested": [true, 1.5, null]});
        assert_eq!(parse(&encode(&data)), Some(data));
    }
    #[test]
    fn malformed_or_deep_data_fails_without_panicking() {
        for bytes in [
            b"<< /x (".as_slice(),
            b"<< /x [ 1",
            b"<< /x (\xfe\xff\x00) >>",
            b"[true] false",
        ] {
            assert!(parse(bytes).is_none());
        }
        let bytes = format!("{}{}", "[ ".repeat(1000), "] ".repeat(1000));
        assert!(parse(bytes.as_bytes()).is_none());
    }
}
