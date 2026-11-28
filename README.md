# xmip-core-path

Declared Paths: an addressing language and an expression, used to address
values in structured content and in artifacts. Each language — XPath, JSON
Pointer and the rest — is a technology mounted directly under this repository
and implements `PathLanguage`; the one `PathEngine` carries the languages
configuration loads and compiles a `Path` through the one it names, once,
into a `CompiledPath`.

**A Path's prefixes are its configuration's.** A `Path` is a language, an
expression and `Namespaces`, the table binding each prefix the expression
uses to a URI. `PathLanguage::compile` receives the table; a prefix resolves
through it and never through the content, a prefix it does not bind is
refused as the expression compiles, and a language without namespaces
refuses a table that binds anything (`Namespaces::unused_by`; ADR-0066,
amendment 2026-11-08).

**Once, per message.** Nothing on the message path parses an expression: it
was compiled with the configuration. A compiled path reads `Content`, one
Stream parsed at most once per form (`Form`) however many paths read it —
`dot`, `jsonpath`, `json-pointer` and `fhirpath` share one JSON parse through
`json`, the text languages borrow the Stream's own text — and writes through
`Rewriting`, which opens the Stream once into the form a language edits
(`Editable`), lets every write land there, and writes it back once, as
ADR-0013's new Stream. Two forms in one rewrite close the first before the
second opens, so a later write sees an earlier one. The tests hold it: a
path compiled once reads a thousand Messages without compiling again, and a
form is parsed once however often it is asked for.

A Path addresses; it does not parse, serialize or evaluate a Contract. The
representation technologies materialize content, this addresses it, and a
Contract evaluates it, and none absorbs the other two
(`repository-model.md` section 5).

**Xmip's expression language is not here.** It is a capability of its
own, [xmip-core-expression](https://github.com/IlleNilsson/xmip-core-expression),
whose parser walks this crate's `cursor` as FHIRPath's does; it depends on
path, and path never on it (ADR-0066, amendment 2026-11-28).

`doc/architecture/runtime-model.md` section 9, *Path*, governs it;
`architecture.toml` names the languages and carries the maturity.
