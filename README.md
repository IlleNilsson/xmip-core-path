# xmip-core-path

Declared Paths: an addressing language and an expression, used to address
values in structured content and in artifacts. A `PathEngine` evaluates one
language and reports its `PathCost`; each language — XPath, JSON Pointer and
the rest — is a technology mounted directly under this repository.

A Path addresses; it does not parse, serialize or evaluate a Contract. The
representation technologies materialize content, this addresses it, and a
Contract evaluates it, and none absorbs the other two
(`repository-model.md` section 5).

`doc/architecture/runtime-model.md` section 9, *Path*, governs it;
`architecture.toml` names the languages and carries the maturity.
