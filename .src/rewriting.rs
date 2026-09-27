//! Content being rewritten into a new Stream, as ADR-0013 asks of anything
//! that changes content: the Stream is opened once into the form a language
//! edits, every write lands in it, and it is written back once, at the end.
//! Two languages of one form — dot and `JSONPath`, regex and index — edit
//! the same open form; a write in another form closes the first into bytes
//! and opens those, so a later write sees an earlier one either way.

use contract::ContractError;
use std::any::Any;
use stream::Stream;
use xcore::StreamId;

/// A form content is edited in, defined by the technology that edits it.
pub trait Editable: Sized + 'static {
    /// The media type a rewritten Stream carries when its source named none.
    const MEDIA_TYPE: &'static str;

    /// `bytes`, opened for editing.
    ///
    /// # Errors
    /// The bytes are not in this form.
    fn open(bytes: &[u8]) -> Result<Self, ContractError>;

    /// The edited content as bytes.
    ///
    /// # Errors
    /// The form cannot be written.
    fn into_bytes(self) -> Result<Vec<u8>, ContractError>;
}

/// An open form, whichever it is.
trait Open: Any {
    fn close(self: Box<Self>) -> Result<Vec<u8>, ContractError>;
    fn media_type(&self) -> &'static str;
}

impl<T: Editable> Open for T {
    fn close(self: Box<Self>) -> Result<Vec<u8>, ContractError> {
        (*self).into_bytes()
    }

    fn media_type(&self) -> &'static str {
        T::MEDIA_TYPE
    }
}

/// A Stream being rewritten into a new one.
pub struct Rewriting {
    id: StreamId,
    source: Stream,
    open: Option<Box<dyn Open>>,
}

impl Rewriting {
    /// Start from `stream`; the Stream [`Rewriting::finish`] produces carries
    /// `id`. Nothing is parsed until a write asks for a form.
    #[must_use]
    pub fn of(stream: &Stream, id: StreamId) -> Self {
        Self {
            id,
            source: stream.clone(),
            open: None,
        }
    }

    /// The content open as `T`: opened on the first write in that form, and
    /// the same one for every write after it.
    ///
    /// # Errors
    /// The content is not in the form `T`, or the form open before it could
    /// not be written back.
    pub fn form_mut<T: Editable>(&mut self) -> Result<&mut T, ContractError> {
        let open_as_t = self
            .open
            .as_deref()
            .is_some_and(|open| (open as &dyn Any).is::<T>());
        if !open_as_t {
            if let Some(open) = self.open.take() {
                self.source = self.written(open)?;
            }
            self.open = Some(Box::new(T::open(self.source.bytes())?));
        }
        self.open
            .as_deref_mut()
            .and_then(|open| (open as &mut dyn Any).downcast_mut::<T>())
            .ok_or_else(|| ContractError::new("the open form is kept under another type"))
    }

    /// The rewritten content as a Stream under the id it was given, in the
    /// source's media type, or the form's when the source named none.
    ///
    /// # Errors
    /// The open form cannot be written back.
    pub fn finish(mut self) -> Result<Stream, ContractError> {
        match self.open.take() {
            Some(open) => self.written(open),
            None => Ok(Stream::new(
                self.id,
                self.source.bytes().to_vec(),
                self.source.media_type().map(str::to_string),
            )),
        }
    }

    fn written(&self, open: Box<dyn Open>) -> Result<Stream, ContractError> {
        let media_type = self
            .source
            .media_type()
            .unwrap_or_else(|| open.media_type())
            .to_string();
        Ok(Stream::new(self.id, open.close()?, Some(media_type)))
    }
}

/// UTF-8 text, edited in place: the form the text languages write in.
impl Editable for String {
    const MEDIA_TYPE: &'static str = "text/plain";

    fn open(bytes: &[u8]) -> Result<Self, ContractError> {
        std::str::from_utf8(bytes)
            .map(str::to_string)
            .map_err(|error| ContractError::new(format!("not UTF-8 text: {error}")))
    }

    fn into_bytes(self) -> Result<Vec<u8>, ContractError> {
        Ok(Self::into_bytes(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn stream(text: &str, media_type: Option<&str>) -> Stream {
        Stream::new(
            StreamId::new(1),
            text.as_bytes().to_vec(),
            media_type.map(str::to_string),
        )
    }

    #[test]
    fn nothing_written_is_the_source_under_the_new_id() {
        let finished = Rewriting::of(&stream("a", Some("text/csv")), StreamId::new(2))
            .finish()
            .expect("finishes");
        assert_eq!(finished.id(), StreamId::new(2));
        assert_eq!(finished.bytes(), b"a");
        assert_eq!(finished.media_type(), Some("text/csv"));
    }

    #[test]
    fn writes_in_one_form_share_it_and_another_form_sees_them() {
        let mut rewriting = Rewriting::of(&stream(r#"{"a":"x"}"#, None), StreamId::new(3));
        rewriting.form_mut::<Value>().expect("json")["a"] = Value::from("y");
        rewriting.form_mut::<Value>().expect("json")["b"] = Value::from(1);
        let text = rewriting.form_mut::<String>().expect("text");
        assert_eq!(text, r#"{"a":"y","b":1}"#);
        text.push(' ');
        assert_eq!(
            rewriting.form_mut::<Value>().expect("json again")["a"],
            Value::from("y")
        );
        let finished = rewriting.finish().expect("finishes");
        assert_eq!(finished.bytes(), br#"{"a":"y","b":1}"#);
        assert_eq!(finished.media_type(), Some("application/json"));
    }

    #[test]
    fn content_not_in_the_form_is_refused() {
        let mut rewriting = Rewriting::of(&stream("{nope", None), StreamId::new(1));
        assert!(rewriting.form_mut::<Value>().is_err());
        let bytes = Stream::new(StreamId::new(1), vec![0xff], None);
        assert!(
            Rewriting::of(&bytes, StreamId::new(1))
                .form_mut::<String>()
                .is_err()
        );
    }
}
