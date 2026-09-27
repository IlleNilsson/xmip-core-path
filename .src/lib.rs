#![forbid(unsafe_code)]

// What every technology of this capability shares, held here rather than
// copied into each (ADR-0044): the language a technology implements and the
// one engine that compiles a Path through it, the content a Message's Stream
// is parsed into once and the rewrite that produces a new one, the token
// cursor a parser walks, and the JSON document the JSON languages read and
// rewrite. And Xmip's one expression language (ADR-0066), which route,
// configure and what is compiled from a design all read.
pub mod content;
pub mod cursor;
pub mod expression;
pub mod json;
pub mod rewriting;

pub use content::{Content, Form};
pub use rewriting::{Editable, Rewriting};

use contract::ContractError;
use std::fmt;
use xcore::ScalarValue;

/// A Path as configuration writes it: the language and the expression, text.
/// [`PathEngine::compile`] turns it into a [`CompiledPath`] once.
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

/// One path language — a technology of this capability. It compiles an
/// expression once, when configuration is read; what the compiled expression
/// reads and writes is [`Content`] parsed once per Message.
pub trait PathLanguage: Send + Sync {
    /// The manifest leaf, and the `language` a [`Path`] names.
    fn language(&self) -> &'static str;

    /// `expression`, compiled.
    ///
    /// # Errors
    /// The expression is not one of this language.
    fn compile(&self, expression: &str) -> Result<Box<dyn CompiledExpression>, ContractError>;
}

/// An expression compiled by its language, read and written per Message
/// without being parsed again.
pub trait CompiledExpression: Send + Sync {
    /// The one scalar the expression finds in `content`, or `None` where the
    /// content has none.
    ///
    /// # Errors
    /// The content is not what the language reads, or the expression finds
    /// an object or an array rather than a value.
    fn read(&self, content: &Content<'_>) -> Result<Option<ScalarValue>, ContractError>;

    /// Set `value` at the place the expression names.
    ///
    /// # Errors
    /// The content is not what the language writes, the expression names no
    /// place, or the value has no form there.
    fn write(&self, rewriting: &mut Rewriting, value: ScalarValue) -> Result<(), ContractError>;
}

/// A [`Path`] compiled once by the language it names.
pub struct CompiledPath {
    path: Path,
    compiled: Box<dyn CompiledExpression>,
}

impl CompiledPath {
    /// The Path this was compiled from.
    #[must_use]
    pub const fn path(&self) -> &Path {
        &self.path
    }

    /// See [`CompiledExpression::read`].
    ///
    /// # Errors
    /// As [`CompiledExpression::read`].
    pub fn read(&self, content: &Content<'_>) -> Result<Option<ScalarValue>, ContractError> {
        self.compiled.read(content)
    }

    /// See [`CompiledExpression::write`].
    ///
    /// # Errors
    /// As [`CompiledExpression::write`].
    pub fn write(
        &self,
        rewriting: &mut Rewriting,
        value: ScalarValue,
    ) -> Result<(), ContractError> {
        self.compiled.write(rewriting, value)
    }
}

impl fmt::Debug for CompiledPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CompiledPath").field(&self.path).finish()
    }
}

/// The one engine: the path languages loaded, and the compilation of a
/// [`Path`] through the one it names. Which languages there are is
/// configuration; nothing here knows any of them.
#[derive(Default)]
pub struct PathEngine {
    languages: Vec<Box<dyn PathLanguage>>,
}

impl PathEngine {
    /// An engine carrying `languages`.
    #[must_use]
    pub fn new(languages: Vec<Box<dyn PathLanguage>>) -> Self {
        Self { languages }
    }

    /// The languages loaded, in the order they were given.
    pub fn languages(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.languages.iter().map(|language| language.language())
    }

