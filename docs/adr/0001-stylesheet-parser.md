# ADR 0001: Range-preserving normalized stylesheet parser

- Status: Accepted for MVP
- Date: 2026-08-26

## Decision

Expose stylesheet syntax through the internal `StylesheetParser` interface and
use the range-preserving normalized parser for the MVP. It structurally tracks
blocks, parentheses, functional selectors, local/global scope, comments, strings,
and Sass parent selectors.

## Evidence

The corpus in `fixtures/selectors` covers `:where`, `:is`, `:not`, `:has`, selector
lists, local/global arguments and blocks, nesting, `&` suffixes, and unresolved
interpolation. Its snapshot is enforced by a parser test.

Biome remains the preferred vendor parser once its SCSS parsing is stable and it
passes this corpus with exact recovered ranges. Until then, importing its evolving
syntax nodes would add risk without changing the semantic contract.

## Consequences

Analysis depends only on normalized facts. A future Biome or Tree-sitter adapter
can replace the implementation without leaking vendor nodes into project indexes.

