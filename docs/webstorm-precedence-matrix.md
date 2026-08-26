# WebStorm precedence verification

Run this matrix against the targeted WebStorm 2026.x release before publishing.

| Operation | TS/TSX origin | SCSS origin | Generated `.d.ts` present | Required result |
|---|---:|---:|---:|---|
| Definition | yes | no | no/yes | Exact SCSS class range |
| References | yes | yes | no/yes | TS and SCSS occurrences |
| Prepare rename | yes | yes | no/yes | Exact class-name range |
| Rename | yes | yes | no/yes | One atomic cross-language edit |

If WebStorm suppresses or redirects any LSP operation, add a native IntelliJ
provider only for that operation and delegate its lookup to the same server.

