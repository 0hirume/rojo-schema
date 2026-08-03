# rojo-schema

`rojo-schema` compiles the local Rojo implementation, Rojo's pinned rbx-dom
reflection database, and Roblox Creator Docs into a deterministic,
self-contained JSON Schema for `.project.json` and `.project.jsonc` files.

The generated Draft 2020-12 schema is class-aware and property-aware. It has a
discriminated node definition for every reflected Roblox class, flattened
inherited properties, source-traced Roblox value forms, enum completion, and
Rojo's project, path, sync-rule, syncback-rule, and attribute grammar.

## Sources and authority

The compiler keeps its three inputs distinct and applies these rules:

- Rojo's Rust types and Serde attributes control project grammar, middleware
  names, path forms, sync rules, and unresolved-value syntax.
- The pinned reflection database controls serialization names, aliases,
  migrations, types, defaults, services, serialization metadata, and the
  concrete value samples used to trace custom Serde implementations.
- Creator Docs controls current API existence, descriptions, deprecations,
  security, thread safety, capabilities, and documentation tags.

Version skew is not hidden. Every discovered item receives a coverage
classification such as `matched`, `api-only`, `reflection-only`,
`type-conflict`, `metadata-conflict`, or `non-projectable`. Source revisions,
versions, paths, and SHA-256 hashes are recorded in the manifest.

The source checkouts are read-only inputs. Generation never writes into them.

## Usage

From this directory, with the default sibling checkouts:

```console
cargo run --locked -- generate
cargo run --locked -- check
```

`generate` writes all three artifacts. `check` generates twice in memory,
rejects nondeterministic output, and fails if any checked-in artifact differs.

Every path can be overridden for either command:

```console
cargo run --locked -- generate --rojo C:/src/rojo --docs C:/src/creator-docs --output dist/rojo.schema.json --manifest dist/manifest.json --coverage dist/coverage.json
```

`--docs` accepts either the Creator Docs checkout or its
`content/en-us/reference/engine` directory.

The reusable library exposes `Config`, `generate`, `write`, and `check`:

```rust
use rojo_schema::{Config, generate, write};

let config = Config::default();
let artifacts = generate(&config)?;
write(&config, &artifacts)?;
# Ok::<(), anyhow::Error>(())
```

## Outputs

- `dist/rojo.schema.json` — self-contained Draft 2020-12 project schema.
- `dist/manifest.json` — schema identity, exact source provenance, counts, and
  fundamental limitations.
- `dist/coverage.json` — every API/reflection item and its disposition,
  diagnostics, source-version reconciliation, and all encountered
  `VariantType` values.

Definitions use stable direct keys such as `value/Vector3`, `enum/Material`,
`property/Instance/Name`, `properties/Part`, `node/Part`, `rojo/Project`, and
`serde/Vector3`. This keeps the large schema reusable while preserving editor
completion for each class.

The checked-in schema can be consumed directly from this repository:

```json
{
  "$schema": "https://github.com/0hirume/rojo-schema/raw/refs/heads/main/dist/rojo.schema.json"
}
```

Pin a commit in that URL when a consumer needs immutable validation behavior.

## Validation

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
cargo run --locked -- generate
cargo run --locked -- check
```

Tests cover every source-traced Roblox variant, flattened inheritance,
enum/middleware completion, optional paths, root fields, recursive nodes,
Draft 2020-12 compilation, deterministic bytes, focused local fixtures, and
representative valid fixtures from Rojo itself.

## JSON Schema limits

JSON Schema cannot know the class produced by an arbitrary filesystem path,
check path existence, execute Rojo's glob compiler, or reproduce runtime-only
resolution checks. Custom serialized values without a reflected default sample
are intentionally represented conservatively instead of being guessed. These
limits are repeated in the generated manifest.
