# StyleBreeze

StyleBreeze is a Rust CSS Modules language server with first-class WebStorm and
Visual Studio Code integrations.
The current navigation implementation understands `.module.css` and
`.module.scss`, including modern selector functions and CSS Modules local/global
scope. It also provides project-wide SCSS variable, mixin, function, `@use`, and
`@forward` support; see [the SCSS guide](docs/scss.md).

## Development

```text
cargo test --workspace
cargo run -p stylebreeze -- --stdio
```

The IntelliJ plugin lives in `editors/intellij`. See `docs/architecture.md` and
`docs/adr/0001-stylesheet-parser.md` for design details.

The VS Code extension lives in `editors/vscode`. CI produces one
`stylebreeze-vscode-extensions.zip` artifact containing the Windows, macOS, and
Linux `.vsix` packages for x64 and ARM64. Extract it and install the matching
package with **Extensions: Install from VSIX...** in the VS Code command palette.
