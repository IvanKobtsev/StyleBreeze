use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Scope {
    Local,
    Global,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClassOccurrence {
    pub name: String,
    pub span: Span,
    pub scope: Scope,
    pub selector: Span,
    pub declaration: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParseDiagnostic {
    pub message: String,
    pub span: Span,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StylesheetFacts {
    pub classes: Vec<ClassOccurrence>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

pub trait StylesheetParser: Send + Sync {
    fn parse(&self, source: &str) -> StylesheetFacts;
}

/// Range-preserving CSS/SCSS selector parser used by the MVP. Its lexer is
/// deliberately independent of vendor ASTs so Biome can be introduced behind
/// `StylesheetParser` after its SCSS corpus reaches parity.
#[derive(Default)]
pub struct NormalizedParser;

impl StylesheetParser for NormalizedParser {
    fn parse(&self, source: &str) -> StylesheetFacts {
        parse_stylesheet(source)
    }
}

fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || b >= 0x80
}

fn skip_string_or_comment(bytes: &[u8], i: &mut usize) -> bool {
    if *i + 1 < bytes.len() && bytes[*i] == b'/' && bytes[*i + 1] == b'*' {
        *i += 2;
        while *i + 1 < bytes.len() && !(bytes[*i] == b'*' && bytes[*i + 1] == b'/') {
            *i += 1;
        }
        *i = (*i + 2).min(bytes.len());
        return true;
    }
    if matches!(bytes[*i], b'\'' | b'"') {
        let quote = bytes[*i];
        *i += 1;
        while *i < bytes.len() {
            if bytes[*i] == b'\\' {
                *i += 2;
            } else if bytes[*i] == quote {
                *i += 1;
                break;
            } else {
                *i += 1;
            }
        }
        return true;
    }
    false
}

fn selector_classes(
    source: &str,
    span: Span,
    inherited: Scope,
) -> (Vec<ClassOccurrence>, Vec<ParseDiagnostic>) {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    let mut scopes = vec![inherited];
    let mut parens: Vec<Option<Scope>> = Vec::new();
    let mut i = span.start;
    while i < span.end {
        if skip_string_or_comment(bytes, &mut i) {
            continue;
        }
        if bytes[i] == b':' {
            let name_start = i + 1;
            let mut j = name_start;
            while j < span.end && ident_byte(bytes[j]) {
                j += 1;
            }
            let name = &source[name_start..j];
            while j < span.end && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < span.end && bytes[j] == b'(' {
                let changed = match name {
                    "global" => Some(Scope::Global),
                    "local" => Some(Scope::Local),
                    _ => None,
                };
                parens.push(changed);
                if let Some(scope) = changed {
                    scopes.push(scope);
                }
                i = j + 1;
                continue;
            }
        }
        if bytes[i] == b')' {
            if let Some(changed) = parens.pop().flatten()
                && scopes.last() == Some(&changed)
            {
                scopes.pop();
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'(' {
            parens.push(None);
            i += 1;
            continue;
        }
        if bytes[i] == b'.' && i + 1 < span.end {
            let start = i + 1;
            if bytes[start] == b'#'
                || (bytes[start] == b'\\' && start + 1 < span.end && bytes[start + 1] == b'#')
            {
                diagnostics.push(ParseDiagnostic {
                    message: "Sass-interpolated class names cannot be resolved statically".into(),
                    span: Span {
                        start: i,
                        end: (i + 2).min(span.end),
                    },
                });
                i += 2;
                continue;
            }
            let mut end = start;
            while end < span.end && ident_byte(bytes[end]) {
                end += 1;
            }
            if end > start {
                out.push(ClassOccurrence {
                    name: source[start..end].into(),
                    span: Span { start, end },
                    scope: *scopes.last().unwrap(),
                    selector: span,
                    declaration: false,
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    (out, diagnostics)
}

pub fn parse_stylesheet(source: &str) -> StylesheetFacts {
    let bytes = source.as_bytes();
    let mut facts = StylesheetFacts::default();
    let mut statement_start = 0usize;
    let mut i = 0usize;
    let mut blocks: Vec<(Scope, Vec<String>)> = Vec::new();
    while i < bytes.len() {
        if skip_string_or_comment(bytes, &mut i) {
            continue;
        }
        match bytes[i] {
            b'{' => {
                let raw = &source[statement_start..i];
                let trimmed = raw.trim();
                let trim_offset = raw.len() - raw.trim_start().len();
                let sel_start = statement_start + trim_offset;
                let selector = Span {
                    start: sel_start,
                    end: i,
                };
                let parent_scope = blocks.last().map_or(Scope::Local, |b| b.0);
                let block_scope = if trimmed == ":global" {
                    Scope::Global
                } else if trimmed == ":local" {
                    Scope::Local
                } else {
                    parent_scope
                };
                let (mut classes, diagnostics) = selector_classes(source, selector, parent_scope);
                facts.diagnostics.extend(diagnostics);
                let parent_names = blocks.last().map(|b| b.1.clone()).unwrap_or_default();
                if trimmed.contains('&') {
                    for parent in &parent_names {
                        for suffix in amp_suffixes(trimmed) {
                            let name = format!("{parent}{suffix}");
                            if !classes.iter().any(|c| c.name == name) {
                                let p = source[selector.start..selector.end].find('&').unwrap()
                                    + selector.start;
                                classes.push(ClassOccurrence {
                                    name,
                                    span: Span {
                                        start: p,
                                        end: p + 1,
                                    },
                                    scope: parent_scope,
                                    selector,
                                    declaration: false,
                                });
                            }
                        }
                    }
                }
                let names = classes
                    .iter()
                    .filter(|c| c.scope == Scope::Local)
                    .map(|c| c.name.clone())
                    .collect();
                for class in &mut classes {
                    class.declaration = class.scope == Scope::Local;
                }
                facts.classes.extend(classes);
                blocks.push((block_scope, names));
                statement_start = i + 1;
            }
            b'}' => {
                blocks.pop();
                statement_start = i + 1;
            }
            b';' => {
                statement_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    facts
}

fn amp_suffixes(selector: &str) -> Vec<&str> {
    let bytes = selector.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && ident_byte(bytes[end]) {
                end += 1;
            }
            if end > start {
                out.push(&selector[start..end]);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn functional_scope() {
        let f = parse_stylesheet(":where(.mine, :global(.external), :is(.other)) {}");
        let local: Vec<_> = f
            .classes
            .iter()
            .filter(|c| c.scope == Scope::Local)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(local, ["mine", "other"]);
    }
    #[test]
    fn nesting_suffix() {
        let f = parse_stylesheet(".parent { &__child {} &:where(.active) {} }");
        assert!(f.classes.iter().any(|c| c.name == "parent__child"));
        assert!(f.classes.iter().any(|c| c.name == "active"));
    }
    #[test]
    fn fixture_corpus_matches_expected_scope() {
        let source = include_str!("../../../fixtures/selectors/modern.module.scss");
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/selectors/modern.expected.json"
        ))
        .unwrap();
        let facts = parse_stylesheet(source);
        let locals: std::collections::HashSet<_> = facts
            .classes
            .iter()
            .filter(|c| c.scope == Scope::Local)
            .map(|c| c.name.as_str())
            .collect();
        let globals: std::collections::HashSet<_> = facts
            .classes
            .iter()
            .filter(|c| c.scope == Scope::Global)
            .map(|c| c.name.as_str())
            .collect();
        for name in expected["localExports"].as_array().unwrap() {
            assert!(
                locals.contains(name.as_str().unwrap()),
                "missing local {name}"
            );
        }
        for name in expected["globalClasses"].as_array().unwrap() {
            assert!(
                globals.contains(name.as_str().unwrap()),
                "missing global {name}"
            );
        }
        assert_eq!(facts.diagnostics.len(), 1);
    }
}
