//! A condition decided over one set of names, in SQL's three truths.
//!
//! *True*, *false* with the reason it failed, or *unknown* with the reason a
//! value was not there — never a silent false. `and` is false when any part
//! is false and unknown when none is false but one is unknown; `or` is true
//! when any part is true; `not` of unknown is unknown. So `not Amount > 1000`
//! does not hold for a Message without an Amount: nothing said it was not
//! over 1000. Only *true* matches.
//!
//! Every decision explains itself: a false or an unknown carries the
//! sentence a person reads to see why.

use std::cmp::Ordering;

use super::Names;
use super::condition::{Comparison, Condition};
use super::operand::Operand;
use super::reading::{Settled, like, read};

/// What a condition decided, with the reason when it did not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Truth {
    True,
    False(String),
    Unknown(String),
}

impl Truth {
    /// Whether it holds: only *true* does.
    #[must_use]
    pub const fn holds(&self) -> bool {
        matches!(self, Self::True)
    }

    /// Why it did not hold; `None` when it did.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::True => None,
            Self::False(why) | Self::Unknown(why) => Some(why),
        }
    }
}

impl Condition {
    /// Decide this condition over `names`. Nothing is parsed here: the
    /// condition was parsed once, when it was compiled.
    #[must_use]
    pub fn evaluate(&self, names: &dyn Names) -> Truth {
        self.decide(names, true)
    }

    /// Decide, writing the reason a part did not hold only when `explain`
    /// asks for it: under `not`, and inside an `or` that holds, a reason
    /// would be written only to be thrown away.
    fn decide(&self, names: &dyn Names, explain: bool) -> Truth {
        match self {
            Self::All(parts) => all(parts, names, explain),
            Self::Any(parts) => match any(parts, names, false) {
                Truth::True => Truth::True,
                _ if explain => any(parts, names, true),
                undecided => undecided,
            },
            Self::Not(inner) => match inner.decide(names, false) {
                Truth::True => Truth::False(why(explain, || format!("{inner} holds"))),
                Truth::False(_) => Truth::True,
                Truth::Unknown(_) if explain => inner.decide(names, true),
                unknown @ Truth::Unknown(_) => unknown,
            },
            Self::Compare {
                left,
                comparison,
                right,
            } => compare(names, left, right, |settled| {
                compared(left, *comparison, settled, explain)
            }),
            Self::Like {
                value,
                pattern,
                negated,
            } => match (read(value, names), read(pattern, names)) {
                (Err(why), _) | (_, Err(why)) => Truth::Unknown(why),
                (Ok(text), Ok(pattern)) => {
                    if like(&text.text(), &pattern.text()) == *negated {
                        let word = if *negated {
                            "which is like"
                        } else {
                            "which is not like"
                        };
                        Truth::False(why(explain, || {
                            format!("{value} is {}, {word} {}", text.show(), pattern.show())
                        }))
                    } else {
                        Truth::True
                    }
                }
            },
            Self::In {
                value,
                list,
                negated,
            } => within(names, value, list, *negated, explain),
            Self::Exists(value) => match read(value, names) {
                Ok(_) => Truth::True,
                Err(why) => Truth::False(why),
            },
        }
    }
}

/// The reason, when it is asked for; nothing written otherwise.
fn why(explain: bool, reason: impl FnOnce() -> String) -> String {
    if explain { reason() } else { String::new() }
}

fn all(parts: &[Condition], names: &dyn Names, explain: bool) -> Truth {
    let mut unknown = None;
    for part in parts {
        match part.decide(names, explain) {
            Truth::True => {}
            failed @ Truth::False(_) => return failed,
            Truth::Unknown(why) => {
                unknown.get_or_insert(why);
            }
        }
    }
    unknown.map_or(Truth::True, Truth::Unknown)
}

