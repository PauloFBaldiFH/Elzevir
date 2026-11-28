// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded physical records; keys are extracted from one decoded object at a time.
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;
use std::io::{BufRead, Read};

use super::{Encoding, Position};

const LINE_BYTES: u64 = 256 * 1024;
pub(super) const ROW_LIMIT: u64 = 200_000;

pub(super) fn scan(
    mut reader: impl BufRead,
    member: usize,
    encoding: Encoding,
    mut visit: impl FnMut(Position, Option<&str>, &Value) -> Result<(), String>,
) -> Result<(), String> {
    let mut row = Position { line: 1, offset: 0 };
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let size = (&mut reader)
            .take(LINE_BYTES + 1)
            .read_until(b'\n', &mut buf)
            .map_err(|_| format!("member {member}: source read or digest verification failed"))?;
        if size == 0 {
            return Ok(());
        }
        let at = |reason: &str| {
            format!(
                "member {member}, line {}, byte {}: {reason}",
                row.line, row.offset
            )
        };
        if size as u64 > LINE_BYTES || row.line > ROW_LIMIT {
            return Err(at("relation reader limit exceeded"));
        }
        let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
        let text = std::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line))
            .map_err(|_| at("expected UTF-8"))?;
        let (envelope, json) = match encoding {
            Encoding::JsonLines => (None, text),
            Encoding::TsvJson => {
                let (id, rest) = text
                    .split_once('\t')
                    .ok_or_else(|| at("expected TSV identifier and JSON object"))?;
                (
                    Some(id),
                    rest.split_once('\t').map_or(rest, |(object, _)| object),
                )
            }
        };
        let Json(value) =
            serde_json::from_str(json).map_err(|_| at("invalid JSON or duplicate property"))?;
        if !value.is_object() {
            return Err(at("expected a JSON object"));
        }
        visit(row, envelope, &value).map_err(|reason| at(&reason))?;
        row.line += 1;
        row.offset += size as u64;
    }
}

// serde_json::Value accepts the last occurrence of a property. Import identities
// must never depend on which occurrence a particular decoder chooses.
struct Json(Value);
impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(JsonVisitor).map(Json)
    }
}
struct JsonVisitor;
impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON value")
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut array = Vec::new();
        while let Some(Json(v)) = seq.next_element()? {
            array.push(v);
        }
        Ok(Value::Array(array))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom("duplicate property"));
            }
            let Json(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}
