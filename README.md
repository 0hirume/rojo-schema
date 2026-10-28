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
  "ClassName": "Part",
  "Properties": {
    "Anchored": true
  }
}
```

### Editor settings

Configure schema associations once to omit `$schema` from individual files.

#### VS Code and Zed

Use this array in your user or project settings:

- In VS Code, set `"json.schemas"` in `settings.json` to this array.
- In Zed settings or `.zed/settings.json`, place it under the nested
  `lsp.json-language-server.settings.json.schemas` keys.

```json
[
  {
    "fileMatch": ["*.project.json", "*.project.jsonc"],
    "url": "https://0hirume.github.io/rojo-schema/latest/project.schema.json"
  },
  {
    "fileMatch": ["*.model.json", "*.model.jsonc"],
    "url": "https://0hirume.github.io/rojo-schema/latest/model.schema.json"
  }
]
```

If VS Code flags a schema URL as untrusted, choose `Trust URI` or `Trust Domain`
from its quick fix.

#### Neovim

Neovim requires version 0.11+ and `vscode-json-language-server`.
Add this to your Lua configuration:

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

#### Helix

Helix requires `vscode-json-language-server`. Add this to your Helix
`languages.toml` or the project's `.helix/languages.toml`:

```toml
[[language-server.vscode-json-language-server.config.json.schemas]]
fileMatch = ["*.project.json", "*.project.jsonc"]
url = "https://0hirume.github.io/rojo-schema/latest/project.schema.json"

[[language-server.vscode-json-language-server.config.json.schemas]]
fileMatch = ["*.model.json", "*.model.jsonc"]
url = "https://0hirume.github.io/rojo-schema/latest/model.schema.json"
```

Schemas update automatically.
