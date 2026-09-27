//! The tokens of an expression.
//!
//! SQL's quoting, because an expression sits inside a TOML string and reads
//! as a WHERE clause does: text in single quotes, a quote doubled inside it
//! (`'O''Brien'`); a name bare, or in double quotes when it holds what a bare
//! name cannot (`"regex:OrderNo:^INV-(\d+)$"`), a double quote doubled. A
//! bare name is a letter or `_` followed by letters, digits and `_`, and a
//! run of `.`, `:`, `/` or `-` joins two of those, so `header:http.x-channel`
//! is one name. That is why subtraction and division are written with space
//! around them: `Amount - 1`, where `Amount-1` is a name.

use std::fmt;

use codec::char_reader::CharReader;
use contract::ContractError;

/// One token of an expression.
#[derive(Clone, PartialEq, Eq)]
pub enum Token {
    /// A bare word: a keyword (`and`, `or`, `not`, `in`, `like`, `is`,
    /// `null`, `exists`, `true`, `false`, `coalesce`, in any case) or a name.
    Word(String),
    /// A name in double quotes, quotes removed; never a keyword.
    Quoted(String),
    /// Text in single quotes, quotes removed.
    Text(String),
    /// Digits. Unsigned, so the parser can fold a minus into the smallest
    /// integer there is.
    Integer(u64),
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Plus,
    Minus,
    Star,
    Slash,
    Concat,
    OpenParen,
    CloseParen,
    Comma,
}

/// How a token reads in a sentence: as it is written.
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = match self {
            Self::Word(word) => return write!(f, "'{word}'"),
            Self::Quoted(name) => return write!(f, "\"{name}\""),
            Self::Text(text) => return write!(f, "the text '{text}'"),
            Self::Integer(number) => return write!(f, "{number}"),
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Concat => "||",
            Self::OpenParen => "(",
            Self::CloseParen => ")",
            Self::Comma => ",",
        };
        write!(f, "'{symbol}'")
    }
}

/// What joins two word characters inside a bare name.
const JOINERS: [char; 4] = ['.', ':', '/', '-'];

