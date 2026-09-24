//! The JSON document every JSON path technology reads and rewrites, shared by
//! dot, JSON Pointer and `JSONPath` (ADR-0044): a Stream parsed once under the
//! `json-schema` descriptor, the bridge between a `serde_json` value and a
//! `StructuredValue`, and the rewrite that produces a new Stream, as ADR-0013
//! asks of anything that changes content. How a language walks the value is
//! each technology's own.

use contract::{ContractDescriptor, ContractError, ContractId, StructuredValue};
use serde_json::Value;
use stream::Stream;
use xcore::StreamId;

/// The descriptor a JSON Stream is read under.
#[must_use]
pub fn descriptor() -> ContractDescriptor {
    ContractDescriptor {
        id: ContractId("json-schema".to_string()),
        version: "1".to_string(),
        representation: "application/json".to_string(),
    }
}

/// `stream` as a value.
///
/// # Errors
/// The Stream is not JSON.
pub fn parse(stream: &Stream) -> Result<Value, ContractError> {
    serde_json::from_slice(stream.bytes())
        .map_err(|error| ContractError::new(format!("not valid JSON: {error}")))
}

/// A JSON Stream, parsed once; every read is a walk after that.
pub struct Document {
    pub descriptor: ContractDescriptor,
    pub value: Value,
}

impl Document {
    /// Parse `stream` once.
    ///
    /// # Errors
    /// The Stream is not JSON.
    pub fn parse(stream: &Stream) -> Result<Self, ContractError> {
        Ok(Self {
            descriptor: descriptor(),
            value: parse(stream)?,
        })
    }
}

/// A JSON Stream being rewritten into a new one.
pub struct Rewrite {
    pub descriptor: ContractDescriptor,
    pub id: StreamId,
    pub value: Value,
}

impl Rewrite {
    /// Start from `stream`; the Stream `finish` produces carries `id`.
    ///
    /// # Errors
    /// The Stream is not JSON.
    pub fn of(stream: &Stream, id: StreamId) -> Result<Self, ContractError> {
        Ok(Self {
            descriptor: descriptor(),
            id,
            value: parse(stream)?,
        })
    }

    /// The rewritten document as a Stream.
    ///
    /// # Errors
    /// The value cannot be serialized.
    pub fn finish(self) -> Result<Stream, ContractError> {
        let bytes = serde_json::to_vec(&self.value)
            .map_err(|error| ContractError::new(format!("cannot serialize JSON: {error}")))?;
        Ok(Stream::new(
            self.id,
            bytes,
            Some(self.descriptor.representation),
        ))
    }
}

/// `value` as the one scalar a promoted property is.
///
/// # Errors
/// An object or an array at `path`, which is refused rather than stringified.
pub fn scalar(value: &Value, path: &str) -> Result<StructuredValue, ContractError> {
    Ok(match value {
        Value::Null => StructuredValue::Null,
        Value::Bool(flag) => StructuredValue::Bool(*flag),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => StructuredValue::Integer(integer),
            None => StructuredValue::Decimal(number.as_f64().unwrap_or(f64::NAN)),
        },
        Value::String(text) => StructuredValue::Text(text.clone()),
        Value::Array(_) | Value::Object(_) => {
            return Err(ContractError::new(format!("{path} is not a scalar")));
        }
    })
}

/// `value` as JSON.
///
/// # Errors
/// Binary, which has no JSON form here.
pub fn from_scalar(value: StructuredValue) -> Result<Value, ContractError> {
    Ok(match value {
        StructuredValue::Null => Value::Null,
        StructuredValue::Bool(flag) => Value::Bool(flag),
        StructuredValue::Integer(integer) => Value::from(integer),
        StructuredValue::Decimal(decimal) => {
            serde_json::Number::from_f64(decimal).map_or(Value::Null, Value::Number)
        }
        StructuredValue::Text(text) => Value::String(text),
        StructuredValue::Binary(_) => {
            return Err(ContractError::new("binary has no JSON form here"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(text: &str) -> Stream {
        Stream::new(StreamId::new(1), text.as_bytes().to_vec(), None)
    }

    #[test]
    fn a_document_parses_once_under_the_json_schema_descriptor() {
        let document = Document::parse(&stream(r#"{"a":1}"#)).expect("json");
        assert_eq!(document.descriptor.id.0, "json-schema");
        assert_eq!(document.value["a"], 1);
        assert!(Document::parse(&stream("{")).is_err());
    }

    #[test]
    fn a_rewrite_finishes_as_a_json_stream_under_the_id_it_was_given() {
        let mut rewrite = Rewrite::of(&stream(r#"{"a":1}"#), StreamId::new(7)).expect("json");
        rewrite.value["a"] = Value::from(2);
        let finished = rewrite.finish().expect("stream");
        assert_eq!(finished.id(), StreamId::new(7));
        assert_eq!(finished.bytes(), br#"{"a":2}"#);
        assert_eq!(finished.media_type(), Some("application/json"));
    }

    #[test]
    fn scalars_cross_both_ways_and_structure_and_binary_are_refused() {
        assert_eq!(
            scalar(&Value::from(2.5), "x").expect("decimal"),
            StructuredValue::Decimal(2.5)
        );
        assert_eq!(
            scalar(&Value::from("t"), "x").expect("text"),
            StructuredValue::Text("t".into())
        );
        assert!(scalar(&serde_json::json!([1]), "x").is_err());
        assert_eq!(
            from_scalar(StructuredValue::Integer(3)).expect("integer"),
            Value::from(3)
        );
        assert_eq!(
            from_scalar(StructuredValue::Decimal(f64::NAN)).expect("nan"),
            Value::Null
        );
        assert!(from_scalar(StructuredValue::Binary(vec![1])).is_err());
    }
}
