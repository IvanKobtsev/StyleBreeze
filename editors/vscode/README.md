# StyleBreeze for VS Code

StyleBreeze provides exact CSS Modules navigation, references, safe rename,
completion, and diagnostics for `.module.css` and `.module.scss` files used from
JavaScript and TypeScript.

## Features

- Navigate from `styles.className` to its stylesheet declaration.
- Find TS/TSX usages from a CSS or SCSS class declaration.
- Rename a class safely across stylesheet and script files.
- Complete statically known CSS Module exports.
- Warn about unknown exports and fade unused classes.
- Resolve relative imports and TypeScript `baseUrl`/`paths` aliases.

The extension runs entirely on your machine. It contains no telemetry and sends
no project data over the network.

## Troubleshooting

Open **View → Output → StyleBreeze** to see language-server output. During local
development, `styleBreeze.server.path` can point to a separately built server.
