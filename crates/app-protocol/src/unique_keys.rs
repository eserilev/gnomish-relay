//! A JSON parse that refuses a key given twice in any object. A parse into a
//! `serde_json::Value` keeps the last value, so a second `id` would pass unseen.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

/// `None` for text that is not JSON, or that gives a key twice in any object.
pub(crate) fn parse(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice::<UniqueKeys>(bytes)
        .ok()
        .map(|v| v.0)
}

struct UniqueKeys(Value);

impl<'de> Deserialize<'de> for UniqueKeys {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_any(UniqueKeysVisitor)
            .map(UniqueKeys)
    }
}

struct UniqueKeysVisitor;

impl<'de> Visitor<'de> for UniqueKeysVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON with no key twice in an object")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(UniqueKeys(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format!("the key {key} is there twice")));
            }
            let UniqueKeys(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_with_unique_keys_reads_as_serde_json_reads_it() {
        let text = br#"{"a":[1,-2,2.5,"x",true,null,{"b":{}}],"c":"d"}"#;

        assert_eq!(parse(text), serde_json::from_slice(text).ok());
    }

    #[test]
    fn a_key_twice_at_any_depth_is_refused() {
        for text in [
            r#"{"a":1,"a":1}"#,
            r#"{"a":{"b":1,"b":2}}"#,
            r#"{"a":[{"b":1,"b":2}]}"#,
        ] {
            assert_eq!(parse(text.as_bytes()), None, "{text}");
        }
    }

    /// The check compares keys after their escapes, as a reader of the value sees them.
    #[test]
    fn a_key_and_the_same_key_with_an_escape_count_as_one_key() {
        assert_eq!(parse(br#"{"a":1,"\u0061":2}"#), None);
    }

    #[test]
    fn the_same_key_in_two_objects_is_fine() {
        assert!(parse(br#"{"a":{"k":1},"b":{"k":2}}"#).is_some());
    }

    #[test]
    fn text_that_is_not_json_is_refused() {
        assert_eq!(parse(b"{\"a\":"), None);
    }
}
