# StyleBreeze

StyleBreeze is a Rust CSS Modules language server with first-class WebStorm and
Visual Studio Code integrations.
The current navigation MVP understands `.module.css` and `.module.scss`, including
modern selector functions and CSS Modules local/global scope.

## Development

```text
cargo test --workspace
cargo run -p stylebreeze -- --stdio
```

The IntelliJ plugin lives in `editors/intellij`. See `docs/architecture.md` and
`docs/adr/0001-stylesheet-parser.md` for design details.

The VS Code extension lives in `editors/vscode`. CI produces separate `.vsix`
artifacts for Windows, macOS, and Linux on x64 and ARM64. Install the matching
artifact with **Extensions: Install from VSIX...** in the VS Code command palette.
