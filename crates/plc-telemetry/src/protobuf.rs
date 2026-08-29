//! Focused Sparkplug B protobuf (proto2) encoder/decoder.
//!
//! Field numbers match Eclipse Tahu `Payload` / `Metric` / `PropertySet`.
//! Implemented in-tree so we do not vendor the EPL-2.0 `.proto` file.

use crate::error::TelemetryError;

const WIRE_VARINT: u32 = 0;
const WIRE_LEN: u32 = 2;
const WIRE_32: u32 = 5;

/// Sparkplug metric value oneof.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    /// `int_value` (Int8/16/32).
    Int(u32),
    /// `long_value` (Int64/UInt64).
    Long(u64),
    /// `float_value`.
    Float(f32),
    /// `boolean_value`.
    Bool(bool),
    /// `string_value`.
    String(String),
}

/// One key/value in a metric `PropertySet`.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// Property name (`Quality`, `Forced`, `engUnit`).
    pub key: String,
    /// Sparkplug DataType of the property.
    pub datatype: u32,
    /// Property value.
    pub value: MetricValue,
}

/// One Sparkplug metric.
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    /// Present on birth; omitted on DATA when `alias` is set.
    pub name: Option<String>,
    /// Stable alias (u32 stored as protobuf uint64).
    pub alias: Option<u64>,
    /// Unix ms.
    pub timestamp: Option<u64>,
    /// Sparkplug DataType.
    pub datatype: u32,
    /// `Quality` / `Forced` / `engUnit`.
    pub properties: Vec<Property>,
    /// Metric value.
    pub value: Option<MetricValue>,
}

impl Metric {
    /// Empty metric of `datatype`.
    #[must_use]
    pub fn new(datatype: u32) -> Self {
        Self {
            name: None,
            alias: None,
            timestamp: None,
            datatype,
            properties: Vec::new(),
            value: None,
        }
    }
}

/// Sparkplug `Payload`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Payload {
    /// Unix ms.
    pub timestamp: Option<u64>,
    /// Metrics.
    pub metrics: Vec<Metric>,
    /// Sequence 0–255 (stored as uint64).
    pub seq: Option<u64>,
}

impl Payload {
    /// Encode proto2 bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(ts) = self.timestamp {
            write_key(&mut buf, 1, WIRE_VARINT);
            write_varint(&mut buf, ts);
        }
        for m in &self.metrics {
            let inner = encode_metric(m);
            write_key(&mut buf, 2, WIRE_LEN);
            write_varint(&mut buf, inner.len() as u64);
            buf.extend_from_slice(&inner);
        }
        if let Some(seq) = self.seq {
            write_key(&mut buf, 3, WIRE_VARINT);
            write_varint(&mut buf, seq);
        }
        buf
    }

    /// Decode proto2 bytes (unknown fields skipped).
    pub fn decode(bytes: &[u8]) -> Result<Self, TelemetryError> {
        let mut cur = Cursor::new(bytes);
        let mut payload = Self::default();
        while cur.remaining() > 0 {
            let (field, wire) = cur.read_key()?;
            match (field, wire) {
                (1, WIRE_VARINT) => payload.timestamp = Some(cur.read_varint()?),
                (2, WIRE_LEN) => {
                    let inner = cur.read_len_bytes()?;
                    payload.metrics.push(decode_metric(inner)?);
                }
                (3, WIRE_VARINT) => payload.seq = Some(cur.read_varint()?),
                _ => cur.skip(wire)?,
            }
        }
        Ok(payload)
    }
}

/// True when an NCMD payload requests `Node Control/Rebirth = true`.
#[must_use]
pub fn is_rebirth_command(bytes: &[u8]) -> bool {
    let Ok(payload) = Payload::decode(bytes) else {
        return false;
    };
    payload.metrics.iter().any(|m| {
        m.name.as_deref() == Some("Node Control/Rebirth")
            && matches!(m.value, Some(MetricValue::Bool(true)))
    })
}

fn encode_metric(m: &Metric) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(name) = &m.name {
        write_string(&mut buf, 1, name);
    }
    if let Some(alias) = m.alias {
        write_key(&mut buf, 2, WIRE_VARINT);
        write_varint(&mut buf, alias);
    }
    if let Some(ts) = m.timestamp {
        write_key(&mut buf, 3, WIRE_VARINT);
        write_varint(&mut buf, ts);
    }
    write_key(&mut buf, 4, WIRE_VARINT);
    write_varint(&mut buf, u64::from(m.datatype));
    if !m.properties.is_empty() {
        let inner = encode_property_set(&m.properties);
        write_key(&mut buf, 9, WIRE_LEN);
        write_varint(&mut buf, inner.len() as u64);
        buf.extend_from_slice(&inner);
    }
    if let Some(v) = &m.value {
        write_metric_value(&mut buf, v);
    }
    buf
}

