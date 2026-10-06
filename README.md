# xmip-core-path

Declared Paths: an addressing language and an expression, used to address
values in structured content and in artifacts. Each language — XPath, JSON
Pointer and the rest — is a technology mounted directly under this repository
and implements `PathLanguage`; the one `PathEngine` carries the languages
configuration loads and compiles a `Path` through the one it names, once,
into a `CompiledPath`.

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

**Xmip's expression language** (`expression`, ADR-0066) is this
capability's own, not a technology of it: the one grammar a route's filter,
a transform's conditions and a Work Process's decisions are written in,
read by route, by `configure` (a platform service, which may depend on a
capability and never on a technology) and by what is compiled from a
design.

```text
MessageType = 'Order' and not Amount > 1000 and header:http.x-channel = 'web'
```

Shaped like SQL's WHERE clause and deliberately small: names bare with
their source's prefix, or in double quotes; text in single quotes, a quote
doubled; 64-bit integers and `true`/`false`, no decimals; `=`, `<>` (`!=`
reads as it, `==` is refused), `<`, `<=`, `>`, `>=`, `[not] like`,
`[not] in (…)`, `exists` (and `is [not] null`), `and`, `or`, `not`; `||`,
`+ - * /` and `coalesce`. No loops, no functions of the user's own, no I/O.
`Expression::parse` parses once and checks every part's kind — a name takes
the kind of what it meets, so `Amount > 1000` reads Amount as an integer —
and `evaluate` decides the tree over any `Names` without parsing again, in
SQL's three truths: a value that is not there is *unknown* with its reason,
never a silent false. A `Condition` prints as the one canonical spelling a
designer writes back; an `Expression` keeps the text it was read from.
Measured on a release build: the line above compiles in about 6 µs and is
decided in under 2 µs. Its lexer reads through `xmip-core-library-codec`'s
character reader and its parser walks this crate's `cursor`, as FHIRPath's
does.

`doc/architecture/runtime-model.md` section 9, *Path*, governs it;
`architecture.toml` names the languages and carries the maturity.
