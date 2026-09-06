# cratestack-parser

Parser and semantic checker for `.cstack` schema files.

## Overview

`cratestack-parser` turns `.cstack` source text into a `cratestack_core::Schema` AST and runs semantic validation. The `include_server_schema!` / `include_embedded_schema!` / `include_client_schema!` proc-macros call this crate at compile time; the LSP and CLI call it at runtime.

## Installation

```toml
[dependencies]
cratestack-parser = "0.7"
```

## Usage

### Parse from a string

```rust
use cratestack_parser::parse_schema;

let source = r#"
auth Principal {
  id String
  role String?
}

model Post {
  id String @id
  title String
  published Boolean @default(false)
  authorId String

  @@allow("read", auth() != null)
}
"#;

let schema = parse_schema_named("schema.cstack", source)?;
```

### Parse from a file

```rust
use cratestack_parser::parse_schema_file;

let schema = parse_schema_file("schema.cstack")?;
```

### Named source for diagnostics

```rust
use cratestack_parser::parse_schema_named;

let schema = parse_schema_named("my-app/schema.cstack", source)?;
```

## Errors

`SchemaError` carries a source span, a line number, and the file it came from, plus an
ariadne-rendered report:

```rust
use cratestack_parser::{SchemaError, parse_schema};

match parse_schema_named("schema.cstack", source) {
    Ok(schema) => { /* ... */ }
    Err(error) => {
        eprintln!("{}", error.message());
        eprintln!("line {}", error.line());
        eprintln!("file {}", error.file());
        let span = error.span();
        eprintln!("bytes {}..{}", span.start, span.end);
        eprintln!("{}", error.render());
    }
}
```

## Supported Constructs

| Construct       | Description                                                |
|-----------------|------------------------------------------------------------|
| `datasource`    | Database config (`provider = "postgresql"` or `"sqlite"`)  |
| `auth`          | Single auth block declaring principal fields               |
| `mixin`         | Reusable field set, applied via `@use(...)` on a model     |
| `model`         | Entity with fields, relations, and policy attributes       |
| `type`          | Named record type (`@computed` resolver fields supported)  |
| `enum`          | Untyped identifier variants                                |
| `procedure`     | `procedure` / `mutation procedure` with typed args/return  |
| `mcp`           | Parsed as a config block                                   |

See the root README for the canonical capability matrix.

## Field and Model Attributes

See [Field Attributes](https://cratestack.dev/reference/field-attributes) for the full list. Common attributes:

- `@id`, `@unique`, `@relation(...)`, `@default(...)`
- `@readonly`, `@server_only`, `@pii`, `@sensitive`
- `@version` (optimistic locking)
- `@length`, `@range`, `@email`, `@regex`, `@uri`, `@iso4217` (validators)
- `@@allow(action, expr)`, `@@deny(action, expr)`
- `@@audit`, `@@soft_delete`
- `@@emit(created, updated, deleted)`

## See Also

- [Mixins reference](https://cratestack.dev/reference/mixins)
- [Field Attributes](https://cratestack.dev/reference/field-attributes)
- [Validators guide](https://cratestack.dev/guides/validators)
- `cratestack-macros` — the proc-macro that drives this parser at compile time
- `cratestack-lsp` — LSP frontend over the same parser

## License

MIT
