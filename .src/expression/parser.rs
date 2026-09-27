//! The grammar: text to a [`Condition`], once.
//!
//! Loosest first: `or`, `and`, `not`, then one predicate — a comparison,
//! `[not] like`, `[not] in (…)`, `is [not] null` or `exists` — over
//! operands, whose own precedence is `||`, `+ -`, `* /` and a unary minus.
//! So `a or b and c` is `a or (b and c)` and `not a = 1` is `not (a = 1)`,
//! as in SQL. Keywords are read in any case. A parenthesized `and` or `or`
//! stays a group of its own, which is what lets a designer's group inside a
//! group print back as it was drawn.
//!
//! The cursor is the path capability's, shared with `FHIRPath` (ADR-0044);
//! the grammar is this language's.

use crate::cursor::Cursor;
use contract::ContractError;

use super::condition::{Comparison, Condition};
use super::lexer::{Token, error, tokenize};
use super::operand::{Operand, Operator};
use super::value::Value;

type Tokens<'a> = Cursor<'a, Token>;

/// What a level of the grammar produced: a condition or a value. Which one
/// a place needs is checked where it is used, so a parenthesis can hold
/// either.
enum Parsed {
    Condition(Condition),
    Operand(Operand),
}

/// Parse `text` as a condition.
///
/// # Errors
/// The text does not lex, does not follow the grammar, or puts a value
/// where a condition belongs or the reverse, said in a sentence.
pub fn condition(text: &str) -> Result<Condition, ContractError> {
    let tokens = tokenize(text)?;
    if tokens.is_empty() {
        return Err(error(
            "an expression needs a condition; `true` takes everything",
        ));
    }
    let mut cursor = Cursor::new("expression", &tokens);
    let parsed = disjunction(&mut cursor)?;
    finished(&cursor)?;
    into_condition(parsed)
}

/// Parse `text` as one value, such as a designer row's property.
///
/// # Errors
/// As [`condition`], for a value.
pub fn operand(text: &str) -> Result<Operand, ContractError> {
    let tokens = tokenize(text)?;
    let mut cursor = Cursor::new("expression", &tokens);
    let parsed = concatenation(&mut cursor)?;
    finished(&cursor)?;
    into_operand(parsed)
}

/// Parse `text` as values separated by commas, the list an `in` holds.
///
/// # Errors
/// As [`condition`], for each value.
pub fn operands(text: &str) -> Result<Vec<Operand>, ContractError> {
    let tokens = tokenize(text)?;
    let mut cursor = Cursor::new("expression", &tokens);
    let list = list(&mut cursor)?;
    finished(&cursor)?;
    Ok(list)
}

fn finished(cursor: &Tokens<'_>) -> Result<(), ContractError> {
    match cursor.peek() {
        None => Ok(()),
        Some(token) => Err(error(format!("unexpected {token:?} after the expression"))),
    }
}

fn disjunction(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    let first = conjunction(cursor)?;
    if !keyword(cursor, "or") {
        return Ok(first);
    }
    let mut parts = vec![into_condition(first)?];
    while keyword(cursor, "or") {
        cursor.advance(1);
        parts.push(into_condition(conjunction(cursor)?)?);
    }
    Ok(Parsed::Condition(Condition::Any(parts)))
}

fn conjunction(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    let first = negation(cursor)?;
    if !keyword(cursor, "and") {
        return Ok(first);
    }
    let mut parts = vec![into_condition(first)?];
    while keyword(cursor, "and") {
        cursor.advance(1);
        parts.push(into_condition(negation(cursor)?)?);
    }
    Ok(Parsed::Condition(Condition::All(parts)))
}

fn negation(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    if keyword(cursor, "not") {
        cursor.advance(1);
        let inner = into_condition(negation(cursor)?)?;
        return Ok(Parsed::Condition(Condition::Not(Box::new(inner))));
    }
    predicate(cursor)
}

