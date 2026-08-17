//! RFC 8785 JSON Canonicalization Scheme for the closed manifest value set.
//!
//! Allowed values: objects, arrays, strings, integers, booleans.
//! Floats, `null`, `NaN`, and `Infinity` are rejected (non-JCS for this profile).

use std::collections::BTreeMap;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use crate::error::PackageError;

/// Strict JSON value used for JCS and duplicate-key rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrictValue {
    /// JSON boolean.
    Bool(bool),
    /// JSON integer (no floats).
    Integer(i64),
    /// JSON string.
    String(String),
    /// JSON array.
    Array(Vec<StrictValue>),
    /// JSON object with unique, sorted keys.
    Object(BTreeMap<String, StrictValue>),
}

impl StrictValue {
    /// RFC 8785 JCS bytes (UTF-8, no insignificant whitespace, sorted keys).
    pub fn to_jcs(&self) -> Result<Vec<u8>, PackageError> {
        let mut out = Vec::new();
        write_jcs(self, &mut out)?;
        Ok(out)
    }
}

/// Parse UTF-8 JSON, reject duplicate keys / comments / trailing data / floats / null.
pub fn parse_strict_json(bytes: &[u8]) -> Result<StrictValue, PackageError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        if looks_like_cbor(bytes) {
            PackageError::CborRejected
        } else {
            PackageError::json("manifest is not UTF-8")
        }
    })?;
    let trimmed_start = text.trim_start_matches(is_json_ws_char);
    if trimmed_start.is_empty() {
        return Err(PackageError::json("empty manifest"));
    }
    if !trimmed_start.starts_with('{') {
        return if looks_like_cbor(bytes) {
            Err(PackageError::CborRejected)
        } else {
            Err(PackageError::json(
                "manifest must be a JSON object (package major 1)",
            ))
        };
    }

    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue::deserialize(&mut de).map_err(|e| PackageError::json(e.to_string()))?;
    de.end()
        .map_err(|e| PackageError::json(format!("trailing data after JSON: {e}")))?;
    if !matches!(value, StrictValue::Object(_)) {
        return Err(PackageError::json("manifest must be a JSON object"));
    }
    Ok(value)
}

/// True if `b` is JSON insignificant whitespace.
pub(crate) fn is_json_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

fn is_json_ws_char(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// Classify a buffer as CBOR (self-describe tag or non-JSON high-bit start).
pub(crate) fn looks_like_cbor(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xD9, 0xD9, 0xF7]) {
        return true;
    }
    let Some(&first) = bytes.iter().find(|b| !is_json_ws(**b)) else {
        return false;
    };
    if is_json_start(first) {
        return false;
    }
    // CBOR maps/arrays/tags/byte-strings and other high-bit major types.
    first >= 0x80 || matches!(first >> 5, 2 | 4 | 5 | 6)
}

fn is_json_start(b: u8) -> bool {
    matches!(b, b'{' | b'[' | b'"' | b't' | b'f' | b'n' | b'-') || b.is_ascii_digit()
}

fn write_jcs(v: &StrictValue, out: &mut Vec<u8>) -> Result<(), PackageError> {
    match v {
        StrictValue::Bool(true) => out.extend_from_slice(b"true"),
        StrictValue::Bool(false) => out.extend_from_slice(b"false"),
        StrictValue::Integer(n) => out.extend_from_slice(&itoa_i64(*n)),
        StrictValue::String(s) => write_jcs_string(s, out),
        StrictValue::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_jcs(item, out)?;
            }
            out.push(b']');
        }
        StrictValue::Object(map) => {
            out.push(b'{');
            for (i, (k, val)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_jcs_string(k, out);
                out.push(b':');
                write_jcs(val, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// ES6 / JCS integer form: base-10, no `+`, no leading zeros except `0`.
fn itoa_i64(n: i64) -> Vec<u8> {
    n.to_string().into_bytes()
}

fn write_jcs_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(br#"\""#),
            '\\' => out.extend_from_slice(br#"\\"#),
            c if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = c as u32;
                out.extend_from_slice(br"\u00");
                out.push(HEX[((code >> 4) & 0xf) as usize]);
                out.push(HEX[(code & 0xf) as usize]);
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictValue;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON object, array, string, integer, or boolean")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(StrictValue::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(StrictValue::Integer(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        i64::try_from(v)
            .map(StrictValue::Integer)
            .map_err(|_| E::custom("integer out of i64 range"))
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Self::Value, E> {
        i64::try_from(v)
            .map(StrictValue::Integer)
            .map_err(|_| E::custom("integer out of i64 range"))
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
        i64::try_from(v)
            .map(StrictValue::Integer)
            .map_err(|_| E::custom("integer out of i64 range"))
    }

    fn visit_f64<E: de::Error>(self, _v: f64) -> Result<Self::Value, E> {
        Err(E::custom(
            "floating-point numbers are not allowed in package manifests (JCS)",
        ))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(StrictValue::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(StrictValue::String(v))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Err(E::custom("null is not allowed in package manifests"))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Err(E::custom("null is not allowed in package manifests"))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::new();
        while let Some(elem) = seq.next_element()? {
            items.push(elem);
        }
        Ok(StrictValue::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut obj = BTreeMap::new();
        while let Some((key, val)) = map.next_entry::<String, StrictValue>()? {
            if obj.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key {key:?}")));
            }
            obj.insert(key, val);
        }
        Ok(StrictValue::Object(obj))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jcs_sorts_keys_and_drops_whitespace() {
        let v = parse_strict_json(br#"{ "b" : 2, "a" : 1 }"#).unwrap();
        assert_eq!(v.to_jcs().unwrap(), br#"{"a":1,"b":2}"#);
    }

    #[test]
    fn jcs_nested_object_and_array() {
        let v = parse_strict_json(br#"{"x":[1,2,{"z":3,"y":4}]}"#).unwrap();
        assert_eq!(v.to_jcs().unwrap(), br#"{"x":[1,2,{"y":4,"z":3}]}"#);
    }

    #[test]
    fn jcs_escapes_control_and_quotes() {
        let v = parse_strict_json(br#"{"nl":"\n","quote":"\""}"#).unwrap();
        assert_eq!(v.to_jcs().unwrap(), br#"{"nl":"\u000a","quote":"\""}"#);
    }

    #[test]
    fn rejects_duplicate_keys() {
        let err = parse_strict_json(br#"{"id":"a","id":"b"}"#).unwrap_err();
        assert!(err.to_string().contains("duplicate key"), "{err}");
    }

    #[test]
    fn rejects_float() {
        let err = parse_strict_json(br#"{"n":1.5}"#).unwrap_err();
        assert!(err.to_string().contains("floating-point"), "{err}");
    }

    #[test]
    fn rejects_comments() {
        assert!(parse_strict_json(br#"{"a":1 /* c */}"#).is_err());
    }

    #[test]
    fn cbor_self_describe_is_detected() {
        assert!(looks_like_cbor(&[0xD9, 0xD9, 0xF7, 0xA0]));
        assert!(looks_like_cbor(&[0xA1, 0x01, 0x02]));
        assert!(!looks_like_cbor(br#"{"a":1}"#));
    }
}
