use oxc_allocator::Allocator;
use oxc_ast::ast::{
    CallExpression, ConditionalExpression, JSXAttribute, JSXAttributeName, JSXAttributeValue,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
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
    pub composition: Option<Span>,
    pub composition_root: Option<Span>,
    pub composition_certain: bool,
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
    let mut composition_visitor = ClassNameVisitor {
        source,
        roots: Vec::new(),
        branches: Vec::new(),
    };
    composition_visitor.visit_program(&parsed.program);
    let mut facts = TypeScriptFacts {
        parse_errors: parsed.errors.into_iter().map(|e| e.to_string()).collect(),
        ..Default::default()
    };
    // Oxc establishes that the buffer is JS/TS and provides recovery diagnostics.
    // This range-preserving scanner extracts only the deliberately tiny CSS Modules grammar.
    scan_imports(source, &mut facts);
    scan_accesses(source, &mut facts);
    let roots = composition_visitor.roots;
    let mut compositions = roots.clone();
    compositions.extend(composition_visitor.branches);
    for access in &mut facts.accesses {
        if let Some((span, certain)) = compositions
            .iter()
            .filter(|(span, _)| access.span.start >= span.start && access.span.end <= span.end)
            .min_by_key(|(span, _)| span.end - span.start)
        {
            access.composition = Some(*span);
            access.composition_certain = *certain;
        }
        access.composition_root = roots
            .iter()
            .find(|(span, _)| access.span.start >= span.start && access.span.end <= span.end)
            .map(|(span, _)| *span);
    }
    let dynamic_roots: Vec<_> = facts
        .accesses
        .iter()
        .filter(|access| access.kind == AccessKind::Dynamic)
        .filter_map(|access| access.composition_root)
        .collect();
    for access in &mut facts.accesses {
        if access
            .composition_root
            .is_some_and(|root| dynamic_roots.contains(&root))
        {
            access.composition_certain = false;
        }
    }
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
                        composition: None,
                        composition_root: None,
                        composition_certain: false,
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
                        composition: None,
                        composition_root: None,
                        composition_certain: false,
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
                        composition: None,
                        composition_root: None,
                        composition_certain: false,
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
                    composition: None,
                    composition_root: None,
                    composition_certain: false,
                });
            }
            i = j.saturating_add(1);
        }
    }
}

struct ClassNameVisitor<'s> {
    source: &'s str,
    roots: Vec<(Span, bool)>,
    branches: Vec<(Span, bool)>,
}

impl<'a> Visit<'a> for ClassNameVisitor<'_> {
    fn visit_jsx_attribute(&mut self, attribute: &JSXAttribute<'a>) {
        if matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name == "className")
            && let Some(JSXAttributeValue::ExpressionContainer(container)) = &attribute.value
        {
            let span = container.expression.span();
            if span.end > span.start {
                let span = Span {
                    start: span.start as usize,
                    end: span.end as usize,
                };
                let expression = &self.source[span.start..span.end];
                let certain = !expression.contains("...")
                    && !expression.contains("=>")
                    && !expression.contains("await ");
                self.roots.push((span, certain));
            }
        }
        walk::walk_jsx_attribute(self, attribute);
    }

    fn visit_conditional_expression(&mut self, expression: &ConditionalExpression<'a>) {
        let whole = expression.span();
        if self
            .roots
            .iter()
            .any(|(root, _)| whole.start as usize >= root.start && whole.end as usize <= root.end)
        {
            for branch in [&expression.consequent, &expression.alternate] {
                let span = branch.span();
                self.branches.push((
                    Span {
                        start: span.start as usize,
                        end: span.end as usize,
                    },
                    true,
                ));
            }
        }
        walk::walk_conditional_expression(self, expression);
    }

    fn visit_call_expression(&mut self, expression: &CallExpression<'a>) {
        let span = expression.span();
        if let Some((_, certain)) = self
            .roots
            .iter_mut()
            .find(|(root, _)| span.start as usize >= root.start && span.end as usize <= root.end)
        {
            let callee = expression.callee.span();
            let name = self.source[callee.start as usize..callee.end as usize].trim();
            if !matches!(name, "clsx" | "classNames" | "cn") {
                *certain = false;
            }
        }
        walk::walk_call_expression(self, expression);
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

    #[test]
    fn associates_accesses_with_jsx_class_name_composition() {
        let f = parse_typescript(
            Path::new("x.tsx"),
            "import s from './x.module.scss'; <button className={clsx(s.base, ok && s.active)} />",
        );
        assert_eq!(f.accesses.len(), 2);
        assert!(f.accesses.iter().all(|a| a.composition.is_some()));
        assert!(f.accesses.iter().all(|a| a.composition_certain));
    }

    #[test]
    fn tracks_conditional_branches_and_unsupported_helpers() {
        let conditional = parse_typescript(
            Path::new("x.tsx"),
            "import s from './x.module.scss'; <i className={ok ? clsx(s.base, s.active) : s.base} />",
        );
        let active = conditional
            .accesses
            .iter()
            .find(|a| a.class_name.as_deref() == Some("active"))
            .unwrap();
        let alternate_base = conditional
            .accesses
            .iter()
            .filter(|a| a.class_name.as_deref() == Some("base"))
            .last()
            .unwrap();
        assert_ne!(active.composition, alternate_base.composition);

        let unsupported = parse_typescript(
            Path::new("x.tsx"),
            "import s from './x.module.scss'; <i className={custom(s.active)} />",
        );
        assert!(!unsupported.accesses[0].composition_certain);
    }
}
