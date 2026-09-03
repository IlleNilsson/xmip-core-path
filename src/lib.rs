#![forbid(unsafe_code)]

use xmip_contract::{ContractError, StructureReader, StructureWriter, StructuredValue};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Path {
    pub language: String,
    pub expression: String,
}

impl Path {
    pub fn new(language: impl Into<String>, expression: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            expression: expression.into(),
        }
    }
}

/// How much of a Stream must be read before an expression can be answered.
///
/// A Stream may be larger than memory, so this is the difference between
/// glancing at a header and paying for the whole payload. Routing wants the
/// cheap answers first: a Subscription that can be decided from a prefix should
/// never force a scan of everything behind it.
///
/// Arrived as `SelectorEvaluation` from the platform repository's
/// `src/contracts.rs`, which is otherwise superseded by `Path` and `PathEngine`.
/// The cost was the one idea in it that this module did not already have.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PathCost {
    /// Answerable from the front of the Stream. Bounded, and cheap enough to
    /// run against every candidate.
    StreamPrefix,
    /// Needs a pass over the Stream, but not a copy of it. Cost grows with the
    /// payload; memory does not.
    StreamScan,
    /// Needs the whole Stream resident and parsed. The expensive answer, and
    /// the reason the other two are worth distinguishing.
    Materialized,
}

pub trait PathEngine: Send + Sync {
    fn language(&self) -> &'static str;
    fn read(
        &self,
        reader: &dyn StructureReader,
        path: &Path,
    ) -> Result<Option<StructuredValue>, ContractError>;
    fn write(
        &self,
        writer: &mut dyn StructureWriter,
        path: &Path,
        value: StructuredValue,
    ) -> Result<(), ContractError>;

    /// Defaults to the expensive answer on purpose. An engine that has not
    /// thought about cost should not be trusted to promise a cheap one, and
    /// over-estimating only loses an optimisation.
    fn cost(&self, _path: &Path) -> PathCost {
        PathCost::Materialized
    }
}

pub struct DirectPathEngine;

impl PathEngine for DirectPathEngine {
    fn language(&self) -> &'static str {
        "direct"
    }

    fn read(
        &self,
        reader: &dyn StructureReader,
        path: &Path,
    ) -> Result<Option<StructuredValue>, ContractError> {
        reader.read(&path.expression)
    }

    fn write(
        &self,
        writer: &mut dyn StructureWriter,
        path: &Path,
        value: StructuredValue,
    ) -> Result<(), ContractError> {
        writer.write(&path.expression, value)
    }

    /// A direct expression names one place. Whatever the reader had to do to
    /// get there, this engine adds no traversal of its own.
    fn cost(&self, _path: &Path) -> PathCost {
        PathCost::StreamPrefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheaper_costs_order_before_expensive_ones() {
        let mut costs = vec![
            PathCost::Materialized,
            PathCost::StreamPrefix,
            PathCost::StreamScan,
        ];
        costs.sort();

        assert_eq!(
            costs,
            vec![
                PathCost::StreamPrefix,
                PathCost::StreamScan,
                PathCost::Materialized
            ]
        );
    }

    #[test]
    fn an_engine_that_says_nothing_is_assumed_expensive() {
        struct Silent;

        impl PathEngine for Silent {
            fn language(&self) -> &'static str {
                "silent"
            }

            fn read(
                &self,
                _reader: &dyn StructureReader,
                _path: &Path,
            ) -> Result<Option<StructuredValue>, ContractError> {
                Ok(None)
            }

            fn write(
                &self,
                _writer: &mut dyn StructureWriter,
                _path: &Path,
                _value: StructuredValue,
            ) -> Result<(), ContractError> {
                Ok(())
            }
        }

        assert_eq!(
            Silent.cost(&Path::new("silent", "/a")),
            PathCost::Materialized
        );
    }
}
