# SCSS modules

StyleBreeze indexes Sass variables, mixins, functions, and module relationships
across every `.scss` file in the workspace. The same resolved symbol identity is
used for navigation, find usages, usage counts, rename, and completion.

Declarations with no resolved usages are published as unnecessary hint
diagnostics and rendered faded by supporting editors. This applies independently
to Sass variables, mixins, and functions, including declarations reached through
namespaced, star-imported, or forwarded modules.

## Supported module behavior

- Local declarations and references for `$variables`, `@mixin`/`@include`, and
  `@function`/function calls.
- Namespaced and `as *` `@use` directives, including `with (...)` configuration.
- `@forward` chains with prefixes, `show`, and `hide`.
- Sass private members and hyphen/underscore name equivalence.
- Explicit, extensionless, partial (`_name.scss`), and index module resolution.
- Cycle-safe graphs. Dynamic and interpolated module paths are intentionally not
  guessed.

Selecting an unimported completion adds a source-specific import such as:

```scss
@use "src/styles/variables.scss" as *;
```

When multiple modules export the same name, completion presents each source as a
separate choice. Existing namespaced imports are not duplicated with a star
import.

Configure ordered load paths under **Settings → StyleBreeze SCSS**. Relative
load paths are resolved from the project root; `.` is the default. StyleBreeze
does not rewrite existing imports when a document is saved.

## Navigation behavior

- Invoking navigation on a Sass or CSS Module usage opens its declaration.
- Invoking navigation on a declaration opens its usages.
- IntelliJ compares resolved files rather than raw URI strings, so Windows drive
  casing and URI escaping do not affect reverse navigation.
- VS Code resolves the definition first and only switches to usages when the
  cursor is on that exact declaration.

## Diagnostic logging

The IntelliJ plugin launches the server with SCSS debug logging enabled. Server
stderr records workspace indexing, SCSS fact counts, resolved dependency counts,
configuration changes, definition/reference results, and completion counts. The
IntelliJ log additionally records navigation requests,
completion dispatches with connected-client and returned-item counts, protocol
target counts, target mapping, and failures to load a URI, PSI file, or document.

In IntelliJ/WebStorm, use **Help → Show Log in Explorer/Finder** and search for
`StyleBreeze`. In VS Code, open **View → Output → StyleBreeze**; protocol tracing
can also be enabled with `styleBreeze.server.trace`.

For a separately launched server, set `STYLEBREEZE_LOG=debug` to enable the same
stderr diagnostics. Unset it or use `STYLEBREEZE_LOG=off` to disable them.
