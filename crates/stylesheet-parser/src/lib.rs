use serde::{Deserialize, Serialize};

mod selector_preview;
pub use selector_preview::{
    NodeRole, PreviewAttribute, PreviewNode, Relationship, RelationshipKind, SelectorPreview,
    SelectorRule, StateRequirement, UnsupportedReason, preview_selector,
};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CustomPropertyDeclaration {
    pub name: String,
    pub span: Span,
    pub selector: Option<String>,
    pub registered: bool,
    pub syntax: Option<String>,
    pub inherits: Option<bool>,
    pub initial_value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CustomPropertyReference {
    pub name: String,
    pub span: Span,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PropertyImport {
    pub path: String,
    pub path_span: Span,
    pub names: Vec<(String, Span)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PropertyAnnotations {
    pub imports: Vec<PropertyImport>,
    pub exports: Vec<(String, Span)>,
    pub suppress_next_lines: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SassSymbolKind {
    Variable,
    Mixin,
    Function,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SassDeclaration {
    pub name: String,
    pub span: Span,
    pub kind: SassSymbolKind,
    pub private: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SassReference {
    pub name: String,
    pub span: Span,
    pub kind: SassSymbolKind,
    pub namespace: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SassDirectiveKind {
    Use,
    Forward,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SassDirective {
    pub kind: SassDirectiveKind,
    pub path: String,
    pub path_span: Span,
    pub statement_span: Span,
    pub namespace: Option<String>,
    pub star: bool,
    pub prefix: Option<String>,
    pub show: Vec<String>,
    pub hide: Vec<String>,
    pub member_spans: Vec<(String, Span)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModifierRule {
    pub modifier: String,
    pub required_all: Vec<String>,
    pub modifier_span: Span,
    pub base_spans: Vec<Span>,
    pub selector: Span,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StylesheetFacts {
    pub classes: Vec<ClassOccurrence>,
    pub modifier_rules: Vec<ModifierRule>,
    pub independent_classes: Vec<String>,
    pub diagnostics: Vec<ParseDiagnostic>,
    pub selectors: Vec<SelectorRule>,
    pub custom_property_declarations: Vec<CustomPropertyDeclaration>,
    pub custom_property_references: Vec<CustomPropertyReference>,
    pub property_annotations: PropertyAnnotations,
    pub sass_declarations: Vec<SassDeclaration>,
    pub sass_references: Vec<SassReference>,
    pub sass_directives: Vec<SassDirective>,
}

struct BlockContext {
    scope: Scope,
    branches: Vec<Vec<(String, Span)>>,
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
    if skip_comment(bytes, i) {
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

fn skip_comment(bytes: &[u8], i: &mut usize) -> bool {
    if *i + 1 >= bytes.len() || bytes[*i] != b'/' {
        return false;
    }
    if bytes[*i + 1] == b'*' {
        *i += 2;
        while *i + 1 < bytes.len() && !(bytes[*i] == b'*' && bytes[*i + 1] == b'/') {
            *i += 1;
        }
        *i = (*i + 2).min(bytes.len());
        return true;
    }
    let starts_sass_comment = bytes[*i + 1] == b'/'
        && (*i == 0
            || bytes[*i - 1].is_ascii_whitespace()
            || matches!(bytes[*i - 1], b'{' | b'}' | b';'));
    if starts_sass_comment {
        *i += 2;
        while *i < bytes.len() && !matches!(bytes[*i], b'\r' | b'\n') {
            *i += 1;
        }
        return true;
    }
    false
}

fn selector_span(source: &str, start: usize, end: usize) -> Span {
    let bytes = source.as_bytes();
    let mut cursor = start;
    loop {
        while cursor < end && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let before = cursor;
        if cursor < end && skip_comment(bytes, &mut cursor) {
            continue;
        }
        if cursor == before {
            break;
        }
    }
    Span { start: cursor, end }
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
    let mut blocks: Vec<BlockContext> = Vec::new();
    let mut direct_compounds: Vec<(Span, Vec<(String, Span)>)> = Vec::new();
    while i < bytes.len() {
        if skip_string_or_comment(bytes, &mut i) {
            continue;
        }
        match bytes[i] {
            b'{' => {
                let selector = selector_span(source, statement_start, i);
                let trimmed = source[selector.start..selector.end].trim_end();
                let parent_scope = blocks.last().map_or(Scope::Local, |block| block.scope);
                let block_scope = if trimmed == ":global" {
                    Scope::Global
                } else if trimmed == ":local" {
                    Scope::Local
                } else {
                    parent_scope
                };
                let (mut classes, diagnostics) = selector_classes(source, selector, parent_scope);
                facts.diagnostics.extend(diagnostics);
                let parent_branches = blocks
                    .last()
                    .map(|block| block.branches.clone())
                    .unwrap_or_default();
                if trimmed.contains('&') {
                    for parent_branch in &parent_branches {
                        for (parent, _) in parent_branch {
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
                }
                let mut child_branches = Vec::new();
                if let Some(modifier) = simple_nested_modifier(trimmed, &classes) {
                    for parent_branch in &parent_branches {
                        let required_all: Vec<_> = parent_branch
                            .iter()
                            .filter(|(base, _)| base != &modifier.name)
                            .map(|(base, _)| base.clone())
                            .collect();
                        if !required_all.is_empty() {
                            facts.modifier_rules.push(ModifierRule {
                                modifier: modifier.name.clone(),
                                required_all,
                                modifier_span: modifier.span,
                                base_spans: parent_branch.iter().map(|(_, span)| *span).collect(),
                                selector,
                            });
                        }
                        let mut child = parent_branch.clone();
                        child.push((modifier.name.clone(), modifier.span));
                        child_branches.push(child);
                    }
                } else if !trimmed.contains('&')
                    && let Some(branches) = simple_compound_branches(source, selector, &classes)
                {
                    for branch in branches {
                        if branch.len() == 1 {
                            facts.independent_classes.push(branch[0].0.clone());
                            child_branches.push(branch);
                        } else if branch.len() == 2 {
                            direct_compounds.push((selector, branch));
                        }
                    }
                }
                for class in &mut classes {
                    class.declaration = class.scope == Scope::Local;
                }
                facts.classes.extend(classes);
                blocks.push(BlockContext {
                    scope: block_scope,
                    branches: child_branches,
                });
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
    facts.independent_classes.sort();
    facts.independent_classes.dedup();
    for (selector, classes) in direct_compounds {
        let first_independent = facts.independent_classes.contains(&classes[0].0);
        let second_independent = facts.independent_classes.contains(&classes[1].0);
        let (base, modifier) = match (first_independent, second_independent) {
            (true, _) => (&classes[0], &classes[1]),
            (false, true) => (&classes[1], &classes[0]),
            _ => continue,
        };
        facts.modifier_rules.push(ModifierRule {
            modifier: modifier.0.clone(),
            required_all: vec![base.0.clone()],
            modifier_span: modifier.1,
            base_spans: vec![base.1],
            selector,
        });
    }
    facts.selectors = selector_preview::collect_selector_rules(source);
    collect_custom_property_facts(source, &mut facts);
    collect_sass_facts(source, &mut facts);
    facts
}

fn collect_sass_facts(source: &str, facts: &mut StylesheetFacts) {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && matches!(bytes[i + 1], b'/' | b'*') {
            let start = i;
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                    i += 1;
                }
            } else {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            for b in &mut masked[start..i] {
                if !matches!(*b, b'\r' | b'\n') {
                    *b = b' ';
                }
            }
            continue;
        }
        if matches!(bytes[i], b'\'' | b'"') {
            let start = i;
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            for b in &mut masked[start..i] {
                if !matches!(*b, b'\r' | b'\n') {
                    *b = b' ';
                }
            }
            continue;
        }
        i += 1;
    }

    for (keyword, kind) in [
        ("@use", SassDirectiveKind::Use),
        ("@forward", SassDirectiveKind::Forward),
    ] {
        let mut cursor = 0;
        while let Some(rel) = find_token(&masked, cursor, keyword.as_bytes()) {
            let at = rel;
            let mut p = at + keyword.len();
            while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            if p >= bytes.len() || !matches!(bytes[p], b'\'' | b'"') {
                cursor = p.max(at + 1);
                continue;
            }
            let quote = bytes[p];
            let path_start = p + 1;
            p += 1;
            while p < bytes.len() && bytes[p] != quote {
                if bytes[p] == b'\\' {
                    p = (p + 2).min(bytes.len());
                } else {
                    p += 1;
                }
            }
            if p >= bytes.len() {
                break;
            }
            let path_end = p;
            let mut end = p + 1;
            let mut depth = 0usize;
            while end < bytes.len() {
                match bytes[end] {
                    b'(' => depth += 1,
                    b')' => depth = depth.saturating_sub(1),
                    b';' if depth == 0 => {
                        end += 1;
                        break;
                    }
                    _ => {}
                }
                end += 1;
            }
            let tail = &source[p + 1..end];
            let words: Vec<&str> = tail.trim_end_matches(';').split_whitespace().collect();
            let mut namespace = None;
            let mut star = false;
            let mut prefix = None;
            if kind == SassDirectiveKind::Use {
                if let Some(pos) = words.iter().position(|w| *w == "as") {
                    if let Some(value) = words.get(pos + 1) {
                        if *value == "*" {
                            star = true
                        } else {
                            namespace = Some(trim_sass_name(value).into());
                        }
                    }
                } else {
                    namespace = default_namespace(&source[path_start..path_end]);
                }
            } else if let Some(pos) = words.iter().position(|w| *w == "as") {
                prefix = words
                    .get(pos + 1)
                    .map(|v| trim_sass_name(v).trim_end_matches('*').to_string());
            }
            let show = directive_names(&words, "show");
            let hide = directive_names(&words, "hide");
            let member_spans = directive_member_spans(source, p + 1, end);
            facts.sass_directives.push(SassDirective {
                kind,
                path: source[path_start..path_end].into(),
                path_span: Span {
                    start: path_start,
                    end: path_end,
                },
                statement_span: Span { start: at, end },
                namespace,
                star,
                prefix,
                show,
                hide,
                member_spans,
            });
            cursor = end.max(at + 1);
        }
    }

    let mut declaration_spans = Vec::new();
    for (keyword, kind, sigil) in [
        ("@mixin", SassSymbolKind::Mixin, false),
        ("@function", SassSymbolKind::Function, false),
    ] {
        let mut cursor = 0;
        while let Some(at) = find_token(&masked, cursor, keyword.as_bytes()) {
            let mut p = at + keyword.len();
            while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            let start = p;
            while p < bytes.len() && sass_ident(bytes[p]) {
                p += 1;
            }
            if p > start {
                push_sass_declaration(source, facts, kind, start, p);
                declaration_spans.push(Span { start, end: p });
            }
            cursor = p.max(at + 1);
            let _ = sigil;
        }
    }
    i = 0;
    while i < bytes.len() {
        if masked[i] == b'$' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && sass_ident(bytes[end]) {
                end += 1;
            }
            if end > start {
                let mut p = end;
                while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                    p += 1;
                }
                let before = source[..i].trim_end().as_bytes().last().copied();
                if p < bytes.len()
                    && bytes[p] == b':'
                    && before.is_none_or(|b| matches!(b, b';' | b'{' | b'}'))
                    && !facts
                        .sass_directives
                        .iter()
                        .any(|d| d.statement_span.start <= i && i < d.statement_span.end)
                {
                    push_sass_declaration(source, facts, SassSymbolKind::Variable, start, end);
                    declaration_spans.push(Span { start, end });
                }
            }
            i = end.max(i + 1);
            continue;
        }
        i += 1;
    }

    i = 0;
    while i < bytes.len() {
        if masked[i] == b'$' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && sass_ident(bytes[end]) {
                end += 1;
            }
            if end > start && !declaration_spans.iter().any(|s| s.start == start) {
                let (namespace, name_start) = namespace_before(source, i);
                facts.sass_references.push(SassReference {
                    name: source[start..end].into(),
                    span: Span { start, end },
                    kind: SassSymbolKind::Variable,
                    namespace: namespace
                        .or_else(|| (name_start < i).then(|| source[name_start..i - 1].into())),
                });
            }
            i = end.max(i + 1);
            continue;
        }
        if bytes.get(i..i + 8) == Some(b"@include") && token_boundary(bytes, i, i + 8) {
            let mut p = i + 8;
            while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            let (namespace, start) = parse_qualified_name(source, p);
            let mut end = start;
            while end < bytes.len() && sass_ident(bytes[end]) {
                end += 1;
            }
            if end > start {
                facts.sass_references.push(SassReference {
                    name: source[start..end].into(),
                    span: Span { start, end },
                    kind: SassSymbolKind::Mixin,
                    namespace,
                });
            }
            i = end.max(i + 1);
            continue;
        }
        if (masked[i].is_ascii_alphabetic() || masked[i] == b'_')
            && (i == 0 || !sass_ident(masked[i - 1]))
        {
            let (namespace, start) = parse_qualified_name(source, i);
            let mut end = start;
            while end < bytes.len() && sass_ident(bytes[end]) {
                end += 1;
            }
            let mut p = end;
            while p < bytes.len() && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            if end > start
                && p < bytes.len()
                && bytes[p] == b'('
                && !declaration_spans.iter().any(|span| span.start == start)
                && !source[..i].ends_with("@function ")
                && !source[..i].ends_with("@mixin ")
            {
                facts.sass_references.push(SassReference {
                    name: source[start..end].into(),
                    span: Span { start, end },
                    kind: SassSymbolKind::Function,
                    namespace,
                });
            }
            i = end.max(i + 1);
            continue;
        }
        i += 1;
    }
}

fn find_token(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    (from..=bytes.len().saturating_sub(needle.len())).find(|&i| {
        bytes.get(i..i + needle.len()) == Some(needle) && token_boundary(bytes, i, i + needle.len())
    })
}
fn token_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    (start == 0 || !sass_ident(bytes[start - 1])) && (end == bytes.len() || !sass_ident(bytes[end]))
}
fn sass_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || b >= 0x80
}
fn trim_sass_name(value: &str) -> &str {
    value.trim_matches(|c: char| c == ',' || c == ';')
}
fn default_namespace(path: &str) -> Option<String> {
    path.rsplit('/')
        .next()
        .map(|s| {
            s.trim_end_matches(".scss")
                .trim_start_matches('_')
                .to_string()
        })
        .filter(|s| !s.is_empty())
}
fn directive_names(words: &[&str], key: &str) -> Vec<String> {
    words
        .iter()
        .position(|w| *w == key)
        .map(|p| {
            words[p + 1..]
                .iter()
                .take_while(|w| !matches!(**w, "with" | "as" | "show" | "hide"))
                .map(|w| trim_sass_name(w).trim_start_matches('$').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}
fn directive_member_spans(source: &str, start: usize, end: usize) -> Vec<(String, Span)> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = start;
    let mut collecting = false;
    while i < end {
        while i < end && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i < end && bytes[i] == b'$' {
            let name_start = i + 1;
            i = name_start;
            while i < end && sass_ident(bytes[i]) {
                i += 1;
            }
            let mut after = i;
            while after < end && bytes[after].is_ascii_whitespace() {
                after += 1;
            }
            if i > name_start && after < end && bytes[after] == b':' {
                out.push((
                    source[name_start..i].into(),
                    Span {
                        start: name_start,
                        end: i,
                    },
                ));
            }
            continue;
        }
        let word_start = i;
        let name_start = i;
        while i < end && sass_ident(bytes[i]) {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let word = &source[name_start..i];
        if matches!(word, "show" | "hide") {
            collecting = true;
            continue;
        }
        if matches!(word, "as" | "with") {
            collecting = false;
            continue;
        }
        if collecting {
            out.push((
                word.into(),
                Span {
                    start: name_start,
                    end: i,
                },
            ));
        }
        if i == word_start {
            i += 1;
        }
    }
    out
}
fn push_sass_declaration(
    source: &str,
    facts: &mut StylesheetFacts,
    kind: SassSymbolKind,
    start: usize,
    end: usize,
) {
    let name = source[start..end].to_string();
    facts.sass_declarations.push(SassDeclaration {
        private: name.starts_with(['-', '_']),
        name,
        span: Span { start, end },
        kind,
    });
}
fn parse_qualified_name(source: &str, start: usize) -> (Option<String>, usize) {
    let bytes = source.as_bytes();
    let mut p = start;
    while p < bytes.len() && sass_ident(bytes[p]) {
        p += 1;
    }
    if p < bytes.len() && bytes[p] == b'.' {
        (Some(source[start..p].into()), p + 1)
    } else {
        (None, start)
    }
}
fn namespace_before(source: &str, dollar: usize) -> (Option<String>, usize) {
    if dollar == 0 || source.as_bytes()[dollar - 1] != b'.' {
        return (None, dollar);
    }
    let mut start = dollar - 1;
    while start > 0 && sass_ident(source.as_bytes()[start - 1]) {
        start -= 1;
    }
    (Some(source[start..dollar - 1].into()), start)
}

fn collect_custom_property_facts(source: &str, facts: &mut StylesheetFacts) {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut statement_start = 0;
    let mut block_selectors: Vec<String> = Vec::new();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            let end = (i + 2).min(bytes.len());
            parse_property_annotation(source, start, end, facts);
            i = end;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                i += 1;
            }
            continue;
        }
        if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if bytes[i] == b'{' {
            let head = source[statement_start..i].trim().to_string();
            block_selectors.push(head);
            statement_start = i + 1;
            i += 1;
            continue;
        }
        if bytes[i] == b'}' {
            block_selectors.pop();
            statement_start = i + 1;
            i += 1;
            continue;
        }
        if bytes[i] == b';' {
            statement_start = i + 1;
            i += 1;
            continue;
        }
        if i + 2 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            let start = i;
            i += 2;
            while i < bytes.len() && ident_byte(bytes[i]) {
                i += 1;
            }
            let end = i;
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if end > start + 2 && j < bytes.len() && bytes[j] == b':' {
                let registered = block_selectors
                    .last()
                    .is_some_and(|s| s.trim_start().starts_with("@property "));
                facts
                    .custom_property_declarations
                    .push(CustomPropertyDeclaration {
                        name: source[start..end].into(),
                        span: Span { start, end },
                        selector: block_selectors.last().cloned(),
                        registered,
                        syntax: None,
                        inherits: None,
                        initial_value: None,
                    });
            }
            continue;
        }
        if bytes.get(i..i + 4) == Some(b"var(") {
            let mut j = i + 4;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j + 2 <= bytes.len() && &source[j..j + 2] == "--" {
                let start = j;
                j += 2;
                while j < bytes.len() && ident_byte(bytes[j]) {
                    j += 1;
                }
                if j > start + 2 {
                    facts
                        .custom_property_references
                        .push(CustomPropertyReference {
                            name: source[start..j].into(),
                            span: Span { start, end: j },
                            line: source[..start].bytes().filter(|b| *b == b'\n').count(),
                        });
                }
            }
        }
        i += 1;
    }
    // Turn @property block headers into registrations and attach simple descriptors.
    for selector in block_selectors {
        let _ = selector;
    }
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find("@property") {
        let at = cursor + relative;
        let mut p = at + "@property".len();
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p + 2 <= bytes.len() && &source[p..p + 2] == "--" {
            let start = p;
            p += 2;
            while p < bytes.len() && ident_byte(bytes[p]) {
                p += 1;
            }
            if let Some(open_rel) = source[p..].find('{') {
                let open = p + open_rel;
                if let Some(close_rel) = source[open + 1..].find('}') {
                    let body = &source[open + 1..open + 1 + close_rel];
                    facts
                        .custom_property_declarations
                        .push(CustomPropertyDeclaration {
                            name: source[start..p].into(),
                            span: Span { start, end: p },
                            selector: None,
                            registered: true,
                            syntax: descriptor(body, "syntax"),
                            inherits: descriptor(body, "inherits").and_then(|v| v.parse().ok()),
                            initial_value: descriptor(body, "initial-value"),
                        });
                }
            }
        }
        cursor = p.max(at + 1);
    }
}

fn descriptor(body: &str, name: &str) -> Option<String> {
    body.split(';').find_map(|part| {
        let (key, value) = part.split_once(':')?;
        (key.trim() == name).then(|| value.trim().trim_matches(['"', '\'']).to_string())
    })
}

fn parse_property_annotation(source: &str, start: usize, end: usize, facts: &mut StylesheetFacts) {
    let text = &source[start..end];
    if let Some(pos) = text.find("@suppress-unresolved-prop") {
        let line = source[..start + pos]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        facts
            .property_annotations
            .suppress_next_lines
            .push(line + 1);
    }
    if let Some(pos) = text.find("@export-props")
        && let Some((_, list)) = text[pos..].split_once(':')
    {
        collect_annotation_names(
            source,
            start + pos + text[pos..].find(':').unwrap() + 1,
            list,
            &mut facts.property_annotations.exports,
        );
    }
    if let Some(pos) = text.find("@import-props") {
        let tail = &text[pos + "@import-props".len()..];
        let Some(q) = tail.find(['"', '\'']) else {
            return;
        };
        let quote = tail.as_bytes()[q];
        let Some(q2) = tail[q + 1..].bytes().position(|b| b == quote) else {
            return;
        };
        let path_start = start + pos + "@import-props".len() + q + 1;
        let path_end = path_start + q2;
        let after = &source[path_end + 1..end];
        let Some(colon) = after.find(':') else {
            return;
        };
        let list_start = path_end + 1 + colon + 1;
        let mut names = Vec::new();
        collect_annotation_names(source, list_start, &source[list_start..end], &mut names);
        facts.property_annotations.imports.push(PropertyImport {
            path: source[path_start..path_end].into(),
            path_span: Span {
                start: path_start,
                end: path_end,
            },
            names,
        });
    }
}

fn collect_annotation_names(source: &str, base: usize, text: &str, out: &mut Vec<(String, Span)>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 <= bytes.len() {
        if bytes[i] == b'-' && bytes[i + 1] == b'-' {
            let start = i;
            i += 2;
            while i < bytes.len() && ident_byte(bytes[i]) {
                i += 1;
            }
            out.push((
                source[base + start..base + i].into(),
                Span {
                    start: base + start,
                    end: base + i,
                },
            ));
        } else {
            i += 1;
        }
    }
}

fn simple_nested_modifier<'a>(
    selector: &str,
    classes: &'a [ClassOccurrence],
) -> Option<&'a ClassOccurrence> {
    let trimmed = selector.trim();
    if !trimmed.starts_with('&')
        || trimmed.matches('&').count() != 1
        || trimmed.contains(',')
        || contains_deferred_selector_syntax(trimmed)
    {
        return None;
    }
    let mut locals = classes.iter().filter(|c| c.scope == Scope::Local);
    let modifier = locals.next()?;
    locals.next().is_none().then_some(modifier)
}

fn simple_compound_branches(
    source: &str,
    selector: Span,
    classes: &[ClassOccurrence],
) -> Option<Vec<Vec<(String, Span)>>> {
    let text = &source[selector.start..selector.end];
    if contains_deferred_selector_syntax(text) {
        return None;
    }
    let mut out = Vec::new();
    let mut start = selector.start;
    for part in text.split(',') {
        let end = start + part.len();
        let trimmed = part.trim();
        if trimmed.contains('&') || has_top_level_combinator(trimmed) {
            return None;
        }
        let branch: Vec<_> = classes
            .iter()
            .filter(|c| c.scope == Scope::Local && c.span.start >= start && c.span.end <= end)
            .map(|c| (c.name.clone(), c.span))
            .collect();
        if branch.is_empty() || branch.len() > 2 {
            return None;
        }
        out.push(branch);
        start = end + 1;
    }
    Some(out)
}

fn contains_deferred_selector_syntax(selector: &str) -> bool {
    selector.contains(":is(")
        || selector.contains(":where(")
        || selector.contains(":not(")
        || selector.contains(":has(")
        || selector.contains(":global")
        || selector.contains(":local")
        || selector.contains("#{")
}

fn has_top_level_combinator(selector: &str) -> bool {
    let bytes = selector.as_bytes();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'>' | b'+' | b'~' if bracket_depth == 0 && paren_depth == 0 => return true,
            b if b.is_ascii_whitespace() && bracket_depth == 0 && paren_depth == 0 => {
                let before = selector[..i].trim_end();
                let after = selector[i..].trim_start();
                if !before.is_empty() && !after.is_empty() {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
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
    fn extracts_custom_properties_and_short_annotations() {
        let source = r#"/* @import-props "./theme.scss": --brand */
/* @export-props: --runtime */
:root { --brand: red; }
@property --progress { syntax: "<number>"; inherits: false; initial-value: 0; }
/* @suppress-unresolved-prop */
.card { color: var(--missing, var(--brand)); }"#;
        let facts = parse_stylesheet(source);
        assert!(
            facts
                .custom_property_declarations
                .iter()
                .any(|d| d.name == "--brand")
        );
        let registration = facts
            .custom_property_declarations
            .iter()
            .find(|d| d.name == "--progress" && d.registered)
            .unwrap();
        assert_eq!(registration.syntax.as_deref(), Some("<number>"));
        assert_eq!(
            facts
                .custom_property_references
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            vec!["--missing", "--brand"]
        );
        assert_eq!(facts.property_annotations.imports[0].names[0].0, "--brand");
        assert_eq!(facts.property_annotations.exports[0].0, "--runtime");
    }
    #[test]
    fn custom_property_scanner_is_safe_for_bom_and_unicode() {
        let source = "\u{feff}.café { --цвет: red; color: var(--цвет); content: \"😀\"; }";
        let facts = parse_stylesheet(source);
        assert!(
            facts
                .custom_property_declarations
                .iter()
                .any(|d| d.name == "--цвет")
        );
        assert!(
            facts
                .custom_property_references
                .iter()
                .any(|r| r.name == "--цвет")
        );
    }
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
    fn infers_pair_aware_nested_modifiers() {
        let f = parse_stylesheet(
            ".first { &.active {} } .second { &.active {} } .active { color: red; }",
        );
        let pairs: Vec<_> = f
            .modifier_rules
            .iter()
            .map(|r| (r.modifier.as_str(), r.required_all[0].as_str()))
            .collect();
        assert_eq!(pairs, [("active", "first"), ("active", "second")]);
        assert!(f.independent_classes.contains(&"active".into()));
    }

    #[test]
    fn selector_list_creates_alternative_modifier_rules() {
        let f = parse_stylesheet(".first, .second { &.active:hover {} }");
        assert_eq!(f.modifier_rules.len(), 2);
        assert_eq!(f.modifier_rules[0].required_all, ["first"]);
        assert_eq!(f.modifier_rules[1].required_all, ["second"]);
    }

    #[test]
    fn nested_modifiers_inherit_the_complete_base_chain() {
        let source = ".gradientWrapper { &.offset { &.narrow {} } }";
        let f = parse_stylesheet(source);
        let narrow = f
            .modifier_rules
            .iter()
            .find(|rule| rule.modifier == "narrow")
            .unwrap();
        assert_eq!(narrow.required_all, ["gradientWrapper", "offset"]);
        assert_eq!(
            &source[narrow.modifier_span.start..narrow.modifier_span.end],
            "narrow"
        );
        assert_eq!(narrow.base_spans.len(), 2);
    }

    #[test]
    fn nested_selector_list_preserves_requirement_alternatives() {
        let f = parse_stylesheet(".first, .second { &.active { &.narrow {} } }");
        let requirements: Vec<_> = f
            .modifier_rules
            .iter()
            .filter(|rule| rule.modifier == "narrow")
            .map(|rule| rule.required_all.clone())
            .collect();
        assert_eq!(
            requirements,
            [
                vec![String::from("first"), String::from("active")],
                vec![String::from("second"), String::from("active")]
            ]
        );
    }

    #[test]
    fn comments_are_trivia_for_nested_modifier_detection() {
        let source = ".base { // .ignored { }\n /* another .ignored */ &.active {} }";
        let f = parse_stylesheet(source);
        assert!(f.classes.iter().all(|class| class.name != "ignored"));
        let rule = f.modifier_rules.first().unwrap();
        assert_eq!(rule.modifier, "active");
        assert_eq!(rule.required_all, ["base"]);
        assert_eq!(
            &source[rule.modifier_span.start..rule.modifier_span.end],
            "active"
        );
    }

    #[test]
    fn nested_selector_without_local_class_does_not_panic() {
        let f = parse_stylesheet(".button { &:hover {} &[disabled] {} & > span {} }");
        assert!(f.modifier_rules.is_empty());
    }

    #[test]
    fn direct_compound_uses_independent_base() {
        let f = parse_stylesheet(".base {} .active {} .base.active {}");
        assert_eq!(f.modifier_rules.len(), 1);
        assert_eq!(f.modifier_rules[0].modifier, "active");
        assert_eq!(f.modifier_rules[0].required_all, ["base"]);
        assert!(f.independent_classes.contains(&"active".into()));
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
    #[test]
    fn extracts_sass_symbols_and_module_directives() {
        let source = r#"@use "src/tokens" as t with ($mode: dark);
@forward "src/tools" as ui-* show helper, $gap;
$local_value: t.$space;
@mixin card-tone { color: $local-value; @include t.paint; }
@function size-up($value) { @return t.scale($value); }
.x { width: size_up($local-value); }"#;
        let facts = parse_stylesheet(source);
        assert!(
            facts
                .sass_declarations
                .iter()
                .any(|d| d.kind == SassSymbolKind::Variable && d.name == "local_value")
        );
        assert!(
            facts
                .sass_declarations
                .iter()
                .any(|d| d.kind == SassSymbolKind::Mixin && d.name == "card-tone")
        );
        assert!(
            facts
                .sass_declarations
                .iter()
                .any(|d| d.kind == SassSymbolKind::Function && d.name == "size-up")
        );
        assert!(
            facts
                .sass_references
                .iter()
                .any(|r| r.kind == SassSymbolKind::Variable
                    && r.namespace.as_deref() == Some("t")
                    && r.name == "space")
        );
        assert!(
            facts
                .sass_references
                .iter()
                .any(|r| r.kind == SassSymbolKind::Mixin
                    && r.namespace.as_deref() == Some("t")
                    && r.name == "paint")
        );
        assert!(
            facts
                .sass_references
                .iter()
                .any(|r| r.kind == SassSymbolKind::Function
                    && r.namespace.as_deref() == Some("t")
                    && r.name == "scale")
        );
        assert_eq!(facts.sass_directives.len(), 2);
        assert!(
            facts.sass_directives[0]
                .member_spans
                .iter()
                .any(|(name, _)| name == "mode")
        );
        assert_eq!(facts.sass_directives[1].prefix.as_deref(), Some("ui-"));
        assert_eq!(facts.sass_directives[1].show, ["helper", "gap"]);
    }
}
