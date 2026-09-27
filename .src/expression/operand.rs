//! What a condition compares: a name, a literal, or a value computed from
//! them — arithmetic, `||` and `coalesce` — and its one canonical spelling.
//!
//! Precedence, loosest first: `||`, then `+` and `-`, then `*` and `/`, then
//! a unary minus. Each binary operator groups to the left, so the printer
//! puts parentheses around a right operand of the same precedence and
//! nowhere else: `a - (b - c)` keeps them, `(a - b) - c` is `a - b - c`.

use std::fmt;

use super::lexer::{Token, tokenize, word_character};
use super::value::Value;

/// A value in an expression, parsed once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operand {
    /// A name another reader answers: bare, prefixed (`header:http.x-channel`,
    /// `party:sender`) or in double quotes.
    Name(String),
    /// A literal, of the kind its spelling says.
    Literal(Value),
    /// `-x`, over an integer.
    Negate(Box<Operand>),
    /// `left <operator> right`.
    Binary {
        left: Box<Operand>,
        operator: Operator,
        right: Box<Operand>,
    },
    /// `coalesce(a, b, …)`: the first that has a value.
    Coalesce(Vec<Operand>),
}

/// The binary operators over values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    /// `||`, text joined.
    Concat,
    Add,
    Subtract,
    Multiply,
    /// `/`, integer division, rounding toward zero.
    Divide,
}

impl Operator {
    /// How the operator is written.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Concat => "||",
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
        }
    }

    /// How tightly it binds: higher binds tighter.
    pub(crate) const fn precedence(self) -> u8 {
        match self {
            Self::Concat => 1,
            Self::Add | Self::Subtract => 2,
            Self::Multiply | Self::Divide => 3,
        }
    }
}

/// The precedence of a unary minus and of anything that needs no
/// parentheses at all.
const UNARY: u8 = 4;
const PRIMARY: u8 = 5;

/// The words a bare name may not be, because the grammar reads them.
pub const KEYWORDS: [&str; 11] = [
    "and", "or", "not", "in", "like", "is", "null", "exists", "true", "false", "coalesce",
];

impl Operand {
    /// Every name this operand reads, in the order written.
    pub(crate) fn collect_names<'a>(&'a self, into: &mut Vec<&'a str>) {
        match self {
            Self::Name(name) => into.push(name),
            Self::Literal(_) => {}
            Self::Negate(inner) => inner.collect_names(into),
            Self::Binary { left, right, .. } => {
                left.collect_names(into);
                right.collect_names(into);
            }
            Self::Coalesce(parts) => {
                for part in parts {
                    part.collect_names(into);
                }
            }
        }
    }

    const fn precedence(&self) -> u8 {
        match self {
            Self::Binary { operator, .. } => operator.precedence(),
            Self::Negate(_) => UNARY,
            Self::Literal(Value::Integer(number)) if *number < 0 => UNARY,
            _ => PRIMARY,
        }
    }

    /// Written at a place that needs at least `wanted` precedence.
    fn write_at(&self, f: &mut fmt::Formatter<'_>, wanted: u8) -> fmt::Result {
        if self.precedence() < wanted {
            write!(f, "({self})")
        } else {
            write!(f, "{self}")
        }
    }
}

/// The canonical spelling: one space around a binary operator, a name bare
/// unless it needs quotes, parentheses only where precedence needs them.
impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => write_name(f, name),
            Self::Literal(value) => write!(f, "{value}"),
            Self::Negate(inner) => {
                f.write_str("-")?;
                inner.write_at(f, PRIMARY)
            }
            Self::Binary {
                left,
                operator,
                right,
            } => {
                left.write_at(f, operator.precedence())?;
                write!(f, " {} ", operator.spelling())?;
                right.write_at(f, operator.precedence() + 1)
            }
            Self::Coalesce(parts) => {
                f.write_str("coalesce(")?;
                write_list(f, parts)?;
                f.write_str(")")
            }
        }
    }
}

