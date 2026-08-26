# StyleBreeze

StyleBreeze is a Rust CSS Modules language server with first-class WebStorm integration.
The current navigation MVP understands `.module.css` and `.module.scss`, including
modern selector functions and CSS Modules local/global scope.

## Development

```text
cargo test --workspace
cargo run -p stylebreeze -- --stdio
```

The IntelliJ plugin lives in `editors/intellij`. See `docs/architecture.md` and
`docs/adr/0001-stylesheet-parser.md` for design details.

