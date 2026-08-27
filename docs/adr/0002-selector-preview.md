# ADR 0002: Structured selector previews

- Status: Accepted
- Date: 2026-08-27

## Decision

Resolve supported CSS and SCSS nesting in the stylesheet parser, convert the
resolved selector into an editor-neutral constraint graph, and return that graph
from `stylebreeze/selectorPreview`. WebStorm owns the themed HTML rendering.

The first version supports compounds, attributes, state pseudo-classes, the four
combinators, and relational `:has()`. Unsupported syntax returns a reason without
a partial DOM. General siblings include a clearly labeled illustrative spacer so
their preview cannot be mistaken for adjacent siblings.

## Consequences

The selected element and relational witnesses remain semantic roles rather than
formatting embedded in the server response. Other editors can render the same
data later, while WebStorm can use a theme-aware blue selection highlight.
