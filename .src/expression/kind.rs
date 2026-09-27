//! The kinds an expression's parts are, checked once when it is compiled —
//! so a comparison of a number with text, or arithmetic on text, is refused
//! when the configuration loads, not when a Message arrives (ADR-0066
//! clause 1).
//!
//! A name has no kind of its own: what it reads is text off the wire, and it
//! takes the kind of what it meets — the literal it is compared with, the
//! arithmetic it is part of. Only parts whose kind is known can disagree.

use contract::ContractError;

use super::condition::Condition;
use super::lexer::error;
use super::operand::{Operand, Operator};
use super::value::Value;

/// What a part of an expression is known to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Integer,
    Boolean,
    /// A name, or what is made of names alone: read as whatever it meets.
    Unknown,
}

impl Kind {
    const fn of(value: &Value) -> Self {
        match value {
            Value::Text(_) => Self::Text,
            Value::Integer(_) => Self::Integer,
            Value::Boolean(_) => Self::Boolean,
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "an integer",
            Self::Boolean => "a boolean",
            Self::Unknown => "a name",
        }
    }

    /// The kind two parts share, when they can meet.
    fn meet(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Unknown, known) | (known, Self::Unknown) => Some(known),
            (left, right) if left == right => Some(left),
            _ => None,
        }
    }
}

/// Check every part of `condition` against what meets it.
///
/// # Errors
/// Two parts whose kinds cannot meet, arithmetic on what is not an integer,
/// `like` on what is not text, or an order asked of booleans — each said
/// with the part it is about.
pub fn check(condition: &Condition) -> Result<(), ContractError> {
    match condition {
        Condition::All(parts) | Condition::Any(parts) => parts.iter().try_for_each(check),
        Condition::Not(inner) => check(inner),
        Condition::Compare {
            left,
            comparison,
            right,
        } => {
            let kind = meet(condition, kind(left)?, kind(right)?)?;
            if comparison.orders() && kind == Kind::Boolean {
                return Err(error(format!("{condition}: a boolean has no order")));
            }
            Ok(())
        }
        Condition::Like { value, pattern, .. } => {
            textual(condition, kind(value)?)?;
            textual(condition, kind(pattern)?)
        }
        Condition::In { value, list, .. } => {
            let mut shared = kind(value)?;
            for item in list {
                shared = meet(condition, shared, kind(item)?)?;
            }
            Ok(())
        }
        Condition::Exists(value) => kind(value).map(|_| ()),
    }
}

fn meet(part: &impl std::fmt::Display, left: Kind, right: Kind) -> Result<Kind, ContractError> {
    left.meet(right).ok_or_else(|| {
        error(format!(
            "{part}: {} is compared with {}",
            left.word(),
            right.word()
        ))
    })
}

fn textual(part: &Condition, kind: Kind) -> Result<(), ContractError> {
    match kind {
        Kind::Text | Kind::Unknown => Ok(()),
        other => Err(error(format!(
            "{part}: like reads text, not {}",
            other.word()
        ))),
    }
}

fn kind(operand: &Operand) -> Result<Kind, ContractError> {
    match operand {
        Operand::Name(_) => Ok(Kind::Unknown),
        Operand::Literal(value) => Ok(Kind::of(value)),
        Operand::Negate(inner) => integer(operand, kind(inner)?),
        Operand::Binary {
            left,
            operator: Operator::Concat,
            right,
        } => {
            kind(left)?;
            kind(right)?;
            Ok(Kind::Text)
        }
        Operand::Binary { left, right, .. } => {
            integer(operand, kind(left)?)?;
            integer(operand, kind(right)?)
        }
        Operand::Coalesce(parts) => {
            let mut shared = Kind::Unknown;
            for part in parts {
                shared = meet(operand, shared, kind(part)?)?;
            }
            Ok(shared)
        }
    }
}

fn integer(part: &Operand, kind: Kind) -> Result<Kind, ContractError> {
    match kind {
        Kind::Integer | Kind::Unknown => Ok(Kind::Integer),
        other => Err(error(format!(
            "{part}: arithmetic is over integers, not {}",
            other.word()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::parser;

    fn checked(text: &str) -> Result<(), String> {
        check(&parser::condition(text).expect("parses")).map_err(|refused| refused.message)
    }

    #[test]
    fn a_name_takes_the_kind_of_what_it_meets() {
        for sound in [
            "Amount > 1000",
            "Urgent = true",
            "MessageType = 'Order'",
            "A = B",
            "Amount + 1 > Limit",
            "Id || '-' || 1 = 'x-1'",
            "coalesce(A, B, 'none') = 'none'",
            "Region in ('EU', 'US', Home)",
            "Customer like 'EU%' and exists Note",
            "-Amount < 0",
        ] {
            assert_eq!(checked(sound), Ok(()), "{sound}");
        }
    }

    #[test]
    fn parts_that_cannot_meet_are_refused_with_the_part() {
        assert_eq!(
            checked("1 = 'x'"),
            Err("expression: 1 = 'x': an integer is compared with text".to_string())
        );
        assert!(
            checked("Amount + 'x' > 1")
                .expect_err("text")
                .contains("arithmetic")
        );
        assert!(checked("-'x' = A").is_err());
        assert!(checked("A || B + 1 = 1").is_err());
        assert!(
            checked("Urgent > true")
                .expect_err("order")
                .contains("no order")
        );
        assert!(
            checked("A like 1")
                .expect_err("like")
                .contains("like reads text")
        );
        assert!(checked("A in ('x', 1)").is_err());
        assert!(checked("coalesce(A, 1) = 'x'").is_err());
        assert!(checked("coalesce(1, 'x') = A").is_err());
        assert!(checked("a = 1 and (b = 'x' or 2 = true)").is_err());
    }
}
