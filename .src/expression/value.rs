//! A value an expression writes as a literal, and the kinds there are.
//!
//! **A value is never guessed.** What a name reads is text, as it came off
//! the wire; it is read as the kind the expression gives it — the literal it
//! is compared with, the arithmetic it takes part in — and that coercion is
//! done in the open, with a sentence when the text will not read as that
//! kind. Inferring that "0012345" is the number 12345 loses a leading zero,
//! and an order number with it.

use std::fmt;

/// A literal: text, a whole number or a truth.
///
/// Three kinds deliberately. Decimal is absent until a Contract needs one,
/// because floating point is the wrong answer for money and a wrong decimal
/// is worse than an absent one (`route`'s rule since 2026-08-26, ADR-0046).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

/// The kinds a literal is, in the order a designer offers them.
pub const KINDS: [&str; 3] = [TEXT, INTEGER, BOOLEAN];

pub(crate) const TEXT: &str = "text";
pub(crate) const INTEGER: &str = "integer";
pub(crate) const BOOLEAN: &str = "boolean";

impl Value {
    /// The name of this value's kind, one of [`KINDS`].
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => TEXT,
            Self::Integer(_) => INTEGER,
            Self::Boolean(_) => BOOLEAN,
        }
    }

    /// The value as a person reads it in an explanation, text quoted so an
    /// empty value is still visible. The same as the literal's canonical
    /// spelling, so a reason quotes what the expression says.
    #[must_use]
    pub fn show(&self) -> String {
        self.to_string()
    }

    /// The value written as its kind reads it, unquoted: what a designer's
    /// row holds beside the kind.
    #[must_use]
    pub fn raw(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Integer(number) => number.to_string(),
            Self::Boolean(flag) => flag.to_string(),
        }
    }

    /// `raw` read as `kind`, one of [`KINDS`]: the reverse of
    /// [`Value::raw`], for a designer's row.
    ///
    /// # Errors
    /// The kind is not one of [`KINDS`], or the text does not read as it —
    /// said as a sentence naming `what`.
    pub fn read(what: &str, raw: &str, kind: &str) -> Result<Self, String> {
        match kind {
            TEXT => Ok(Self::Text(raw.to_string())),
            INTEGER => raw
                .trim()
                .parse()
                .map(Self::Integer)
                .map_err(|_| format!("{what} compares with '{raw}', which is not an integer")),
            BOOLEAN => boolean(raw)
                .map(Self::Boolean)
                .ok_or_else(|| format!("{what} compares with '{raw}', which is not true or false")),
            other => Err(format!(
                "{what} compares as '{other}', which is not a kind; one of {}",
                KINDS.join(", ")
            )),
        }
    }
}

/// `true` or `false`, whatever their case and surrounding space.
pub(crate) fn boolean(text: &str) -> Option<bool> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// The literal's canonical spelling: `'text'` with a quote doubled, a
/// number as written, `true` or `false`.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => write!(f, "'{}'", text.replace('\'', "''")),
            Self::Integer(number) => write!(f, "{number}"),
            Self::Boolean(flag) => write!(f, "{flag}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_shows_as_its_literal_and_reads_back_from_its_raw_text() {
        let values = [
            Value::Text("O'Brien".into()),
            Value::Integer(-12),
            Value::Boolean(true),
        ];
        assert_eq!(values[0].show(), "'O''Brien'");
        assert_eq!(values[1].show(), "-12");
        for value in values {
            assert_eq!(
                Value::read("A", &value.raw(), value.kind()).expect("reads"),
                value
            );
        }
        assert_eq!(KINDS, ["text", "integer", "boolean"]);
    }

    #[test]
    fn a_row_that_does_not_read_as_its_kind_is_refused_in_words() {
        assert_eq!(
            Value::read("Amount", "about ten", "integer").expect_err("no"),
            "Amount compares with 'about ten', which is not an integer"
        );
        assert!(Value::read("Urgent", "yes", "boolean").is_err());
        assert!(Value::read("Amount", "1", "decimal").is_err());
        assert_eq!(boolean(" FALSE "), Some(false));
    }
}
