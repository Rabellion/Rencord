//! Opt-in dump of the *structure* of gateway events, for checking decoders
//! against what Discord really sends. Set `RENCORD_GATEWAY_SHAPE=<file>`.
//! Only field names, value types and list lengths are written. No value, and
//! no key that looks like an id, ever reaches the file.

use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Write;

/// How many examples of each event name are kept; later ones add nothing.
const PER_EVENT: u8 = 3;

pub fn shape(value: &Value) -> Value {
    match value {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("bool"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
        Value::Array(items) => {
            let item = items.iter().map(shape).reduce(merge).unwrap_or(Value::Null);
            json!({"_len": items.len(), "_item": item})
        }
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, val) in map {
                // Maps keyed by snowflake would leak ids as keys.
                let key = if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
                    "<id>".to_owned()
                } else {
                    key.clone()
                };
                let val = shape(val);
                let merged = match out.remove(&key) {
                    Some(old) => merge(old, val),
                    None => val,
                };
                out.insert(key, merged);
            }
            Value::Object(out)
        }
    }
}

/// Union of two shapes: the keys of both, and "a|b" where the types differ.
fn merge(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Object(mut x), Value::Object(y)) => {
            for (key, val) in y {
                let merged = match x.remove(&key) {
                    Some(old) => merge(old, val),
                    None => val,
                };
                x.insert(key, merged);
            }
            Value::Object(x)
        }
        (Value::Null, other) | (other, Value::Null) => other,
        (a, b) if a == b => a,
        (Value::String(a), Value::String(b)) => {
            let mut parts: Vec<&str> = a.split('|').chain(b.split('|')).collect();
            parts.sort_unstable();
            parts.dedup();
            Value::String(parts.join("|"))
        }
        (a, b) => json!(format!("mixed({a}|{b})")),
    }
}

pub struct ShapeLog {
    file: std::fs::File,
    seen: HashMap<String, u8>,
}

impl ShapeLog {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("RENCORD_GATEWAY_SHAPE").filter(|p| !p.is_empty())?;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(file) => Some(Self {
                file,
                seen: HashMap::new(),
            }),
            Err(err) => {
                tracing::warn!("cannot open the gateway shape file: {err}");
                None
            }
        }
    }

    pub fn record(&mut self, name: &str, data: &Value) {
        let count = self.seen.entry(name.to_owned()).or_default();
        if *count >= PER_EVENT {
            return;
        }
        *count += 1;
        let line = json!({"t": name, "shape": shape(data)});
        if let Err(err) = writeln!(self.file, "{line}") {
            tracing::warn!("cannot write the gateway shape file: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shape_has_names_types_and_lengths_but_no_values() {
        let data = json!({
            "content": "hunter2 secret message",
            "author": {"id": "123456789012345678", "username": "ada"},
            "mentions": [{"id": "1", "bot": true}, {"id": "2", "nick": null}],
            "roles": {"987654321098765432": {"color": 5}},
            "tags": [],
        });
        let text = shape(&data).to_string();
        for leaked in [
            "hunter2",
            "secret",
            "123456789012345678",
            "ada",
            "987654321098765432",
        ] {
            assert!(!text.contains(leaked), "{leaked} leaked into {text}");
        }
        let shaped = shape(&data);
        assert_eq!(shaped["content"], "string");
        assert_eq!(shaped["mentions"]["_len"], 2);
        // Optional fields from different elements are merged.
        assert_eq!(shaped["mentions"]["_item"]["bot"], "bool");
        assert!(shaped["mentions"]["_item"].get("nick").is_some());
        assert_eq!(shaped["roles"]["<id>"]["color"], "number");
        assert_eq!(shaped["tags"]["_len"], 0);
    }

    #[test]
    fn only_the_first_few_of_each_event_are_written() {
        let path = std::env::temp_dir().join(format!("rencord-shape-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let mut log = ShapeLog {
            file,
            seen: HashMap::new(),
        };
        for _ in 0..10 {
            log.record("TYPING_START", &json!({"user_id": "1"}));
        }
        log.record("READY", &json!({}));
        let lines = std::fs::read_to_string(&path).unwrap().lines().count();
        let _ = std::fs::remove_file(&path);
        assert_eq!(lines, usize::from(PER_EVENT) + 1);
    }
}