    /// Compile `path` through the language it names.
    ///
    /// # Errors
    /// No loaded language is the one `path` names, or it refused the
    /// expression.
    pub fn compile(&self, path: &Path) -> Result<CompiledPath, ContractError> {
        let language = self
            .languages
            .iter()
            .find(|language| language.language() == path.language)
            .ok_or_else(|| {
                let loaded: Vec<&str> = self.languages().collect();
                ContractError::new(format!(
                    "{} is not a path language loaded here; the languages are {}",
                    path.language,
                    if loaded.is_empty() {
                        "none".to_string()
                    } else {
                        loaded.join(", ")
                    }
                ))
            })?;
        Ok(CompiledPath {
            path: path.clone(),
            compiled: language.compile(&path.expression)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use stream::Stream;
    use xcore::StreamId;

    /// A language whose expression is a JSON object's member name, counting
    /// how often it compiles.
    struct Member(&'static AtomicUsize);

    struct Named(String);

    impl PathLanguage for Member {
        fn language(&self) -> &'static str {
            "member"
        }

        fn compile(&self, expression: &str) -> Result<Box<dyn CompiledExpression>, ContractError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(Named(expression.to_string())))
        }
    }

    impl CompiledExpression for Named {
        fn read(&self, content: &Content<'_>) -> Result<Option<ScalarValue>, ContractError> {
            let value = content.form::<serde_json::Value>()?;
            value
                .get(&self.0)
                .map(|found| json::scalar(found, &self.0))
                .transpose()
        }

        fn write(
            &self,
            rewriting: &mut Rewriting,
            value: ScalarValue,
        ) -> Result<(), ContractError> {
            let document = rewriting.form_mut::<serde_json::Value>()?;
            document[&self.0] = json::from_scalar(value)?;
            Ok(())
        }
    }

    #[test]
    fn the_engine_compiles_through_the_language_a_path_names_and_refuses_others() {
        static COMPILED: AtomicUsize = AtomicUsize::new(0);
        let engine = PathEngine::new(vec![Box::new(Member(&COMPILED))]);
        let compiled = engine
            .compile(&Path::new("member", "id"))
            .expect("compiles");
        assert_eq!(compiled.path(), &Path::new("member", "id"));
        assert_eq!(
            format!("{compiled:?}"),
            format!("CompiledPath({:?})", compiled.path())
        );

        let refused = engine
            .compile(&Path::new("xpath", "/a"))
            .expect_err("not loaded");
        assert_eq!(
            refused.message,
            "xpath is not a path language loaded here; the languages are member"
        );
        let empty = PathEngine::default()
            .compile(&Path::new("member", "id"))
            .expect_err("none loaded");
        assert!(empty.message.ends_with("the languages are none"));
    }

    #[test]
    fn a_path_compiles_once_and_reads_every_message_without_compiling_again() {
        static COMPILED: AtomicUsize = AtomicUsize::new(0);
        let engine = PathEngine::new(vec![Box::new(Member(&COMPILED))]);
        let compiled = engine
            .compile(&Path::new("member", "id"))
            .expect("compiles");
        for number in 0..1000 {
            let stream = Stream::new(
                StreamId::new(1),
                format!(r#"{{"id":{number}}}"#).into_bytes(),
                None,
            );
            let content = Content::of(&stream);
            assert_eq!(
                compiled.read(&content).expect("reads"),
                Some(ScalarValue::Integer(number))
            );
        }
        assert_eq!(COMPILED.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_compiled_path_writes_through_a_rewriting() {
        static COMPILED: AtomicUsize = AtomicUsize::new(0);
        let engine = PathEngine::new(vec![Box::new(Member(&COMPILED))]);
        let compiled = engine
            .compile(&Path::new("member", "id"))
            .expect("compiles");
        let stream = Stream::new(StreamId::new(1), br#"{"id":1}"#.to_vec(), None);
        let mut rewriting = Rewriting::of(&stream, StreamId::new(2));
        compiled
            .write(&mut rewriting, ScalarValue::Integer(2))
            .expect("writes");
        assert_eq!(
            rewriting.finish().expect("finishes").bytes(),
            br#"{"id":2}"#
        );
    }
}
