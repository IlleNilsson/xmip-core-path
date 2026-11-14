//! An operand read against one set of names: the value it has, or the
//! reason it has none.
//!
//! A value that is not there is *unknown*, SQL's third truth, and carries
//! its reason — "nothing promoted Region", "Amount is 'about ten', which is
//! not an integer" — so a condition over it neither holds nor silently
//! fails. What a name reads is text; it is read as the kind of what it
//! meets, here and only here (see [`super::value`]).

use std::borrow::Cow;
use std::cmp::Ordering;

use super::Names;
use super::operand::{Operand, Operator};
use super::value::{Value, boolean};

/// The reason an operand has no value.
pub(crate) type Unknown = String;

/// What an operand read.
#[derive(Clone, Debug)]
pub(crate) enum Reading<'a> {
    /// A name's text, not yet read as any kind; the name says whose.
    Raw {
        name: &'a str,
        text: &'a str,
    },
    Text(Cow<'a, str>),
    Integer(i64),
    Boolean(bool),
}

/// Two readings settled on one kind, for a comparison.
pub(crate) enum Settled<'a> {
    Text(Cow<'a, str>, Cow<'a, str>),
    Integer(i64, i64),
    Boolean(bool, bool),
}

/// Read `operand`, borrowing what it can from the expression and the names.
pub(crate) fn read<'a>(operand: &'a Operand, names: &'a dyn Names) -> Result<Reading<'a>, Unknown> {
    match operand {
        Operand::Name(name) => names
            .value(name)
            .map(|text| Reading::Raw { name, text })
            .ok_or_else(|| names.absent(name)),
        Operand::Literal(Value::Text(text)) => Ok(Reading::Text(Cow::Borrowed(text))),
        Operand::Literal(Value::Integer(number)) => Ok(Reading::Integer(*number)),
        Operand::Literal(Value::Boolean(flag)) => Ok(Reading::Boolean(*flag)),
        Operand::Negate(inner) => read(inner, names)?
            .integer()?
            .checked_neg()
            .map(Reading::Integer)
            .ok_or_else(|| format!("{operand} overflows")),
        Operand::Binary {
            left,
            operator: Operator::Concat,
            right,
        } => {
            let mut joined = read(left, names)?.text().into_owned();
            joined.push_str(&read(right, names)?.text());
            Ok(Reading::Text(Cow::Owned(joined)))
        }
        Operand::Binary {
            left,
            operator,
            right,
        } => arithmetic(
            operand,
            *operator,
            &read(left, names)?,
            &read(right, names)?,
        ),
        Operand::Coalesce(parts) => {
            let mut reasons = Vec::new();
            for part in parts {
                match read(part, names) {
                    Ok(reading) => return Ok(reading),
                    Err(reason) => reasons.push(reason),
                }
            }
            Err(reasons.join("; and "))
        }
    }
}

fn arithmetic<'a>(
    operand: &Operand,
    operator: Operator,
    left: &Reading<'_>,
    right: &Reading<'_>,
) -> Result<Reading<'a>, Unknown> {
    let (left, right) = (left.integer()?, right.integer()?);
    let result = match operator {
        Operator::Add => left.checked_add(right),
        Operator::Subtract => left.checked_sub(right),
        Operator::Multiply => left.checked_mul(right),
        Operator::Divide if right == 0 => return Err(format!("{operand} divides by zero")),
        Operator::Divide => left.checked_div(right),
        Operator::Concat => None,
    };
    result
        .map(Reading::Integer)
        .ok_or_else(|| format!("{operand} overflows"))
}

impl<'a> Reading<'a> {
    /// Read as an integer; a name's text that is not one is unknown, said.
    pub(crate) fn integer(&self) -> Result<i64, Unknown> {
        match self {
            Self::Integer(number) => Ok(*number),
            Self::Raw { name, text } => text
                .trim()
                .parse()
                .map_err(|_| format!("{name} is '{text}', which is not an integer")),
            other => Err(format!("{} is not an integer", other.show())),
        }
    }

    fn boolean(&self) -> Result<bool, Unknown> {
        match self {
            Self::Boolean(flag) => Ok(*flag),
            Self::Raw { name, text } => boolean(text)
                .ok_or_else(|| format!("{name} is '{text}', which is not true or false")),
            other => Err(format!("{} is not true or false", other.show())),
        }
    }

