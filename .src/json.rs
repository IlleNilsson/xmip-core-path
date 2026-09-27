//! The JSON document every JSON path technology reads and rewrites, shared by
//! dot, JSON Pointer, `JSONPath` and `FHIRPath` (ADR-0044): a Stream parsed
//! into one [`Value`] once per Message ([`crate::Form`]) and opened into one
//! for a rewrite ([`crate::Editable`]), the bridge between a value and a
//! scalar, and the write at a pointer that replaces a value or adds a member.
//! How a language walks the value is each technology's own.

use crate::{Editable, Form};
use contract::ContractError;
use serde_json::Value;
use stream::Stream;
use xcore::ScalarValue;

fn parse(bytes: &[u8]) -> Result<Value, ContractError> {
    serde_json::from_slice(bytes)
        .map_err(|error| ContractError::new(format!("not valid JSON: {error}")))
}

impl Form for Value {
    fn parse(stream: &Stream) -> Result<Self, ContractError> {
        parse(stream.bytes())
    }
}

impl Editable for Value {
    const MEDIA_TYPE: &'static str = "application/json";

    fn open(bytes: &[u8]) -> Result<Self, ContractError> {
        parse(bytes)
    }

    fn into_bytes(self) -> Result<Vec<u8>, ContractError> {
        serde_json::to_vec(&self)
            .map_err(|error| ContractError::new(format!("cannot serialize JSON: {error}")))
    }
}

/// `value` as the one scalar a promoted property is.
///
/// # Errors
/// An object or an array at `path`, which is refused rather than stringified.
pub fn scalar(value: &Value, path: &str) -> Result<ScalarValue, ContractError> {
    Ok(match value {
        Value::Null => ScalarValue::Null,
        Value::Bool(flag) => ScalarValue::Bool(*flag),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => ScalarValue::Integer(integer),
            None => ScalarValue::Decimal(number.as_f64().unwrap_or(f64::NAN)),
        },
        Value::String(text) => ScalarValue::Text(text.clone()),
        Value::Array(_) | Value::Object(_) => {
            return Err(ContractError::new(format!("{path} is not a scalar")));
        }
    })
}

/// `value` as JSON.
///
/// # Errors
/// Binary, which has no JSON form here.
pub fn from_scalar(value: ScalarValue) -> Result<Value, ContractError> {
    Ok(match value {
        ScalarValue::Null => Value::Null,
        ScalarValue::Bool(flag) => Value::Bool(flag),
        ScalarValue::Integer(integer) => Value::from(integer),
        ScalarValue::Decimal(decimal) => {
            serde_json::Number::from_f64(decimal).map_or(Value::Null, Value::Number)
        }
        ScalarValue::Text(text) => Value::String(text),
        ScalarValue::Binary(_) => {
            return Err(ContractError::new("binary has no JSON form here"));
        }
    })
}

/// One RFC 6901 reference token, unescaped: `~1` is `/`, `~0` is `~`.
#[must_use]
pub fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Set `replacement` at `pointer` in `document`: the value there is replaced,
/// or, when the pointer's parent is an object without that member, the
/// member is added. A pointer into nothing is refused: a write names a
/// place, it does not invent structure.
///
/// # Errors
/// `pointer` is not a JSON pointer, or reaches no object to write into.
pub fn set_at_pointer(
    document: &mut Value,
    pointer: &str,
    replacement: Value,
) -> Result<(), ContractError> {
    if let Some(existing) = document.pointer_mut(pointer) {
        *existing = replacement;
        return Ok(());
    }
    let (parent, key) = pointer
        .rsplit_once('/')
        .ok_or_else(|| ContractError::new(format!("{pointer:?} is not a JSON pointer")))?;
    match document.pointer_mut(parent) {
        Some(Value::Object(members)) => {
            members.insert(unescape(key), replacement);
            Ok(())
        }
        _ => Err(ContractError::new(format!(
            "{pointer:?} has no object to write into"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Content, Rewriting};
    use xcore::StreamId;

    fn stream(text: &str) -> Stream {
        Stream::new(StreamId::new(1), text.as_bytes().to_vec(), None)
    }

    #[test]
    fn a_json_stream_parses_once_into_one_value_and_other_bytes_are_refused() {
        let order = stream(r#"{"a":1}"#);
        let content = Content::of(&order);
        let first = content.form::<Value>().expect("json");
        assert_eq!(first["a"], 1);
        assert!(std::rc::Rc::ptr_eq(
            &first,
            &content.form::<Value>().expect("json")
        ));
        let broken = stream("{");
        assert!(
            Content::of(&broken)
                .form::<Value>()
                .expect_err("not JSON")
                .message
                .starts_with("not valid JSON")
        );
    }

    #[test]
    fn a_rewrite_finishes_as_a_json_stream_under_the_id_it_was_given() {
        let mut rewriting = Rewriting::of(&stream(r#"{"a":1}"#), StreamId::new(7));
        rewriting.form_mut::<Value>().expect("json")["a"] = Value::from(2);
        let finished = rewriting.finish().expect("stream");
        assert_eq!(finished.id(), StreamId::new(7));
        assert_eq!(finished.bytes(), br#"{"a":2}"#);
        assert_eq!(finished.media_type(), Some("application/json"));
    }

    #[test]
    fn scalars_cross_both_ways_and_structure_and_binary_are_refused() {
        assert_eq!(
            scalar(&Value::from(2.5), "x").expect("decimal"),
            ScalarValue::Decimal(2.5)
        );
        assert_eq!(
            scalar(&Value::from("t"), "x").expect("text"),
            ScalarValue::Text("t".into())
        );
        assert!(scalar(&serde_json::json!([1]), "x").is_err());
        assert_eq!(
            from_scalar(ScalarValue::Integer(3)).expect("integer"),
            Value::from(3)
        );
        assert_eq!(
            from_scalar(ScalarValue::Decimal(f64::NAN)).expect("nan"),
            Value::Null
        );
        assert!(from_scalar(ScalarValue::Binary(vec![1])).is_err());
    }

    #[test]
    fn a_pointer_write_replaces_or_adds_a_member_and_refuses_nowhere() {
        let mut document = serde_json::json!({"a": {"b": 1}, "list": [1]});
        set_at_pointer(&mut document, "/a/b", Value::from(2)).expect("replaces");
        set_at_pointer(&mut document, "/a/c~1d~0e", Value::from(3)).expect("adds");
        assert_eq!(document["a"], serde_json::json!({"b": 2, "c/d~e": 3}));
        assert!(set_at_pointer(&mut document, "/list/5", Value::Null).is_err());
        assert!(set_at_pointer(&mut document, "/nowhere/deep", Value::Null).is_err());
        assert!(set_at_pointer(&mut document, "a", Value::Null).is_err());
        assert_eq!(unescape("~01~1"), "~1/");
    }
}
