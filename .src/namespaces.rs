//! The namespaces a Path's configuration binds: each prefix to a URI.
//!
//! A path language that names things by a prefix — `XPath`'s `o:order` —
//! resolves the prefix through this table and never through the document
//! it reads, so `o:order` means the element `order` in the namespace the
//! configuration gave `o`, whatever prefix a document happens to write
//! for it, and wherever in the document that is declared. A language with
//! no namespaces refuses a table that binds anything ([`Namespaces::unused_by`]),
//! so a binding nobody reads is said at configuration rather than ignored.

use std::collections::BTreeMap;

use contract::ContractError;

/// Prefix to URI, as a Path's `namespaces` table writes them.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Namespaces(BTreeMap<String, String>);

impl Namespaces {
    /// A table binding nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// This table with `prefix` bound to `uri`, replacing what `prefix` was
    /// bound to.
    #[must_use]
    pub fn bind(mut self, prefix: impl Into<String>, uri: impl Into<String>) -> Self {
        self.0.insert(prefix.into(), uri.into());
        self
    }

    /// The URI `prefix` is bound to, where it is.
    #[must_use]
    pub fn uri(&self, prefix: &str) -> Option<&str> {
        self.0.get(prefix).map(String::as_str)
    }

    /// Every binding, prefix and URI, in the prefixes' order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(prefix, uri)| (prefix.as_str(), uri.as_str()))
    }

    /// Whether nothing is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The refusal of a language that has no namespaces, where this table
    /// binds anything.
    ///
    /// # Errors
    /// The table binds a prefix, naming the language and the prefixes.
    pub fn unused_by(&self, language: &str) -> Result<(), ContractError> {
        if self.is_empty() {
            return Ok(());
        }
        let prefixes: Vec<&str> = self.0.keys().map(String::as_str).collect();
        Err(ContractError::new(format!(
            "{language} has no namespaces, and the path binds {}",
            prefixes.join(", ")
        )))
    }
}

impl<P: Into<String>, U: Into<String>> FromIterator<(P, U)> for Namespaces {
    fn from_iter<I: IntoIterator<Item = (P, U)>>(bindings: I) -> Self {
        Self(
            bindings
                .into_iter()
                .map(|(prefix, uri)| (prefix.into(), uri.into()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_resolves_through_the_table_and_a_language_without_namespaces_refuses_one() {
        let bound = Namespaces::new().bind("o", "urn:example:order");
        assert_eq!(bound.uri("o"), Some("urn:example:order"));
        assert_eq!(bound.uri("p"), None);
        assert_eq!(
            bound.iter().collect::<Vec<_>>(),
            [("o", "urn:example:order")]
        );
        assert_eq!(
            bound.unused_by("jsonpath").expect_err("refused").message,
            "jsonpath has no namespaces, and the path binds o"
        );
        assert!(Namespaces::new().unused_by("jsonpath").is_ok());
        let collected: Namespaces = [("o", "urn:example:order")].into_iter().collect();
        assert_eq!(collected, bound);
    }
}
