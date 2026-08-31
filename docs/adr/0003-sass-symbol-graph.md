# ADR-0003: Resolve Sass through an editor-neutral symbol graph

**Status:** Accepted

**Date:** 2026-08-31

## Context

Sass variables, mixins, and functions are visible through local declarations,
namespaced or star `@use` directives, and filtered or prefixed `@forward` chains.
Editor-only completion logic cannot provide consistent navigation, references,
rename, and import edits across those paths.

## Decision

The stylesheet parser emits exact declaration, reference, and module-directive
spans. The resolver applies ordered Sass load roots and Sass partial/index lookup.
The analysis project assigns symbol identity to the declaration file, kind, and
canonical Sass name, then resolves visibility through a cycle-safe module graph.
LSP completion responses contain the auto-import edits; IntelliJ only presents
and applies them.

## Consequences

- Definition, references, rename, and completion share one resolution model.
- Same-named symbols in unrelated modules remain distinct.
- Other editors can reuse the server without reproducing Sass semantics.
- Dynamic/interpolated module paths remain unresolved rather than guessed.