fn decode_metric(bytes: &[u8]) -> Result<Metric, TelemetryError> {
    let mut cur = Cursor::new(bytes);
    let mut m = Metric::new(0);
    while cur.remaining() > 0 {
        let (field, wire) = cur.read_key()?;
        match (field, wire) {
            (1, WIRE_LEN) => m.name = Some(cur.read_string()?),
            (2, WIRE_VARINT) => m.alias = Some(cur.read_varint()?),
            (3, WIRE_VARINT) => m.timestamp = Some(cur.read_varint()?),
            (4, WIRE_VARINT) => m.datatype = cur.read_varint()? as u32,
            (9, WIRE_LEN) => {
                let inner = cur.read_len_bytes()?;
                m.properties = decode_property_set(inner)?;
            }
            (10, WIRE_VARINT) => m.value = Some(MetricValue::Int(cur.read_varint()? as u32)),
            (11, WIRE_VARINT) => m.value = Some(MetricValue::Long(cur.read_varint()?)),
            (12, WIRE_32) => {
                let bits = cur.read_fixed32()?;
                m.value = Some(MetricValue::Float(f32::from_bits(bits)));
            }
            (14, WIRE_VARINT) => m.value = Some(MetricValue::Bool(cur.read_varint()? != 0)),
            (15, WIRE_LEN) => m.value = Some(MetricValue::String(cur.read_string()?)),
            _ => cur.skip(wire)?,
        }
    }
    Ok(m)
}

fn encode_property_set(props: &[Property]) -> Vec<u8> {
    let mut buf = Vec::new();
    for p in props {
        write_string(&mut buf, 1, &p.key);
    }
    for p in props {
        let inner = encode_property_value(p);
        write_key(&mut buf, 2, WIRE_LEN);
        write_varint(&mut buf, inner.len() as u64);
        buf.extend_from_slice(&inner);
    }
    buf
}

fn decode_property_set(bytes: &[u8]) -> Result<Vec<Property>, TelemetryError> {
    let mut cur = Cursor::new(bytes);
    let mut keys = Vec::new();
    let mut values = Vec::new();
    while cur.remaining() > 0 {
        let (field, wire) = cur.read_key()?;
        match (field, wire) {
            (1, WIRE_LEN) => keys.push(cur.read_string()?),
            (2, WIRE_LEN) => {
                let inner = cur.read_len_bytes()?;
                values.push(decode_property_value(inner)?);
            }
            _ => cur.skip(wire)?,
        }
    }
    let mut out = Vec::new();
    for (i, key) in keys.into_iter().enumerate() {
        if let Some((datatype, value)) = values.get(i) {
            out.push(Property {
                key,
                datatype: *datatype,
                value: value.clone(),
            });
        }
    }
    Ok(out)
}

fn encode_property_value(p: &Property) -> Vec<u8> {
    let mut buf = Vec::new();
    write_key(&mut buf, 1, WIRE_VARINT);
    write_varint(&mut buf, u64::from(p.datatype));
    match &p.value {
        MetricValue::Int(v) => {
            write_key(&mut buf, 3, WIRE_VARINT);
            write_varint(&mut buf, u64::from(*v));
        }
        MetricValue::Long(v) => {
            write_key(&mut buf, 4, WIRE_VARINT);
            write_varint(&mut buf, *v);
        }
        MetricValue::Float(v) => {
            write_key(&mut buf, 5, WIRE_32);
            buf.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        MetricValue::Bool(v) => {
            write_key(&mut buf, 7, WIRE_VARINT);
            write_varint(&mut buf, u64::from(*v));
        }
        MetricValue::String(s) => write_string(&mut buf, 8, s),
    }
    buf
}

fn decode_property_value(bytes: &[u8]) -> Result<(u32, MetricValue), TelemetryError> {
    let mut cur = Cursor::new(bytes);
    let mut datatype = 0_u32;
    let mut value = MetricValue::Bool(false);
    while cur.remaining() > 0 {
        let (field, wire) = cur.read_key()?;
        match (field, wire) {
            (1, WIRE_VARINT) => datatype = cur.read_varint()? as u32,
            (3, WIRE_VARINT) => value = MetricValue::Int(cur.read_varint()? as u32),
            (4, WIRE_VARINT) => value = MetricValue::Long(cur.read_varint()?),
            (5, WIRE_32) => {
                let bits = cur.read_fixed32()?;
                value = MetricValue::Float(f32::from_bits(bits));
            }
            (7, WIRE_VARINT) => value = MetricValue::Bool(cur.read_varint()? != 0),
            (8, WIRE_LEN) => value = MetricValue::String(cur.read_string()?),
            _ => cur.skip(wire)?,
        }
    }
    Ok((datatype, value))
}

fn write_metric_value(buf: &mut Vec<u8>, v: &MetricValue) {
    match v {
        MetricValue::Int(x) => {
            write_key(buf, 10, WIRE_VARINT);
            write_varint(buf, u64::from(*x));
        }
        MetricValue::Long(x) => {
            write_key(buf, 11, WIRE_VARINT);
            write_varint(buf, *x);
        }
        MetricValue::Float(x) => {
            write_key(buf, 12, WIRE_32);
            buf.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        MetricValue::Bool(x) => {
            write_key(buf, 14, WIRE_VARINT);
            write_varint(buf, u64::from(*x));
        }
        MetricValue::String(s) => write_string(buf, 15, s),
    }
}

fn write_key(buf: &mut Vec<u8>, field: u32, wire: u32) {
    write_varint(buf, u64::from((field << 3) | wire));
}

fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        buf.push(b);
        if v == 0 {
            break;
        }
    }
}

