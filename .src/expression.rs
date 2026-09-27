//! Xmip's expression language (ADR-0066 clause 2): the one grammar a
//! route's filter, a transform's conditions and an Xmip Process's decisions
//! are written in. It is the path capability's own, not a technology of it,
//! because every layer that reads an expression — route, the transforms and
//! Xmip Processes compiled from designs, and `configure`, a platform service
//! that may depend on a capability and never on a technology — must reach
//! the one tree.
//!
//! Shaped like SQL's WHERE clause and deliberately small:
//!
//! ```text
//! MessageType = 'Order' and not Amount > 1000 and header:http.x-channel = 'web'
//! ```
//!
//! - **Names** are bare, with the prefix of the source that reads them
//!   (`header:http.x-channel`, `party:sender`, `context:X`), or in double
//!   quotes when they hold what a bare name cannot. **Text** is in single
//!   quotes, a quote doubled inside it. **Integers** and **booleans**
//!   (`true`, `false`) are written as themselves; there are no decimals.
//! - **Conditions**: `=`, `<>` (and `!=`, read as `<>`), `<`, `<=`, `>`,
//!   `>=`; `[not] like` with `%` and `_`; `[not] in (…)`; `exists X`, and
//!   `X is [not] null` read as it; `and`, `or`, `not`, parentheses.
//! - **Values**: `||` joins text, `+ - * /` compute integers, `coalesce(…)`
//!   takes the first that has a value.
//! - No loops, no functions of the user's own, no I/O: its cost grows with
//!   its length alone.
//!
//! **Parsed once, evaluated from the tree.** [`Expression::parse`] reads
//! the text, checks every part's kind and keeps the tree; nothing
//! evaluating it ever parses again. A value that is not there is *unknown*,
//! carrying its reason, in SQL's three truths ([`Truth`]) — never a silent
//! false. A name reads text through [`Names`], and the text is read as the
//! kind of what it meets: `Amount > 1000` reads Amount as an integer,
//! `Amount = '1000'` as text.
//!
//! **One canonical spelling.** [`Condition`]'s `Display` writes the text a
//! designer writes back when it edits (ADR-0064 clause 4): canonical text
//! parses to a tree that prints as the same text, byte for byte, and any
//! accepted text parses to a tree that prints as its canonical equivalent.
//! Reading never rewrites: an [`Expression`] keeps the text it was written
//! as.

mod condition;
mod kind;
mod lexer;
mod operand;
mod parser;
mod reading;
mod truth;
mod value;

pub use condition::{Comparison, Condition, OPERATORS};
pub use operand::{KEYWORDS, Operand, Operator};
pub use parser::{operand, operands};
pub use truth::Truth;
pub use value::{KINDS, Value};

use std::fmt;

use contract::ContractError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Where an expression's names are read: a Message's promoted properties,
/// a transform's fields. The text a name has, or `None` when it has none.
pub trait Names {
    /// The text `name` has, or `None`.
    fn value(&self, name: &str) -> Option<&str>;

    /// The reason a name has no value, in the reader's own words.
    fn absent(&self, name: &str) -> String {
        format!("{name} has no value")
    }
}

/// A condition compiled from its text: parsed and checked once, and kept
/// with the text it was written as.
#[derive(Clone, Debug)]
pub struct Expression {
    text: String,
    condition: Condition,
}

impl Expression {
    /// Compile `text`: parse it and check every part's kind.
    ///
    /// # Errors
    /// The text is not an expression, or two of its parts are of kinds that
    /// cannot meet — said in a sentence naming the part.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let condition = parser::condition(text)?;
        kind::check(&condition)?;
        Ok(Self {
            text: text.to_string(),
            condition,
        })
    }

    /// Compile a condition built as a tree, a designer's rows; its text is
    /// the canonical one.
    ///
    /// # Errors
    /// As [`Expression::parse`], for the kinds.
    pub fn from_condition(condition: Condition) -> Result<Self, ContractError> {
        kind::check(&condition)?;
        Ok(Self {
            text: condition.to_string(),
            condition,
        })
    }

    /// Everything: `true`.
    #[must_use]
    pub fn everything() -> Self {
        Self {
            text: "true".to_string(),
            condition: Condition::everything(),
        }
    }

    /// The text as it was written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text as a designer writes it back.
    #[must_use]
    pub fn canonical(&self) -> String {
        self.condition.to_string()
    }

    /// The compiled tree.
    #[must_use]
    pub const fn condition(&self) -> &Condition {
        &self.condition
    }

    /// Every name the expression reads, each once, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.condition.names()
    }

    /// Decide it over `names`, from the tree.
    #[must_use]
    pub fn evaluate(&self, names: &dyn Names) -> Truth {
        self.condition.evaluate(names)
    }
}

/// Two expressions are equal when they compile to the same tree, however
/// each was spaced or spelled.
impl PartialEq for Expression {
    fn eq(&self, other: &Self) -> bool {
        self.condition == other.condition
    }
}

impl Eq for Expression {}

/// The text as it was written.
impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Written as the text it was read from, so a document read and written is
/// unchanged.
impl Serialize for Expression {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

/// Read from text and compiled there, so a configuration holding an
/// expression that does not compile is refused as it loads.
impl<'de> Deserialize<'de> for Expression {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(|refused| serde::de::Error::custom(refused.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESEARCH: &str =
        "MessageType = 'Order' and not Amount > 1000 and header:http.x-channel = 'web'";

    #[test]
    fn an_expression_keeps_its_text_and_compares_by_its_tree() {
        let spaced = Expression::parse("MessageType='Order'  AND Amount>1000").expect("parses");
        assert_eq!(spaced.text(), "MessageType='Order'  AND Amount>1000");
        assert_eq!(
            spaced.canonical(),
            "MessageType = 'Order' and Amount > 1000"
        );
        assert_eq!(
            spaced,
            Expression::parse("MessageType = 'Order' and Amount > 1000").expect("parses")
        );
        assert_eq!(
            Expression::parse(RESEARCH).expect("parses").names(),
            ["Amount", "MessageType", "header:http.x-channel"]
        );
        assert_eq!(Expression::everything().text(), "true");
    }

    #[test]
    fn it_is_read_and_written_as_its_text_and_refused_when_it_does_not_compile() {
        #[derive(Debug, Deserialize, Serialize)]
        struct Holder {
            filter: Expression,
        }

        let holder: Holder = toml::from_str("filter = \"Amount>1000\"").expect("reads");
        assert_eq!(holder.filter.canonical(), "Amount > 1000");
        assert_eq!(
            toml::to_string(&holder).expect("writes"),
            "filter = \"Amount>1000\"\n"
        );
        let refused = toml::from_str::<Holder>("filter = \"Amount > 'x' + 1\"")
            .expect_err("arithmetic on text");
        assert!(refused.to_string().contains("arithmetic"), "{refused}");
    }

    #[test]
    fn a_tree_compiles_with_its_canonical_text() {
        let condition = parser::condition(RESEARCH).expect("parses");
        let compiled = Expression::from_condition(condition).expect("compiles");
        assert_eq!(compiled.text(), RESEARCH);
        let wrong = parser::condition("1 = 'x'").expect("parses");
        assert!(Expression::from_condition(wrong).is_err());
    }
}
