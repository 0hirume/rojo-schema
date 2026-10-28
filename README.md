# rojo-schema

JSON Schema completion, documentation, and validation for Rojo project and JSON
model files.

## Usage

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

### VS Code

In VS Code, you can configure schema associations in your user or workspace
`settings.json` so you don't need to add `$schema` to each file:

```json
{
  "json.schemas": [
    {
      "fileMatch": ["*.project.json", "*.project.jsonc"],
      "url": "https://0hirume.github.io/rojo-schema/latest/project.schema.json"
    },
    {
      "fileMatch": ["*.model.json", "*.model.jsonc"],
      "url": "https://0hirume.github.io/rojo-schema/latest/model.schema.json"
    }
  ]
}
```

If VS Code marks a schema URL as untrusted, use its quick fix to choose
**Trust URI** or **Trust Domain**.

### Zed

Add schema associations to your Zed settings or `.zed/settings.json` to apply
them without a `$schema` declaration in each file:

```json
{
  "lsp": {
    "json-language-server": {
      "settings": {
        "json": {
          "schemas": [
            {
              "fileMatch": ["*.project.json", "*.project.jsonc"],
              "url": "https://0hirume.github.io/rojo-schema/latest/project.schema.json"
            },
            {
              "fileMatch": ["*.model.json", "*.model.jsonc"],
              "url": "https://0hirume.github.io/rojo-schema/latest/model.schema.json"
            }
          ]
        }
      }
    }
  }
}
```

### Neovim

With Neovim 0.11+, `nvim-lspconfig`, and the JSON language server installed, add
these associations to your `jsonls` configuration. They apply without a
`$schema` declaration in each file:

```lua
vim.lsp.config("jsonls", {
  settings = {
    json = {
      schemas = {
        {
          fileMatch = { "*.project.json", "*.project.jsonc" },
          url = "https://0hirume.github.io/rojo-schema/latest/project.schema.json",
        },
        {
          fileMatch = { "*.model.json", "*.model.jsonc" },
          url = "https://0hirume.github.io/rojo-schema/latest/model.schema.json",
        },
      },
    },
  },
})

vim.lsp.enable("jsonls")
```

Schemas update automatically.
