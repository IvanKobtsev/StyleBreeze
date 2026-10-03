# StyleBreeze

StyleBreeze is a WebStorm plugin that displays diagnostics and provides precise CSS Module navigation using the same Rust analysis engine as [StyleContract](https://github.com/IvanKobtsev/StyleContract). It supports TypeScript, TSX, CSS, and SCSS files, including unsaved editor changes. Navigate from TypeScript references to their declarations, or from class declarations to their exact TypeScript usages, without unrelated same-named classes from other modules.

## Development

Keep the StyleContract repository next to this repository, then run:

```console
cargo test --workspace --all-targets
cd editors/jetbrains
gradlew.bat runIde
```

The `runIde` task builds the current `style-breeze` Rust executable first, compiles the plugin directly from this checkout, prepares an isolated WebStorm 2026.2 sandbox, and launches that sandbox with the development executable path injected. Release binaries under `dist/` are not required for local development.

On first use, diagnostics are enabled automatically when `style-contract.json` exists at the workspace root. Otherwise they remain disabled. The project-specific setting under **Settings | Tools | StyleBreeze** can enable diagnostics and select either the root configuration or a custom relative/absolute path.

## Packaging

Release packaging follows LocaleBreeze: six native binaries are staged under `dist/jetbrains/<platform>/`, then one cross-platform WebStorm ZIP is produced under `dist/editors/`.