/// Split `expression` into tokens. Whitespace is any Unicode whitespace, and
/// a word may hold any letter.
///
/// # Errors
/// A character that begins no token, an unterminated quote, a decimal, a
/// number too large, or `==` and `!` alone, each named with its offset.
pub fn tokenize(expression: &str) -> Result<Vec<Token>, ContractError> {
    let mut reader = CharReader::new(expression);
    let mut tokens = Vec::new();
    while let Some(first) = reader.peek() {
        if reader.skip_whitespace() {
            continue;
        }
        let at = reader.offset();
        let token = match first {
            '=' if reader.starts_with("==") => {
                return Err(error(format!(
                    "'==' at {at} is not an operator; equality is '='"
                )));
            }
            '!' if reader.eat_str("!=") => Token::NotEqual,
            '<' if reader.eat_str("<>") => Token::NotEqual,
            '<' if reader.eat_str("<=") => Token::LessOrEqual,
            '>' if reader.eat_str(">=") => Token::GreaterOrEqual,
            '|' if reader.eat_str("||") => Token::Concat,
            '\'' => Token::Text(quoted(&mut reader, '\'')?),
            '"' => name(&mut reader)?,
            digit if digit.is_ascii_digit() => number(&mut reader)?,
            letter if letter.is_alphabetic() || letter == '_' => {
                Token::Word(word(&mut reader).to_string())
            }
            other => {
                let token = symbol(other).ok_or_else(|| {
                    error(format!("unexpected {other:?} at {at} in {expression:?}"))
                })?;
                reader.bump();
                token
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

fn symbol(character: char) -> Option<Token> {
    Some(match character {
        '=' => Token::Equal,
        '<' => Token::Less,
        '>' => Token::Greater,
        '+' => Token::Plus,
        '-' => Token::Minus,
        '*' => Token::Star,
        '/' => Token::Slash,
        '(' => Token::OpenParen,
        ')' => Token::CloseParen,
        ',' => Token::Comma,
        _ => return None,
    })
}

/// Whether `character` may stand inside a bare name on its own.
pub(crate) fn word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// A bare word: word characters, and runs of joiners between two of them.
fn word<'a>(reader: &mut CharReader<'a>) -> &'a str {
    let start = reader.offset();
    loop {
        reader.take_while(word_character);
        let joiners = reader.peek_while(|character| JOINERS.contains(&character));
        let after = reader.rest()[joiners.len()..].chars().next();
        if joiners.is_empty() || !after.is_some_and(word_character) {
            return reader.since(start);
        }
        reader.eat_str(joiners);
    }
}

/// Text between `quote`s, a doubled quote standing for one.
fn quoted(reader: &mut CharReader<'_>, quote: char) -> Result<String, ContractError> {
    let start = reader.offset();
    reader.bump();
    let mut text = String::new();
    while let Some(character) = reader.bump() {
        if character != quote {
            text.push(character);
        } else if reader.eat(quote) {
            text.push(quote);
        } else {
            return Ok(text);
        }
    }
    Err(error(format!(
        "unterminated quote at {start}: {}",
        reader.since(start)
    )))
}

fn name(reader: &mut CharReader<'_>) -> Result<Token, ContractError> {
    let at = reader.offset();
    let name = quoted(reader, '"')?;
    if name.is_empty() {
        return Err(error(format!("an empty name at {at}")));
    }
    Ok(Token::Quoted(name))
}

/// Digits; a point after them is a decimal, which an expression does not
/// have.
fn number(reader: &mut CharReader<'_>) -> Result<Token, ContractError> {
    let at = reader.offset();
    let digits = reader.take_while(|character| character.is_ascii_digit());
    if reader.peek() == Some('.') && reader.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) {
        let fraction = reader.take_while(|c| c == '.' || c.is_ascii_digit());
        return Err(error(format!(
            "{digits}{fraction} at {at} is a decimal, and an expression has integers, \
             text and booleans only"
        )));
    }
    digits
        .parse()
        .map(Token::Integer)
        .map_err(|_| error(format!("{digits} at {at} is too large for an integer")))
}

/// A refusal in the language's name.
pub(crate) fn error(message: impl fmt::Display) -> ContractError {
    ContractError::new(format!("expression: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word_token(text: &str) -> Token {
        Token::Word(text.into())
    }

    #[test]
    fn tokenizes_every_kind() {
        let tokens = tokenize(
            "MessageType = 'O''Brien' and not Amount >= 12 or \"a b\" <> x || y \
             + 1 - 2 * 3 / 4 < 5 <= 6 > 7 != (8, 9)",
        )
        .expect("lexes");
        assert_eq!(
            tokens,
            vec![
                word_token("MessageType"),
                Token::Equal,
                Token::Text("O'Brien".into()),
                word_token("and"),
                word_token("not"),
                word_token("Amount"),
                Token::GreaterOrEqual,
                Token::Integer(12),
                word_token("or"),
                Token::Quoted("a b".into()),
                Token::NotEqual,
                word_token("x"),
                Token::Concat,
                word_token("y"),
                Token::Plus,
                Token::Integer(1),
                Token::Minus,
                Token::Integer(2),
                Token::Star,
                Token::Integer(3),
                Token::Slash,
                Token::Integer(4),
                Token::Less,
                Token::Integer(5),
                Token::LessOrEqual,
                Token::Integer(6),
                Token::Greater,
                Token::Integer(7),
                Token::NotEqual,
                Token::OpenParen,
                Token::Integer(8),
                Token::Comma,
                Token::Integer(9),
                Token::CloseParen,
            ]
        );
    }

    #[test]
    fn a_joiner_between_two_word_characters_is_part_of_the_name() {
        assert_eq!(
            tokenize("a- b.").expect_err("a point alone").message,
            "expression: unexpected '.' at 4 in \"a- b.\""
        );
        assert_eq!(
            tokenize("header:http.x-channel content:/order/id Amount-1 Amount - 1 a -1")
                .expect("lexes"),
            vec![
                word_token("header:http.x-channel"),
                word_token("content:/order/id"),
                word_token("Amount-1"),
                word_token("Amount"),
                Token::Minus,
                Token::Integer(1),
                word_token("a"),
                Token::Minus,
                Token::Integer(1),
            ]
        );
    }

    #[test]
    fn refuses_what_begins_no_token_and_says_where() {
        let stray = tokenize("a. b").expect_err("a point alone");
        assert!(stray.message.contains("'.' at 1"), "{}", stray.message);
        assert!(tokenize("'open").is_err());
        assert!(tokenize("\"open").is_err());
        assert!(tokenize("\"\" = 1").is_err());
        assert!(tokenize("a ! 1").is_err());
        assert!(tokenize("a | b").is_err());
        let decimal = tokenize("Amount > 12.5").expect_err("a decimal");
        assert!(
            decimal.message.contains("12.5 at 9 is a decimal"),
            "{}",
            decimal.message
        );
        let equal = tokenize("a == 1").expect_err("==");
        assert!(
            equal.message.contains("equality is '='"),
            "{}",
            equal.message
        );
        assert!(tokenize("99999999999999999999").is_err());
    }

    #[test]
    fn multibyte_whitespace_and_letters_lex_without_panic() {
        let tokens =
            tokenize("naïve\u{a0}=\u{3000}'Zoë 名前'\u{2003}and\u{a0}größer").expect("lexes");
        assert_eq!(
            tokens,
            vec![
                word_token("naïve"),
                Token::Equal,
                Token::Text("Zoë 名前".into()),
                word_token("and"),
                word_token("größer"),
            ]
        );
        assert!(tokenize("a = 1\u{a0}€").is_err());
    }
}
