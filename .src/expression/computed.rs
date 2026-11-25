//! A value compiled from its text: the language's names, literals,
//! arithmetic, `||` and `coalesce`, with no condition around them — what a
//! Work Process computes the values it keeps from (ADR-0066, amendment
//! 2026-11-25). Parsed and checked once, as an [`super::Expression`] is,
//! and computed from the tree.

use std::fmt;

use contract::ContractError;

use super::operand::Operand;
use super::{Names, kind, parser, reading};

/// A value compiled from its text, kept with the text it was written as.
#[derive(Clone, Debug)]
pub struct Computed {
    text: String,
    operand: Operand,
}

impl Computed {
    /// Compile `text`: parse it as one value and check every part's kind.
    ///
    /// # Errors
    /// The text is not one value, or two of its parts are of kinds that
    /// cannot meet, said in a sentence naming the part.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let operand = parser::operand(text)?;
        kind::value(&operand)?;
        Ok(Self {
            text: text.to_string(),
            operand,
        })
    }

    /// The text as it was written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The compiled tree.
    #[must_use]
    pub const fn operand(&self) -> &Operand {
        &self.operand
    }

    /// Every name it reads, each once, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.operand.collect_names(&mut found);
        found.sort_unstable();
        found.dedup();
        found
    }

    /// Its value over `names`, as text: a number and a truth as written.
    ///
    /// # Errors
    /// The value is *unknown*: a name with no value, text that does not
    /// read as the kind asked, a division by zero or an overflow, each with
    /// its reason.
    pub fn compute(&self, names: &dyn Names) -> Result<String, String> {
        reading::read(&self.operand, names).map(|reading| reading.text().into_owned())
    }
}

/// Two values are equal when they compile to the same tree.
impl PartialEq for Computed {
    fn eq(&self, other: &Self) -> bool {
        self.operand == other.operand
    }
}

impl Eq for Computed {}

/// The text as it was written.
impl fmt::Display for Computed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct Map(BTreeMap<&'static str, &'static str>);

    impl Names for Map {
        fn value(&self, name: &str) -> Option<&str> {
            self.0.get(name).copied()
        }
    }

    #[test]
    fn a_value_is_computed_from_names_as_the_kind_it_meets() {
        let names = Map(BTreeMap::from([("Delivered", "3"), ("Quantity", "2")]));
        let counted = Computed::parse("coalesce(Delivered, 0) + Quantity").expect("compiles");
        assert_eq!(counted.compute(&names), Ok("5".to_string()));
        assert_eq!(counted.names(), ["Delivered", "Quantity"]);
        assert_eq!(counted.text(), "coalesce(Delivered, 0) + Quantity");
        let started = Computed::parse("coalesce(Accepted, 0)").expect("compiles");
        assert_eq!(started.compute(&Map(BTreeMap::new())), Ok("0".to_string()));
        let joined = Computed::parse("'order ' || Quantity").expect("compiles");
        assert_eq!(joined.compute(&names), Ok("order 2".to_string()));
        assert_eq!(
            Computed::parse("Delivered+Quantity").expect("compiles"),
            Computed::parse("Delivered + Quantity").expect("compiles"),
            "compared by the tree, however spaced"
        );
    }

    #[test]
    fn a_value_that_cannot_be_known_says_why_and_a_condition_is_no_value() {
        let names = Map(BTreeMap::from([("Delivered", "three")]));
        let counted = Computed::parse("Delivered + 1").expect("compiles");
        assert_eq!(
            counted.compute(&names),
            Err("Delivered is 'three', which is not an integer".to_string())
        );
        assert!(
            Computed::parse("Delivered + 'x'").is_err(),
            "arithmetic on text"
        );
        assert!(
            Computed::parse("Delivered > 1").is_err(),
            "a condition, not a value"
        );
        assert!(Computed::parse("").is_err(), "nothing");
    }
}