/// Operands separated by `, `.
pub(crate) fn write_list(f: &mut fmt::Formatter<'_>, parts: &[Operand]) -> fmt::Result {
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{part}")?;
    }
    Ok(())
}

/// A name bare when it lexes back as the one word it is and is no keyword;
/// otherwise in double quotes, a double quote doubled.
fn write_name(f: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
    if bare(name) {
        f.write_str(name)
    } else {
        write!(f, "\"{}\"", name.replace('"', "\"\""))
    }
}

fn bare(name: &str) -> bool {
    let starts = name
        .chars()
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_');
    starts
        && name
            .chars()
            .all(|c| word_character(c) || ".:/-".contains(c))
        && !KEYWORDS
            .iter()
            .any(|keyword| keyword.eq_ignore_ascii_case(name))
        && tokenize(name).is_ok_and(|tokens| tokens == [Token::Word(name.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> Operand {
        Operand::Name(text.into())
    }

    fn integer(number: i64) -> Operand {
        Operand::Literal(Value::Integer(number))
    }

    fn binary(left: Operand, operator: Operator, right: Operand) -> Operand {
        Operand::Binary {
            left: Box::new(left),
            operator,
            right: Box::new(right),
        }
    }

    #[test]
    fn a_name_is_bare_when_it_can_be_and_quoted_when_it_cannot() {
        assert_eq!(
            name("header:http.x-channel").to_string(),
            "header:http.x-channel"
        );
        assert_eq!(name("content:/order/id").to_string(), "content:/order/id");
        assert_eq!(name("a b").to_string(), "\"a b\"");
        assert_eq!(name("say \"hi\"").to_string(), "\"say \"\"hi\"\"\"");
        assert_eq!(name("and").to_string(), "\"and\"");
        assert_eq!(name("Null").to_string(), "\"Null\"");
        assert_eq!(name("a-").to_string(), "\"a-\"");
        assert_eq!(name("1a").to_string(), "\"1a\"");
        assert_eq!(
            name(r"regex:OrderNo:^INV-(\d+)$").to_string(),
            r#""regex:OrderNo:^INV-(\d+)$""#
        );
    }

    #[test]
    fn parentheses_stand_only_where_precedence_needs_them() {
        let a = || name("a");
        let b = || name("b");
        let c = || name("c");
        let left = binary(
            binary(a(), Operator::Subtract, b()),
            Operator::Subtract,
            c(),
        );
        assert_eq!(left.to_string(), "a - b - c");
        let right = binary(
            a(),
            Operator::Subtract,
            binary(b(), Operator::Subtract, c()),
        );
        assert_eq!(right.to_string(), "a - (b - c)");
        let mixed = binary(binary(a(), Operator::Add, b()), Operator::Multiply, c());
        assert_eq!(mixed.to_string(), "(a + b) * c");
        let joined = binary(
            a(),
            Operator::Concat,
            binary(b(), Operator::Add, integer(1)),
        );
        assert_eq!(joined.to_string(), "a || b + 1");
        let negated = Operand::Negate(Box::new(binary(a(), Operator::Add, b())));
        assert_eq!(negated.to_string(), "-(a + b)");
        assert_eq!(
            binary(a(), Operator::Subtract, integer(-5)).to_string(),
            "a - -5"
        );
        let twice = Operand::Negate(Box::new(integer(-5)));
        assert_eq!(twice.to_string(), "-(-5)");
        let fallback = Operand::Coalesce(vec![a(), Operand::Literal(Value::Text("x".into()))]);
        assert_eq!(fallback.to_string(), "coalesce(a, 'x')");
    }

    #[test]
    fn names_are_gathered_in_the_order_written() {
        let operand = Operand::Coalesce(vec![
            binary(name("b"), Operator::Add, name("a")),
            Operand::Negate(Box::new(name("c"))),
        ]);
        let mut names = Vec::new();
        operand.collect_names(&mut names);
        assert_eq!(names, ["b", "a", "c"]);
    }
}