fn any(parts: &[Condition], names: &dyn Names, explain: bool) -> Truth {
    if parts.is_empty() {
        return Truth::False(why(explain, || "no condition was given".to_string()));
    }
    let mut reasons = Vec::new();
    let mut unknown = false;
    for part in parts {
        match part.decide(names, explain) {
            Truth::True => return Truth::True,
            Truth::False(why) => reasons.push(why),
            Truth::Unknown(why) => {
                unknown = true;
                reasons.push(why);
            }
        }
    }
    let reasons = reasons.join("; and ");
    if unknown {
        Truth::Unknown(reasons)
    } else {
        Truth::False(reasons)
    }
}

/// Read both sides, settle them on one kind and let `decide` compare them;
/// a side with no value, or one that will not read as the other's kind, is
/// unknown.
fn compare(
    names: &dyn Names,
    left: &Operand,
    right: &Operand,
    decide: impl Fn(&Settled<'_>) -> Truth,
) -> Truth {
    let settled = read(left, names).and_then(|left| Ok((left, read(right, names)?)));
    match settled.and_then(|(left, right)| left.settle(right)) {
        Ok(settled) => decide(&settled),
        Err(why) => Truth::Unknown(why),
    }
}

fn compared(left: &Operand, comparison: Comparison, settled: &Settled<'_>, explain: bool) -> Truth {
    let order = settled.order();
    let (holds, sentence) = match comparison {
        Comparison::Equal => (order == Ordering::Equal, "not"),
        Comparison::NotEqual => (order != Ordering::Equal, ""),
        Comparison::Less => (order == Ordering::Less, "which is not under"),
        Comparison::LessOrEqual => (order != Ordering::Greater, "which is over"),
        Comparison::Greater => (order == Ordering::Greater, "which is not over"),
        Comparison::GreaterOrEqual => (order != Ordering::Less, "which is under"),
    };
    if holds {
        return Truth::True;
    }
    if !explain {
        return Truth::False(String::new());
    }
    let (actual, wanted) = settled.shown();
    Truth::False(if sentence.is_empty() {
        format!("{left} is {wanted}")
    } else {
        format!("{left} is {actual}, {sentence} {wanted}")
    })
}

/// `in`: equal to one of the list. Unknown when none is equal and one of
/// them could not be read, as SQL has it.
fn within(
    names: &dyn Names,
    value: &Operand,
    list: &[Operand],
    negated: bool,
    explain: bool,
) -> Truth {
    let reading = match read(value, names) {
        Ok(reading) => reading,
        Err(why) => return Truth::Unknown(why),
    };
    let mut unknown = None;
    let mut found = false;
    for item in list {
        match read(item, names).and_then(|item| reading.clone().settle(item)) {
            Ok(settled) if settled.order() == Ordering::Equal => {
                found = true;
                break;
            }
            Ok(_) => {}
            Err(why) => {
                unknown.get_or_insert(why);
            }
        }
    }
    if !found && let Some(why) = unknown {
        return Truth::Unknown(why);
    }
    if found != negated {
        return Truth::True;
    }
    if !explain {
        return Truth::False(String::new());
    }
    let word = if negated { "in" } else { "not in" };
    let list: Vec<String> = list.iter().map(ToString::to_string).collect();
    Truth::False(format!(
        "{value} is {}, which is {word} ({})",
        reading.show(),
        list.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::parser::condition;
    use std::collections::BTreeMap;

    struct Promoted(BTreeMap<&'static str, &'static str>);

    impl Names for Promoted {
        fn value(&self, name: &str) -> Option<&str> {
            self.0.get(name).copied()
        }

        fn absent(&self, name: &str) -> String {
            format!("nothing promoted {name}")
        }
    }

    fn orders() -> Promoted {
        Promoted(BTreeMap::from([
            ("MessageType", "Order"),
            ("Amount", "1500"),
            ("Customer", "EU-0042"),
            ("Urgent", "true"),
            ("header:http.x-channel", "web"),
        ]))
    }

    fn truth(text: &str) -> Truth {
        condition(text).expect("parses").evaluate(&orders())
    }

    #[test]
    fn the_research_filter_decides_with_its_reasons() {
        let filter =
            "MessageType = 'Order' and not Amount > 1000 and header:http.x-channel = 'web'";
        assert_eq!(
            truth(filter),
            Truth::False("Amount > 1000 holds".to_string())
        );
        assert!(truth(&filter.replace("1000", "2000")).holds());
    }

    #[test]
    fn every_comparison_says_why_it_failed() {
        for (text, why) in [
            (
                "MessageType = 'Invoice'",
                "MessageType is 'Order', not 'Invoice'",
            ),
            ("MessageType <> 'Order'", "MessageType is 'Order'"),
            ("Amount < 900", "Amount is 1500, which is not under 900"),
            ("Amount <= 900", "Amount is 1500, which is over 900"),
            ("Amount > 2000", "Amount is 1500, which is not over 2000"),
            ("Amount >= 2000", "Amount is 1500, which is under 2000"),
            (
                "Customer like 'US%'",
                "Customer is 'EU-0042', which is not like 'US%'",
            ),
            (
                "Customer not like 'EU%'",
                "Customer is 'EU-0042', which is like 'EU%'",
            ),
            (
                "Amount in (1, 2)",
                "Amount is '1500', which is not in (1, 2)",
            ),
            (
                "Amount not in (1500)",
                "Amount is '1500', which is in (1500)",
            ),
            ("exists Region", "nothing promoted Region"),
        ] {
            assert_eq!(truth(text), Truth::False(why.to_string()), "{text}");
        }
    }

    #[test]
    fn the_literal_s_kind_is_the_kind_read() {
        // "1500" read as an integer is over 900; read as text it is not.
        assert!(truth("Amount > 900").holds());
        assert!(!truth("Amount > '900'").holds());
        assert!(truth("Urgent = true and Customer like 'EU-00_2'").holds());
        assert!(truth("Amount in (1500, 7) and MessageType not in ('Invoice')").holds());
        assert!(truth("Amount + 500 = 2000 and Customer || '!' = 'EU-0042!'").holds());
    }

    #[test]
    fn a_missing_value_is_unknown_never_a_silent_false() {
        let unknown = |why: &str| Truth::Unknown(why.to_string());
        assert_eq!(truth("Region = 'SE'"), unknown("nothing promoted Region"));
        assert_eq!(
            truth("not Region = 'SE'"),
            unknown("nothing promoted Region")
        );
        assert_eq!(truth("Region <> 'SE'"), unknown("nothing promoted Region"));
        assert_eq!(
            truth("Customer > 5"),
            unknown("Customer is 'EU-0042', which is not an integer")
        );
        assert_eq!(
            truth("MessageType = 'Order' and Region = 'SE'"),
            unknown("nothing promoted Region")
        );
        assert_eq!(
            truth("MessageType = 'Invoice' and Region = 'SE'"),
            Truth::False("MessageType is 'Order', not 'Invoice'".to_string())
        );
        assert!(truth("MessageType = 'Order' or Region = 'SE'").holds());
        assert_eq!(
            truth("MessageType = 'Invoice' or Region = 'SE'"),
            unknown("MessageType is 'Order', not 'Invoice'; and nothing promoted Region")
        );
        assert_eq!(
            truth("Region in ('SE')"),
            unknown("nothing promoted Region")
        );
        assert_eq!(
            truth("Amount in (7, Limit)"),
            unknown("nothing promoted Limit")
        );
        assert!(truth("Amount in (1500, Limit)").holds());
        assert_eq!(
            truth("Amount not in (7, Limit)"),
            unknown("nothing promoted Limit")
        );
        assert!(truth("not (Region = 'SE' or MessageType = 'Order') or true").holds());
        assert_eq!(
            truth("not (Region = 'SE' and MessageType = 'Order')"),
            unknown("nothing promoted Region")
        );
        assert!(truth("not exists Region and coalesce(Region, 'EU') = 'EU'").holds());
    }

    #[test]
    fn true_takes_everything_and_false_nothing() {
        assert!(truth("true").holds());
        assert_eq!(
            truth("false"),
            Truth::False("no condition was given".to_string())
        );
        assert_eq!(truth("not true"), Truth::False("true holds".to_string()));
    }
}
