use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StyleImport {
    pub binding: String,
    pub specifier: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AccessKind {
    Dot,
    Bracket,
    Dynamic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StyleAccess {
    pub binding: String,
    pub class_name: Option<String>,
    pub span: Span,
    pub kind: AccessKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeScriptFacts {
    pub imports: Vec<StyleImport>,
    pub accesses: Vec<StyleAccess>,
    pub parse_errors: Vec<String>,
}

fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$') || b >= 0x80
}

pub fn parse_typescript(path: &Path, source: &str) -> TypeScriptFacts {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut facts = TypeScriptFacts {
        parse_errors: parsed.errors.into_iter().map(|e| e.to_string()).collect(),
        ..Default::default()
    };
    // Oxc establishes that the buffer is JS/TS and provides recovery diagnostics.
    // This range-preserving scanner extracts only the deliberately tiny CSS Modules grammar.
    scan_imports(source, &mut facts);
    scan_accesses(source, &mut facts);
    facts
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}
fn quoted(source: &str, i: usize) -> Option<(String, usize, Span)> {
    let b = source.as_bytes();
    if i >= b.len() || !matches!(b[i], b'\'' | b'"') {
        return None;
    }
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() && b[j] != q {
        if b[j] == b'\\' {
            j += 1;
        }
        j += 1;
    }
    (j < b.len()).then(|| {
        (
            source[i + 1..j].into(),
            j + 1,
            Span {
                start: i + 1,
                end: j,
            },
        )
    })
}
fn scan_imports(source: &str, facts: &mut TypeScriptFacts) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        if &bytes[i..i + 6] == b"import" && (i == 0 || !ident(bytes[i - 1])) {
            let mut j = skip_ws(bytes, i + 6);
            let binding_start = if j < bytes.len() && bytes[j] == b'*' {
                j = skip_ws(bytes, j + 1);
                if bytes.get(j..j + 2) != Some(b"as") {
                    i += 6;
                    continue;
                }
                j = skip_ws(bytes, j + 2);
                j
            } else {
                j
            };
            while j < bytes.len() && ident(bytes[j]) {
                j += 1;
            }
            if j == binding_start {
                i += 6;
                continue;
            }
            let binding = source[binding_start..j].to_string();
            if let Some(from) = source[j..].find("from") {
                let q = skip_ws(bytes, j + from + 4);
                if let Some((specifier, end, _spec_span)) = quoted(source, q) {
                    if specifier.contains(".module.") {
                        facts.imports.push(StyleImport {
                            binding,
                            specifier,
                            span: Span {
                                start: binding_start,
                                end: j,
                            },
                        });
                    }
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
}
fn scan_accesses(source: &str, facts: &mut TypeScriptFacts) {
    let bytes = source.as_bytes();
    for import in facts.imports.clone() {
        let needle = import.binding.as_bytes();
        let mut i = 0;
        while i + needle.len() < bytes.len() {
            if &bytes[i..i + needle.len()] != needle
                || (i > 0 && ident(bytes[i - 1]))
                || (i + needle.len() < bytes.len() && ident(bytes[i + needle.len()]))
            {
                i += 1;
                continue;
            }
            let j = skip_ws(bytes, i + needle.len());
            if import.span.start == i {
                i = j.saturating_add(1);
                continue;
            }
            if j < bytes.len() && bytes[j] == b'.' {
                let start = j + 1;
                let mut end = start;
                while end < bytes.len() && ident(bytes[end]) {
                    end += 1;
                }
                if end > start {
                    facts.accesses.push(StyleAccess {
                        binding: import.binding.clone(),
                        class_name: Some(source[start..end].into()),
                        span: Span { start, end },
                        kind: AccessKind::Dot,
                    });
                }
            } else if j < bytes.len() && bytes[j] == b'[' {
                let q = skip_ws(bytes, j + 1);
                if let Some((name, _, span)) = quoted(source, q) {
                    facts.accesses.push(StyleAccess {
                        binding: import.binding.clone(),
                        class_name: Some(name),
                        span,
                        kind: AccessKind::Bracket,
                    });
                } else {
                    facts.accesses.push(StyleAccess {
                        binding: import.binding.clone(),
                        class_name: None,
                        span: Span {
                            start: j,
                            end: j + 1,
                        },
                        kind: AccessKind::Dynamic,
                    });
                }
            } else {
                facts.accesses.push(StyleAccess {
                    binding: import.binding.clone(),
                    class_name: None,
                    span: Span {
                        start: i,
                        end: i + needle.len(),
                    },
                    kind: AccessKind::Dynamic,
                });
            }
            i = j.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_and_accesses() {
        let f = parse_typescript(
            Path::new("x.tsx"),
            "import * as s from './x.module.scss'; s.foo; s['bar']; s[name]; consume(s)",
        );
        assert_eq!(f.imports.len(), 1);
        assert_eq!(f.accesses.len(), 4);
    }

    #[test]
    fn unicode_before_import_does_not_split_utf8() {
        let f = parse_typescript(
            Path::new("unicode.ts"),
            "const caption = 'Wait…'; import styles from './x.module.css'; styles.root;",
        );
        assert_eq!(f.imports.len(), 1);
        assert_eq!(f.accesses.len(), 1);
        assert_eq!(f.accesses[0].class_name.as_deref(), Some("root"));
    }
}
