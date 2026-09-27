//! What an expression decides: conditions over operands, gathered in `and`
//! and `or` groups, with `not` around any of them — and its one canonical
//! spelling.
//!
//! A condition is exactly what a designer's row or group holds (ADR-0066
//! clause 3): a comparison, `like`, `in` or `exists` is a row; `and` and
//! `or` are groups, kept as written, so `a and (b and c)` is a group inside
//! a group and prints back the same way.

use std::fmt;

use super::operand::{Operand, write_list};

/// A condition, parsed once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    /// Every part holds. Empty, it is `true`: everything.
    All(Vec<Condition>),
    /// At least one part holds. Empty, it is `false`: nothing.
    Any(Vec<Condition>),
    Not(Box<Condition>),
    /// `left <comparison> right`.
    Compare {
        left: Operand,
        comparison: Comparison,
        right: Operand,
    },
    /// `value like pattern`, or `not like`: `%` is any run of characters,
    /// `_` any one.
    Like {
        value: Operand,
        pattern: Operand,
        negated: bool,
    },
    /// `value in (a, b, …)`, or `not in`.
    In {
        value: Operand,
        list: Vec<Operand>,
        negated: bool,
    },
    /// `exists value`: it has a value. `value is not null` reads as this,
    /// and `value is null` as `not exists value`.
    Exists(Operand),
}

/// The six comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    /// `<>`; `!=` reads as it.
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl Comparison {
    /// Every comparison, in the order a designer offers them.
    pub const ALL: [Self; 6] = [
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessOrEqual,
        Self::Greater,
        Self::GreaterOrEqual,
    ];

    /// How the comparison is written.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        }
    }

    /// Whether it asks for an order, which a boolean does not have.
    #[must_use]
    pub const fn orders(self) -> bool {
        !matches!(self, Self::Equal | Self::NotEqual)
    }
}

/// The words a row's operator is, in the order a designer offers them:
/// the six comparisons, then `like`, `not like`, `in`, `not in` and
/// `exists`.
pub const OPERATORS: [&str; 11] = [
    "=", "<>", "<", "<=", ">", ">=", LIKE, NOT_LIKE, IN, NOT_IN, EXISTS,
];

pub(crate) const LIKE: &str = "like";
pub(crate) const NOT_LIKE: &str = "not like";
pub(crate) const IN: &str = "in";
pub(crate) const NOT_IN: &str = "not in";
pub(crate) const EXISTS: &str = "exists";

impl Condition {
    /// Everything: an empty `and`, written `true`.
    #[must_use]
    pub const fn everything() -> Self {
        Self::All(Vec::new())
    }

    /// The word of [`OPERATORS`] this condition is a row of; `None` for a
    /// group and for `not`.
    #[must_use]
    pub const fn operator(&self) -> Option<&'static str> {
        Some(match self {
            Self::Compare { comparison, .. } => comparison.spelling(),
            Self::Like { negated: false, .. } => LIKE,
            Self::Like { negated: true, .. } => NOT_LIKE,
            Self::In { negated: false, .. } => IN,
            Self::In { negated: true, .. } => NOT_IN,
            Self::Exists(_) => EXISTS,
            Self::All(_) | Self::Any(_) | Self::Not(_) => return None,
        })
    }

    /// Every name this condition reads, each once, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.collect_names(&mut found);
        found.sort_unstable();
        found.dedup();
        found
    }

    fn collect_names<'a>(&'a self, into: &mut Vec<&'a str>) {
        match self {
            Self::All(parts) | Self::Any(parts) => {
                for part in parts {
                    part.collect_names(into);
                }
            }
            Self::Not(inner) => inner.collect_names(into),
            Self::Compare { left, right, .. } => {
                left.collect_names(into);
                right.collect_names(into);
            }
            Self::Like { value, pattern, .. } => {
                value.collect_names(into);
                pattern.collect_names(into);
            }
            Self::In { value, list, .. } => {
                value.collect_names(into);
                for item in list {
                    item.collect_names(into);
                }
            }
            Self::Exists(value) => value.collect_names(into),
        }
    }

    /// Written as a part of a group or under `not`: a group with more than
    /// one part in parentheses, so a group inside a group stays one.
    fn write_part(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All(parts) | Self::Any(parts) if parts.len() > 1 => write!(f, "({self})"),
            _ => write!(f, "{self}"),
        }
    }
}