    /// Read as text: a name's as it is, a number and a truth as written.
    pub(crate) fn text(&self) -> Cow<'a, str> {
        match self {
            Self::Raw { text, .. } => Cow::Borrowed(text),
            Self::Text(text) => text.clone(),
            Self::Integer(number) => Cow::Owned(number.to_string()),
            Self::Boolean(flag) => Cow::Owned(flag.to_string()),
        }
    }

    /// As an explanation shows it: text quoted, the rest as written.
    pub(crate) fn show(&self) -> String {
        match self {
            Self::Raw { text, .. } => quoted(text),
            Self::Text(text) => quoted(text),
            Self::Integer(number) => number.to_string(),
            Self::Boolean(flag) => flag.to_string(),
        }
    }

    /// This and `other` read as one kind: a name's text as the kind of what
    /// it meets, two names' as text.
    pub(crate) fn settle(self, other: Self) -> Result<Settled<'a>, Unknown> {
        use Reading::{Boolean, Integer, Raw, Text};
        Ok(match (self, other) {
            (Integer(left), right @ (Raw { .. } | Integer(_))) => {
                Settled::Integer(left, right.integer()?)
            }
            (left @ Raw { .. }, Integer(right)) => Settled::Integer(left.integer()?, right),
            (Boolean(left), right @ (Raw { .. } | Boolean(_))) => {
                Settled::Boolean(left, right.boolean()?)
            }
            (left @ Raw { .. }, Boolean(right)) => Settled::Boolean(left.boolean()?, right),
            (left @ (Raw { .. } | Text(_)), right @ (Raw { .. } | Text(_))) => {
                Settled::Text(left.text(), right.text())
            }
            (left, right) => {
                return Err(format!(
                    "{} cannot be compared with {}",
                    left.show(),
                    right.show()
                ));
            }
        })
    }
}

impl Settled<'_> {
    pub(crate) fn order(&self) -> Ordering {
        match self {
            Self::Text(left, right) => left.cmp(right),
            Self::Integer(left, right) => left.cmp(right),
            Self::Boolean(left, right) => left.cmp(right),
        }
    }

    /// Both sides as an explanation shows them.
    pub(crate) fn shown(&self) -> (String, String) {
        match self {
            Self::Text(left, right) => (quoted(left), quoted(right)),
            Self::Integer(left, right) => (left.to_string(), right.to_string()),
            Self::Boolean(left, right) => (left.to_string(), right.to_string()),
        }
    }
}

fn quoted(text: &str) -> String {
    Value::Text(text.to_string()).show()
}

/// One piece of a `like` pattern.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece {
    /// `%`: any run of characters, none included.
    Run,
    /// `_`: exactly one character.
    One,
    /// A character that matches only itself.
    Itself(char),
}

/// The pieces of `pattern`: `%` a run, `_` one character, and, where an
/// `escape` is given, the escape before `%`, `_` or itself that character
/// as itself (SQL's `ESCAPE` clause).
///
/// # Errors
/// The escape stands before anything else, or at the end: SQL's invalid
/// escape sequence, said in words.
pub(crate) fn pieces(pattern: &str, escape: Option<char>) -> Result<Vec<Piece>, Unknown> {
    let mut pieces = Vec::with_capacity(pattern.len());
    let mut characters = pattern.chars();
    while let Some(character) = characters.next() {
        pieces.push(match character {
            found if Some(found) == escape => match characters.next() {
                Some(next) if next == '%' || next == '_' || Some(next) == escape => {
                    Piece::Itself(next)
                }
                _ => {
                    return Err(format!(
                        "{} escapes nothing where it stands in {}: it escapes %, _ or itself",
                        quoted(&found.to_string()),
                        quoted(pattern)
                    ));
                }
            },
            '%' => Piece::Run,
            '_' => Piece::One,
            other => Piece::Itself(other),
        });
    }
    Ok(pieces)
}

