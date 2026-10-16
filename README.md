# StyleBreeze

StyleBreeze is a WebStorm plugin that displays project-wide diagnostics and provides precise CSS Module navigation using the same Rust analysis engine as [StyleContract](https://github.com/IvanKobtsev/StyleContract). Version 0.2.0 supports every StyleContract 0.9.3 diagnostic across TypeScript, TSX, CSS, and SCSS files, including unopened files and unsaved editor changes. Navigate from TypeScript references to their declarations, or from class declarations to their exact TypeScript usages, without unrelated same-named classes from other modules.

## Development

Keep the StyleContract repository next to this repository, then run:

```console
cargo test --workspace --all-targets
cd editors/jetbrains
gradlew.bat runIde
```

The `runIde` task builds the current `style-breeze` Rust executable first, compiles the plugin directly from this checkout, prepares an isolated WebStorm 2026.2 sandbox, and launches that sandbox with the development executable path injected. Release binaries under `dist/` are not required for local development.

On first use, diagnostics are enabled automatically when `style-contract.json` exists at the workspace root. Otherwise they remain disabled. The project-specific setting under **Settings | Tools | StyleBreeze** can enable diagnostics and select either the root configuration or a custom relative/absolute path.

StyleBreeze publishes the complete StyleContract 0.9.3 rule set to WebStorm's Problems view: `missing-symbol`, `unused-class`, `unused-dependent-class`, `unused-export`, `naming-convention-local`, `naming-convention-global`, `dynamic-reference`, `empty-rule`, `class-leak`, and `leaked-class-usage`. Rule severities and targeted suppressions come directly from the selected StyleContract configuration.

Every StyleContract finding includes a quick fix to suppress that rule for the affected line. StyleBreeze inserts line comments in TypeScript, TSX, and SCSS files, and block comments in CSS files.

## Packaging

Release packaging follows LocaleBreeze: six native binaries are staged under `dist/jetbrains/<platform>/`, then one cross-platform WebStorm ZIP is produced under `dist/editors/`.
