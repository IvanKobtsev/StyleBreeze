# Architecture

StyleBreeze separates editor-neutral analysis from transport and editor integration.

```text
CSS/SCSS -> stylesheet-parser --\
                                  analysis index -> protocol -> LSP server
JS/TS/TSX -> typescript-parser --/                    |
relative imports -> resolver -------------------------+-> WebStorm plugin
```

The analysis crate owns project facts and operations. Parser spans are UTF-8 byte
offsets; the protocol crate alone converts them to LSP UTF-16 positions. Each file
is represented by one replaceable record, so an edit atomically removes its old
exports, references, and diagnostics.

## MVP boundaries

The MVP supports `.module.css`/`.module.scss`, relative imports, default and
namespace bindings, dot and literal bracket access, modern selector functions,
CSS Modules scope, basic Sass nesting, navigation, references, diagnostics, and
safe rename. Sass graphs, interpolation evaluation, naming conventions, ICSS,
completion, unused exports, hover, and CodeLens are intentionally deferred.
 
