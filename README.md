# rojo-schema

JSON Schema completion, documentation, and validation for Rojo project and JSON
model files.

## Use it

Add `$schema` at the top level of a `.project.json` or `.project.jsonc` file:

```json
{
  "$schema": "https://0hirume.github.io/rojo-schema/latest/project.schema.json",
  "name": "MyProject",
  "tree": {}
}
```

`.model.json` and `.model.jsonc` files use the model schema:

```json
{
  "$schema": "https://0hirume.github.io/rojo-schema/latest/model.schema.json",
  "className": "Part",
  "properties": {
    "Anchored": true
  }
}
```

> [!NOTE]
> Recent VS Code versions require approval before downloading schemas from a
> new domain. If the schema URL is reported as untrusted, use its quick fix and
> choose **Trust URI** or **Trust Domain**. You can also add
> `https://0hirume.github.io` to `json.schemaDownload.trustedDomains` manually.

Any editor with JSON Schema support can then provide:

- Rojo project, tree, and JSON model completion
- Roblox class, property, enum, and value completion
- Property descriptions and deprecation messages
- Validation for Rojo's project and JSON model formats

The latest schemas update automatically when their upstream sources change.
The [schema page](https://0hirume.github.io/rojo-schema/) links to the current
schemas, their manifest and coverage report, and every immutable snapshot.

## How it is generated

`rojo-schema` combines three sources without modifying them:

- Rojo defines the project, tree, and JSON model grammars.
- Rojo's reflection database defines Roblox classes, properties, types, enums,
  defaults, and serialization metadata.
- Roblox Creator Docs provides descriptions, deprecations, security, thread
  safety, capabilities, and documentation tags.

The generated schema uses JSON Schema Draft 2020-12. Its manifest records the
exact source revisions and hashes, while its coverage report shows how API and
reflection entries were reconciled.

## Development

Generation requires local Rojo and Creator Docs checkouts:

```console
cargo run --locked -- generate --rojo C:/path/to/rojo --docs C:/path/to/creator-docs
cargo run --locked -- check --rojo C:/path/to/rojo --docs C:/path/to/creator-docs
```

`generate` writes the project schema, model schema, manifest, and coverage
report to the ignored `dist` directory. `check` verifies that generation is
deterministic and the existing artifacts are current. `--docs` also accepts the
Creator Docs `content/en-us/reference/engine` directory.

Tests use `ROJO_SCHEMA_ROJO` and `ROJO_SCHEMA_DOCS` for the same source paths:

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
```

## Limits

JSON Schema cannot inspect the class produced by an arbitrary filesystem path,
check whether paths exist, execute Rojo's glob compiler, or reproduce
runtime-only resolution checks. Values that cannot be derived safely from the
sources are represented conservatively rather than guessed.
