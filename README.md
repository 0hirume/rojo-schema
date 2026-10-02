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

If VS Code marks a schema URL as untrusted, use its quick fix to choose
**Trust URI** or **Trust Domain**.

Schemas update automatically. The [schema page](https://0hirume.github.io/rojo-schema/)
includes current schemas, provenance, coverage, and immutable snapshots.
