# Built-in plugins

Plugins that ship with Basecraft. They use only the public plugin API, the same one third parties get. If a built-in plugin needs a private hook, the API is incomplete and gets fixed instead.

Each plugin is a folder with a `plugin.toml` manifest.

## Settings forms

A plugin can declare the settings the app asks for before a run, as
`[[forms]]` in `plugin.toml`. The app draws one dialog from them; nothing in
the app is specific to the plugin. A form names its `operation`, the run
button, the new file's `output` suffix and extension, `min_rows`, the
`working` message, a `note`, `fixed` parameters sent with every run and
`threads = true` to pass the allocated CPU threads. Each `[[forms.fields]]`
has a `name`, `label`, `type` (`choice`, `integer` or `boolean`) and
`default`, plus as needed:

- `choices`: `[value, label]` pairs; an empty value sends nothing, for the
  tool's own default.
- `min` and `max` for integers, and `min_when = [[condition, minimum]]`.
- `when = "name == value"` (or `!=`): the field applies only then; otherwise
  it is greyed and not sent.
- `value_when = [[condition, value]]`: the field is fixed while a condition
  holds.
- `local = true`: the field shapes the form and is never sent.

The host checks the values against the form before a run. See `fasttree`,
`iqtree` and `distance`.

## Installed operation contracts

See [operation contracts](operation-contract.md) for the contract, the handler
form, installation and the schema profile. Every operation runs through the
shared host in the GUI, CLI, Python and MCP. Forms are presentation metadata;
operations.json supplies scientific defaults, bindings and result schemas.
The installed-operation picker supports compatible flat file-operation forms.