fn write_string(buf: &mut Vec<u8>, field: u32, s: &str) {
    write_key(buf, field, WIRE_LEN);
    write_varint(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn read_byte(&mut self) -> Result<u8, TelemetryError> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| TelemetryError::protobuf("truncated protobuf"))?;
        self.pos += 1;
        Ok(b)
    }

    fn read_varint(&mut self) -> Result<u64, TelemetryError> {
        let mut result = 0_u64;
        for shift in 0..10 {
            let b = self.read_byte()?;
            result |= u64::from(b & 0x7f) << (shift * 7);
            if b & 0x80 == 0 {
                return Ok(result);
            }
        }
        Err(TelemetryError::protobuf("varint overflow"))
    }

    fn read_key(&mut self) -> Result<(u32, u32), TelemetryError> {
        let tag = self.read_varint()?;
        Ok(((tag >> 3) as u32, (tag & 7) as u32))
    }

    fn read_len_bytes(&mut self) -> Result<&'a [u8], TelemetryError> {
        let len = self.read_varint()? as usize;
        if self.remaining() < len {
            return Err(TelemetryError::protobuf("truncated length-delimited"));
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.buf[start..self.pos])
    }

    fn read_string(&mut self) -> Result<String, TelemetryError> {
        let bytes = self.read_len_bytes()?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| TelemetryError::protobuf("metric name is not utf-8"))
    }

    fn read_fixed32(&mut self) -> Result<u32, TelemetryError> {
        if self.remaining() < 4 {
            return Err(TelemetryError::protobuf("truncated fixed32"));
        }
        let mut b = [0_u8; 4];
        b.copy_from_slice(&self.buf[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(b))
    }

    fn skip(&mut self, wire: u32) -> Result<(), TelemetryError> {
        match wire {
            WIRE_VARINT => {
                let _ = self.read_varint()?;
                Ok(())
            }
            1 => {
                if self.remaining() < 8 {
                    return Err(TelemetryError::protobuf("truncated fixed64"));
                }
                self.pos += 8;
                Ok(())
            }
            WIRE_LEN => {
                let _ = self.read_len_bytes()?;
                Ok(())
            }
            WIRE_32 => {
                let _ = self.read_fixed32()?;
                Ok(())
            }
            other => Err(TelemetryError::protobuf(format!(
                "unsupported wire type {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_timestamp_seq_golden() {
        let p = Payload {
            timestamp: Some(1),
            metrics: Vec::new(),
            seq: Some(0),
        };
        assert_eq!(p.encode(), vec![0x08, 0x01, 0x18, 0x00]);
        assert_eq!(Payload::decode(&p.encode()).unwrap(), p);
    }

    #[test]
    fn bool_metric_roundtrip() {
        let mut m = Metric::new(11);
        m.name = Some("a".into());
        m.alias = Some(1);
        m.timestamp = Some(1);
        m.value = Some(MetricValue::Bool(true));
        let p = Payload {
            timestamp: Some(1),
            metrics: vec![m],
            seq: Some(0),
        };
        let bytes = p.encode();
        // Payload ts=1, one metric (11 bytes), seq=0.
        assert_eq!(bytes[0], 0x08);
        assert_eq!(bytes[1], 0x01);
        assert_eq!(bytes[2], 0x12);
        assert_eq!(bytes[3], 11);
        let back = Payload::decode(&bytes).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn rebirth_detects_named_bool() {
        let mut m = Metric::new(11);
        m.name = Some("Node Control/Rebirth".into());
        m.value = Some(MetricValue::Bool(true));
        let p = Payload {
            timestamp: Some(1),
            metrics: vec![m],
            seq: None,
        };
        assert!(is_rebirth_command(&p.encode()));
        assert!(!is_rebirth_command(&[0x08, 0x01]));
    }
}
