#![forbid(unsafe_code)]

use xmip_contract::{ContractError, StructuredValue, StructureReader, StructureWriter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Path {
    pub language: String,
    pub expression: String,
}

impl Path {
    pub fn new(language: impl Into<String>, expression: impl Into<String>) -> Self {
        Self { language: language.into(), expression: expression.into() }
    }
}

pub trait PathEngine: Send + Sync {
    fn language(&self) -> &'static str;
    fn read(&self, reader: &dyn StructureReader, path: &Path) -> Result<Option<StructuredValue>, ContractError>;
    fn write(&self, writer: &mut dyn StructureWriter, path: &Path, value: StructuredValue) -> Result<(), ContractError>;
}

pub struct DirectPathEngine;

impl PathEngine for DirectPathEngine {
    fn language(&self) -> &'static str { "direct" }

    fn read(&self, reader: &dyn StructureReader, path: &Path) -> Result<Option<StructuredValue>, ContractError> {
        reader.read(&path.expression)
    }

    fn write(&self, writer: &mut dyn StructureWriter, path: &Path, value: StructuredValue) -> Result<(), ContractError> {
        writer.write(&path.expression, value)
    }
}