fn predicate(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    if keyword(cursor, "exists") {
        cursor.advance(1);
        let value = into_operand(concatenation(cursor)?)?;
        return Ok(Parsed::Condition(Condition::Exists(value)));
    }

    let left = concatenation(cursor)?;
    let negated = keyword(cursor, "not");
    if negated {
        cursor.advance(1);
    }
    let condition = if let Some(comparison) = comparison(cursor.peek()).filter(|_| !negated) {
        cursor.advance(1);
        Condition::Compare {
            left: into_operand(left)?,
            comparison,
            right: into_operand(concatenation(cursor)?)?,
        }
    } else if keyword(cursor, "like") {
        cursor.advance(1);
        Condition::Like {
            value: into_operand(left)?,
            pattern: into_operand(concatenation(cursor)?)?,
            negated,
        }
    } else if keyword(cursor, "in") {
        cursor.advance(1);
        expect(cursor, &Token::OpenParen, "after in")?;
        let list = list(cursor)?;
        expect(cursor, &Token::CloseParen, "after the list in holds")?;
        Condition::In {
            value: into_operand(left)?,
            list,
            negated,
        }
    } else if negated {
        return Err(error("not after a value is followed by like or in"));
    } else if keyword(cursor, "is") {
        cursor.advance(1);
        null_test(cursor, into_operand(left)?)?
    } else {
        return Ok(left);
    };
    Ok(Parsed::Condition(condition))
}

/// `is null` and `is not null`, read as `not exists` and `exists`.
fn null_test(cursor: &mut Tokens<'_>, value: Operand) -> Result<Condition, ContractError> {
    let not = keyword(cursor, "not");
    if not {
        cursor.advance(1);
    }
    if !keyword(cursor, "null") {
        return Err(error(format!(
            "is is followed by null or not null, not {:?}",
            cursor.peek()
        )));
    }
    cursor.advance(1);
    let exists = Condition::Exists(value);
    Ok(if not {
        exists
    } else {
        Condition::Not(Box::new(exists))
    })
}

fn comparison(token: Option<&Token>) -> Option<Comparison> {
    Some(match token? {
        Token::Equal => Comparison::Equal,
        Token::NotEqual => Comparison::NotEqual,
        Token::Less => Comparison::Less,
        Token::LessOrEqual => Comparison::LessOrEqual,
        Token::Greater => Comparison::Greater,
        Token::GreaterOrEqual => Comparison::GreaterOrEqual,
        _ => return None,
    })
}

