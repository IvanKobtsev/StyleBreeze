# Changelog

## 0.3.0

- Initial VS Code extension with navigation, references, rename, completion,
  unknown-export warnings, and unused-export fading.
- Added project-wide Sass variables, mixins, functions, module graphs,
  auto-import completion, rename, and import-path fixing infrastructure.
- Fixed `.module.scss` navigation so usages open declarations while declarations
  continue to open their usages.
- Fade unused Sass variable, mixin, and function declarations using resolved
  project-wide usages.
