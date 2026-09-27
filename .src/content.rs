//! The content a compiled path reads: one Stream, parsed into each form a
//! language asks for once, however many paths read it. `content:dot:a` and
//! `content:jsonpath:$.b` over one Message share one JSON parse; an `XPath`
//! read beside them parses the XML once more, and no more than once.
//!
//! A `Content` lives for one Message on one thread. The forms it holds are
//! the technologies' own types; this knows none of them.

use contract::ContractError;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::rc::Rc;
use stream::Stream;

/// A form a Stream is parsed into — a JSON value, an XML document, the lines
/// of a text — defined by the technology that reads it.
pub trait Form: Sized + 'static {
    /// `stream`, parsed.
    ///
    /// # Errors
    /// The Stream is not in this form.
    fn parse(stream: &Stream) -> Result<Self, ContractError>;
}

/// One form, parsed, or why it could not be. A refusal is kept too, so a
/// Stream that is not JSON is found so once, not once per path.
struct Parsed {
    kind: TypeId,
    form: Result<Rc<dyn Any>, String>,
}

/// One Stream and the forms it has been parsed into so far.
pub struct Content<'s> {
    stream: &'s Stream,
    forms: RefCell<Vec<Parsed>>,
}

impl<'s> Content<'s> {
    /// `stream`, not parsed yet: a form is parsed when a path first asks.
    #[must_use]
    pub const fn of(stream: &'s Stream) -> Self {
        Self {
            stream,
            forms: RefCell::new(Vec::new()),
        }
    }

    /// The Stream itself.
    #[must_use]
    pub const fn stream(&self) -> &'s Stream {
        self.stream
    }

    /// The Stream as UTF-8 text, decoded once by the Stream and borrowed
    /// from it, never copied.
    ///
    /// # Errors
    /// The Stream is not UTF-8.
    pub fn text(&self) -> Result<&'s str, ContractError> {
        self.stream
            .text()
            .map_err(|error| ContractError::new(format!("not UTF-8 text: {error}")))
    }

    /// The Stream as `T`, parsed on the first call and shared by every later
    /// one.
    ///
    /// # Errors
    /// The Stream is not in the form `T`; the same refusal every time.
    pub fn form<T: Form>(&self) -> Result<Rc<T>, ContractError> {
        let kind = TypeId::of::<T>();
        let known = self
            .forms
            .borrow()
            .iter()
            .find(|parsed| parsed.kind == kind)
            .map(|parsed| parsed.form.clone());
        let form = match known {
            Some(form) => form,
            None => {
                let form = T::parse(self.stream)
                    .map(|parsed| Rc::new(parsed) as Rc<dyn Any>)
                    .map_err(|refused| refused.message);
                self.forms.borrow_mut().push(Parsed {
                    kind,
                    form: form.clone(),
                });
                form
            }
        };
        form.map_err(ContractError::new)?
            .downcast::<T>()
            .map_err(|_| ContractError::new("a parsed form is kept under another type"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use xcore::StreamId;

    static PARSED: AtomicUsize = AtomicUsize::new(0);

    /// The Stream's length, counting how often it is parsed.
    struct Length(usize);

    impl Form for Length {
        fn parse(stream: &Stream) -> Result<Self, ContractError> {
            PARSED.fetch_add(1, Ordering::Relaxed);
            if stream.is_empty() {
                return Err(ContractError::new("empty"));
            }
            Ok(Self(stream.len()))
        }
    }

    #[test]
    fn a_form_is_parsed_once_however_often_it_is_asked_for_and_so_is_a_refusal() {
        let stream = Stream::new(StreamId::new(1), b"abc".to_vec(), None);
        let content = Content::of(&stream);
        for _ in 0..100 {
            assert_eq!(content.form::<Length>().expect("parses").0, 3);
        }
        let empty = Stream::new(StreamId::new(2), Vec::new(), None);
        let nothing = Content::of(&empty);
        for _ in 0..100 {
            assert_eq!(
                nothing.form::<Length>().err().map(|e| e.message).as_deref(),
                Some("empty")
            );
        }
        assert_eq!(PARSED.load(Ordering::Relaxed), 2);
        assert!(std::ptr::eq(content.stream(), &stream));
    }

    #[test]
    fn text_is_borrowed_from_the_stream_and_bytes_that_are_not_text_are_refused() {
        let stream = Stream::new(StreamId::new(1), b"order".to_vec(), None);
        let text = Content::of(&stream).text().expect("text");
        assert!(std::ptr::eq(text, stream.text().expect("text")));
        let bytes = Stream::new(StreamId::new(1), vec![0xff, 0xfe], None);
        assert!(
            Content::of(&bytes)
                .text()
                .expect_err("not UTF-8")
                .message
                .starts_with("not UTF-8 text")
        );
    }
}