/// One precedence level of binary operators over values, left to right.
fn binary(
    cursor: &mut Tokens<'_>,
    operators: &[(Token, Operator)],
    tighter: fn(&mut Tokens<'_>) -> Result<Parsed, ContractError>,
) -> Result<Parsed, ContractError> {
    let mut left = tighter(cursor)?;
    while let Some((_, operator)) = operators
        .iter()
        .find(|(token, _)| cursor.peek() == Some(token))
    {
        cursor.advance(1);
        let right = into_operand(tighter(cursor)?)?;
        left = Parsed::Operand(Operand::Binary {
            left: Box::new(into_operand(left)?),
            operator: *operator,
            right: Box::new(right),
        });
    }
    Ok(left)
}

fn concatenation(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    binary(cursor, &[(Token::Concat, Operator::Concat)], additive)
}

fn additive(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    let operators = [
        (Token::Plus, Operator::Add),
        (Token::Minus, Operator::Subtract),
    ];
    binary(cursor, &operators, multiplicative)
}

fn multiplicative(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    let operators = [
        (Token::Star, Operator::Multiply),
        (Token::Slash, Operator::Divide),
    ];
    binary(cursor, &operators, unary)
}

/// A minus. Directly before digits it is the literal's sign, so `-5` is one
/// integer and the smallest integer there is can be written.
fn unary(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    if cursor.peek() != Some(&Token::Minus) {
        return primary(cursor);
    }
    cursor.advance(1);
    if let Some(Token::Integer(digits)) = cursor.peek() {
        cursor.advance(1);
        let number = 0i64
            .checked_sub_unsigned(*digits)
            .ok_or_else(|| error(format!("-{digits} is too small for an integer")))?;
        return Ok(Parsed::Operand(Operand::Literal(Value::Integer(number))));
    }
    let inner = into_operand(unary(cursor)?)?;
    Ok(Parsed::Operand(Operand::Negate(Box::new(inner))))
}

fn primary(cursor: &mut Tokens<'_>) -> Result<Parsed, ContractError> {
    let literal = |value| Ok(Parsed::Operand(Operand::Literal(value)));
    match cursor.take() {
        Some(Token::OpenParen) => {
            let inner = disjunction(cursor)?;
            expect(cursor, &Token::CloseParen, "to close the parenthesis")?;
            Ok(inner)
        }
        Some(Token::Text(text)) => literal(Value::Text(text.clone())),
        Some(Token::Integer(digits)) => {
            let number = i64::try_from(*digits)
                .map_err(|_| error(format!("{digits} is too large for an integer")))?;
            literal(Value::Integer(number))
        }
        Some(Token::Quoted(name)) => Ok(Parsed::Operand(Operand::Name(name.clone()))),
        Some(Token::Word(word)) => word_operand(cursor, word),
        other => Err(error(format!("expected a value, found {}", found(other)))),
    }
}

fn word_operand(cursor: &mut Tokens<'_>, word: &str) -> Result<Parsed, ContractError> {
    let is = |keyword: &str| word.eq_ignore_ascii_case(keyword);
    if is("true") || is("false") {
        return Ok(Parsed::Operand(Operand::Literal(Value::Boolean(is(
            "true",
        )))));
    }
    if is("coalesce") {
        expect(cursor, &Token::OpenParen, "after coalesce")?;
        let parts = list(cursor)?;
        expect(cursor, &Token::CloseParen, "to close coalesce")?;
        return Ok(Parsed::Operand(Operand::Coalesce(parts)));
    }
    if is("null") {
        return Err(error(
            "null is not a value; ask `exists X` or `X is null` of a name",
        ));
    }
    if super::operand::KEYWORDS.iter().any(|keyword| is(keyword)) {
        return Err(error(format!("expected a value, found '{word}'")));
    }
    Ok(Parsed::Operand(Operand::Name(word.to_string())))
}

/// One or more values separated by commas.
fn list(cursor: &mut Tokens<'_>) -> Result<Vec<Operand>, ContractError> {
    let mut items = vec![into_operand(concatenation(cursor)?)?];
    while cursor.peek() == Some(&Token::Comma) {
        cursor.advance(1);
        items.push(into_operand(concatenation(cursor)?)?);
    }
    Ok(items)
}

fn expect(cursor: &mut Tokens<'_>, token: &Token, why: &str) -> Result<(), ContractError> {
    match cursor.take() {
        Some(found) if found == token => Ok(()),
        other => Err(error(format!(
            "expected {token:?} {why}, found {}",
            found(other)
        ))),
    }
}

fn found(token: Option<&Token>) -> String {
    token.map_or_else(|| "the end".to_string(), |token| format!("{token:?}"))
}

fn keyword(cursor: &Tokens<'_>, word: &str) -> bool {
    matches!(cursor.peek(), Some(Token::Word(found)) if found.eq_ignore_ascii_case(word))
}

/// A parsed part where a condition belongs. `true` and `false` are the
/// empty `and` and the empty `or`; any other value is refused, because a
/// condition compares it.
fn into_condition(parsed: Parsed) -> Result<Condition, ContractError> {
    match parsed {
        Parsed::Condition(condition) => Ok(condition),
        Parsed::Operand(Operand::Literal(Value::Boolean(true))) => Ok(Condition::All(Vec::new())),
        Parsed::Operand(Operand::Literal(Value::Boolean(false))) => Ok(Condition::Any(Vec::new())),
        Parsed::Operand(value) => Err(error(format!(
            "{value} is a value where a condition belongs; compare it, as in {value} = true"
        ))),
    }
}

/// A parsed part where a value belongs.
fn into_operand(parsed: Parsed) -> Result<Operand, ContractError> {
    match parsed {
        Parsed::Operand(operand) => Ok(operand),
        Parsed::Condition(condition) => Err(error(format!(
            "({condition}) is a condition where a value belongs"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> String {
        condition(text).expect("parses").to_string()
    }

    fn refused(text: &str) -> String {
        condition(text).expect_err("refused").message
    }

    #[test]
    fn the_research_filter_reads_and_prints_back_as_written() {
        let filter =
            "MessageType = 'Order' and not Amount > 1000 and header:http.x-channel = 'web'";
        let read = condition(filter).expect("parses");
        let Condition::All(parts) = &read else {
            panic!("an and at the top: {read:?}");
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(parts[1], Condition::Not(_)));
        assert_eq!(read.to_string(), filter);
    }

    #[test]
    fn precedence_is_sql_s() {
        assert_eq!(
            parsed("a = 1 or b = 2 and c = 3"),
            "a = 1 or (b = 2 and c = 3)"
        );
        assert_eq!(
            parsed("(a = 1 or b = 2) and c = 3"),
            "(a = 1 or b = 2) and c = 3"
        );
        assert_eq!(parsed("not a = 1 and b = 2"), "not a = 1 and b = 2");
        assert_eq!(parsed("not (a = 1 and b = 2)"), "not (a = 1 and b = 2)");
        assert_eq!(parsed("a + b * c = 7"), "a + b * c = 7");
        assert_eq!(parsed("(a + b) * c = 7"), "(a + b) * c = 7");
        assert_eq!(parsed("a || b + 1 = 'x2'"), "a || b + 1 = 'x2'");
        assert_eq!(parsed("a - (b - c) = 0"), "a - (b - c) = 0");
        assert_eq!(parsed("-a = -5"), "-a = -5");
        assert_eq!(
            parsed("a = -9223372036854775808"),
            "a = -9223372036854775808"
        );
    }

    #[test]
    fn a_group_in_parentheses_stays_a_group_and_a_chain_is_one() {
        let nested = condition("a = 1 and (b = 2 and c = 3)").expect("parses");
        assert!(matches!(&nested, Condition::All(parts) if parts.len() == 2));
        let flat = condition("a = 1 and b = 2 and c = 3").expect("parses");
        assert!(matches!(&flat, Condition::All(parts) if parts.len() == 3));
        assert_eq!(nested.to_string(), "a = 1 and (b = 2 and c = 3)");
    }

    #[test]
    fn other_spellings_read_as_the_canonical_one() {
        assert_eq!(parsed("A != 1"), "A <> 1");
        assert_eq!(
            parsed("A IS NOT NULL AND B is null"),
            "exists A and not exists B"
        );
        assert_eq!(
            parsed("NOT a LIKE 'x%' OR a NOT LIKE '%y'"),
            "not a like 'x%' or a not like '%y'"
        );
        assert_eq!(parsed("a Not In ( 1,2 )"), "a not in (1, 2)");
        assert_eq!(parsed("COALESCE(a,'x') = 'x'"), "coalesce(a, 'x') = 'x'");
        assert_eq!(parsed("TRUE"), "true");
        assert_eq!(parsed("(false)"), "false");
        assert_eq!(parsed("\"Amount\" > 1"), "Amount > 1");
        assert_eq!(parsed("a = - 5"), "a = -5");
    }

    #[test]
    fn a_value_where_a_condition_belongs_and_the_reverse_are_refused() {
        assert_eq!(
            refused("Urgent"),
            "expression: Urgent is a value where a condition belongs; compare it, as in \
             Urgent = true"
        );
        assert!(refused("a = 1 and b").contains("b is a value"));
        assert!(refused("(a = 1) = true").contains("is a condition where a value belongs"));
        assert!(refused("exists (a = 1)").contains("where a value belongs"));
    }

    #[test]
    fn what_the_grammar_does_not_have_is_refused() {
        assert!(refused("").contains("needs a condition"));
        assert!(refused("a =").contains("found the end"));
        assert!(refused("a = 1 b = 2").contains("unexpected 'b'"));
        assert!(refused("(a = 1").contains("expected ')'"));
        assert!(refused("a = null").contains("null is not a value"));
        assert!(refused("a is 1").contains("null or not null"));
        assert!(refused("a not = 1").contains("like or in"));
        assert!(refused("a in 1").contains("expected '('"));
        assert!(refused("and = 1").contains("found 'and'"));
        assert!(refused("a = 9223372036854775808").contains("too large"));
        assert!(refused("a = -9223372036854775809").contains("too small"));
        assert!(refused("length(a) = 1").contains("unexpected '('"));
    }

    #[test]
    fn a_value_and_a_list_parse_on_their_own() {
        assert_eq!(operand("a || 'x'").expect("value").to_string(), "a || 'x'");
        assert!(operand("a = 1").is_err());
        let list = operands("'EU', 'US'").expect("list");
        assert_eq!(list.len(), 2);
        assert!(operands("").is_err());
    }
}
