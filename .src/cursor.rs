//! The cursor a path language's parser walks its tokens with, shared by
//! `FHIRPath` and the predicate language (ADR-0044). The tokens and the
//! grammar are each language's own; what is the same is looking at the next
//! token, taking it, and insisting on one.

use sdk::contract::ContractError;
use std::fmt::Debug;

/// A cursor over a language's tokens.
pub struct Cursor<'a, T> {
    language: &'static str,
    tokens: &'a [T],
    at: usize,
}

impl<'a, T: Debug + PartialEq> Cursor<'a, T> {
    /// At the first of `tokens`; `language` names the language in every
    /// refusal, the way its lexer already does.
    #[must_use]
    pub const fn new(language: &'static str, tokens: &'a [T]) -> Self {
        Self {
            language,
            tokens,
            at: 0,
        }
    }

    /// The next token, not taken.
    #[must_use]
    pub fn peek(&self) -> Option<&'a T> {
        self.tokens.get(self.at)
    }

    /// The token `ahead` past the next, not taken.
    #[must_use]
    pub fn peek_at(&self, ahead: usize) -> Option<&'a T> {
        self.tokens.get(self.at + ahead)
    }

    /// The next token, taken; `None` at the end, where the cursor stays.
    pub fn take(&mut self) -> Option<&'a T> {
        let token = self.tokens.get(self.at);
        if token.is_some() {
            self.at += 1;
        }
        token
    }

    /// Past `count` tokens already looked at.
    pub fn advance(&mut self, count: usize) {
        self.at = (self.at + count).min(self.tokens.len());
    }

    /// Take `token`, or refuse what is there instead.
    ///
    /// # Errors
    /// The next token is not `token`, or there is none.
    pub fn expect(&mut self, token: &T) -> Result<(), ContractError> {
        match self.take() {
            Some(found) if found == token => Ok(()),
            other => Err(self.error(format!("expected {token:?}, found {other:?}"))),
        }
    }

    /// A refusal in the language's name.
    #[must_use]
    pub fn error(&self, message: impl std::fmt::Display) -> ContractError {
        ContractError::new(format!("{}: {message}", self.language))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_peeks_takes_and_insists() {
        let tokens = ['a', 'b', 'c'];
        let mut cursor = Cursor::new("letters", &tokens);
        assert_eq!(cursor.peek(), Some(&'a'));
        assert_eq!(cursor.peek_at(1), Some(&'b'));
        assert_eq!(cursor.take(), Some(&'a'));
        cursor.expect(&'b').expect("b");
        let refused = cursor.expect(&'x').expect_err("c is not x");
        assert_eq!(refused.message, "letters: expected 'x', found Some('c')");
        assert_eq!(cursor.take(), None);
        assert_eq!(cursor.peek(), None);
    }

    #[test]
    fn advancing_stops_at_the_end() {
        let tokens = [1, 2];
        let mut cursor = Cursor::new("numbers", &tokens);
        cursor.advance(5);
        assert_eq!(cursor.take(), None);
        let refused = cursor.expect(&1).expect_err("nothing left");
        assert_eq!(refused.message, "numbers: expected 1, found None");
    }
}