/// Whether `text` matches the `like` pattern, as [`pieces`] reads it.
/// Linear in the text for a pattern with one `%`, and never worse than the
/// two lengths multiplied.
///
/// # Errors
/// As [`pieces`].
pub(crate) fn like(text: &str, pattern: &str, escape: Option<char>) -> Result<bool, Unknown> {
    let pattern = pieces(pattern, escape)?;
    let text: Vec<char> = text.chars().collect();
    let (mut at, mut from) = (0, 0);
    let mut retry: Option<(usize, usize)> = None;
    while at < text.len() {
        match pattern.get(from) {
            Some(Piece::Run) => {
                from += 1;
                retry = Some((from, at));
            }
            Some(Piece::One) => {
                at += 1;
                from += 1;
            }
            Some(&Piece::Itself(wanted)) if wanted == text[at] => {
                at += 1;
                from += 1;
            }
            _ => match retry {
                Some((after, start)) => {
                    from = after;
                    at = start + 1;
                    retry = Some((after, at));
                }
                None => return Ok(false),
            },
        }
    }
    Ok(pattern[from..].iter().all(|&rest| rest == Piece::Run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::parser::operand;
    use std::collections::BTreeMap;

    struct Map(BTreeMap<&'static str, &'static str>);

    impl Names for Map {
        fn value(&self, name: &str) -> Option<&str> {
            self.0.get(name).copied()
        }
    }

    fn names() -> Map {
        Map(BTreeMap::from([
            ("First", "Ada"),
            ("Last", "Lovelace"),
            ("Amount", "1500"),
            ("Words", "about ten"),
            ("Big", "9223372036854775807"),
        ]))
    }

    fn value(text: &str) -> Result<String, Unknown> {
        let parsed = operand(text).expect("parses");
        let names = names();
        read(&parsed, &names).map(|reading| reading.show())
    }

    #[test]
    fn values_are_computed_from_names_and_literals() {
        assert_eq!(value("First || ' ' || Last"), Ok("'Ada Lovelace'".into()));
        assert_eq!(value("Amount + 1 * 2"), Ok("1502".into()));
        assert_eq!(value("Amount / 7 - -1"), Ok("215".into()));
        assert_eq!(value("-Amount"), Ok("-1500".into()));
        assert_eq!(value("coalesce(Middle, First)"), Ok("'Ada'".into()));
        assert_eq!(value("'n' || 1 || true"), Ok("'n1true'".into()));
    }

    #[test]
    fn a_value_that_is_not_there_is_unknown_with_its_reason() {
        assert_eq!(value("Middle"), Err("Middle has no value".into()));
        assert_eq!(value("First || Middle"), Err("Middle has no value".into()));
        assert_eq!(
            value("Words + 1"),
            Err("Words is 'about ten', which is not an integer".into())
        );
        assert_eq!(
            value("Amount / 0"),
            Err("Amount / 0 divides by zero".into())
        );
        assert_eq!(value("Big + 1"), Err("Big + 1 overflows".into()));
        assert_eq!(
            value("coalesce(Middle, Nick)"),
            Err("Middle has no value; and Nick has no value".into())
        );
    }

    #[test]
    fn a_name_is_read_as_the_kind_it_meets() {
        let amount = || Reading::Raw {
            name: "Amount",
            text: " 0012 ",
        };
        assert_eq!(
            amount()
                .settle(Reading::Integer(12))
                .expect("reads")
                .shown()
                .0,
            "12"
        );
        let as_text = amount().settle(Reading::Text("x".into())).expect("reads");
        assert_eq!(as_text.shown().0, "' 0012 '");
        assert!(amount().settle(Reading::Boolean(true)).is_err());
        assert!(
            Reading::Text("x".into())
                .settle(Reading::Integer(1))
                .is_err()
        );
    }

    #[test]
    fn like_matches_runs_and_single_characters() {
        for (text, pattern, matched) in [
            ("EU-0042", "EU%", true),
            ("EU-0042", "%42", true),
            ("EU-0042", "%-00%", true),
            ("EU-0042", "EU-00_2", true),
            ("EU-0042", "EU_", false),
            ("", "%", true),
            ("", "_", false),
            ("aaab", "%a%ab", true),
            ("naïve", "na_ve", true),
            ("abc", "abc", true),
            ("abc", "abd", false),
        ] {
            assert_eq!(
                like(text, pattern, None),
                Ok(matched),
                "{text} like {pattern}"
            );
        }
    }

    #[test]
    fn an_escaped_wildcard_matches_only_itself() {
        for (text, pattern, matched) in [
            ("a_b-1", r"a\_b%", true),
            ("axb-1", r"a\_b%", false),
            ("100%", r"100\%", true),
            ("1000", r"100\%", false),
            (r"C:\in", r"C:\\in", true),
            ("a_b-1", "a_b%", true),
            ("axb-1", "a_b%", true),
        ] {
            let escape = pattern.contains('\\').then_some('\\');
            assert_eq!(
                like(text, pattern, escape),
                Ok(matched),
                "{text} like {pattern}"
            );
        }
        assert_eq!(like("a!_b", "a!!!_b", Some('!')), Ok(true));
        let refused = like("ab", r"a\b", Some('\\')).expect_err("escapes nothing");
        assert_eq!(
            refused,
            r"'\' escapes nothing where it stands in 'a\b': it escapes %, _ or itself"
        );
        assert!(like("a", r"a\", Some('\\')).is_err(), "at the end");
    }
}