/// The canonical spelling: keywords in lower case, one space between
/// words, `<>` for not equal, `exists` for a value that is there,
/// parentheses around a group inside a group and nowhere else.
impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All(parts) if parts.is_empty() => f.write_str("true"),
            Self::Any(parts) if parts.is_empty() => f.write_str("false"),
            Self::All(parts) | Self::Any(parts) => {
                let join = if matches!(self, Self::All(_)) {
                    " and "
                } else {
                    " or "
                };
                for (index, part) in parts.iter().enumerate() {
                    if index > 0 {
                        f.write_str(join)?;
                    }
                    if parts.len() == 1 {
                        write!(f, "{part}")?;
                    } else {
                        part.write_part(f)?;
                    }
                }
                Ok(())
            }
            Self::Not(inner) => {
                f.write_str("not ")?;
                inner.write_part(f)
            }
            Self::Compare {
                left,
                comparison,
                right,
            } => write!(f, "{left} {} {right}", comparison.spelling()),
            Self::Like { value, pattern, .. } => {
                write!(f, "{value} {} {pattern}", self.operator().unwrap_or(LIKE))
            }
            Self::In { value, list, .. } => {
                write!(f, "{value} {} (", self.operator().unwrap_or(IN))?;
                write_list(f, list)?;
                f.write_str(")")
            }
            Self::Exists(value) => write!(f, "{EXISTS} {value}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::value::Value;

    fn equals(name: &str, number: i64) -> Condition {
        Condition::Compare {
            left: Operand::Name(name.into()),
            comparison: Comparison::Equal,
            right: Operand::Literal(Value::Integer(number)),
        }
    }

    #[test]
    fn a_group_inside_a_group_keeps_its_parentheses() {
        let filter = Condition::All(vec![
            equals("a", 1),
            Condition::Any(vec![equals("b", 2), equals("c", 3)]),
            Condition::All(vec![equals("d", 4), equals("e", 5)]),
            Condition::Not(Box::new(Condition::All(vec![
                equals("f", 6),
                equals("g", 7),
            ]))),
            Condition::Not(Box::new(equals("h", 8))),
            Condition::everything(),
        ]);
        assert_eq!(
            filter.to_string(),
            "a = 1 and (b = 2 or c = 3) and (d = 4 and e = 5) and not (f = 6 and g = 7) \
             and not h = 8 and true"
        );
        assert_eq!(Condition::Any(Vec::new()).to_string(), "false");
        assert_eq!(Condition::All(vec![equals("a", 1)]).to_string(), "a = 1");
    }

    #[test]
    fn every_row_prints_as_its_operator() {
        let name = || Operand::Name("Region".into());
        let text = |t: &str| Operand::Literal(Value::Text(t.into()));
        let rows = [
            Condition::Like {
                value: name(),
                pattern: text("E%"),
                negated: true,
            },
            Condition::In {
                value: name(),
                list: vec![text("EU"), text("US")],
                negated: false,
            },
            Condition::Exists(name()),
        ];
        let printed: Vec<String> = rows.iter().map(ToString::to_string).collect();
        assert_eq!(
            printed,
            [
                "Region not like 'E%'",
                "Region in ('EU', 'US')",
                "exists Region"
            ]
        );
        assert_eq!(rows[1].operator(), Some("in"));
        assert_eq!(Condition::everything().operator(), None);
        assert_eq!(OPERATORS[1], Comparison::NotEqual.spelling());
    }

    #[test]
    fn names_are_each_read_once_and_sorted() {
        let filter = Condition::Any(vec![equals("b", 1), equals("a", 2), equals("b", 3)]);
        assert_eq!(filter.names(), ["a", "b"]);
    }
}
