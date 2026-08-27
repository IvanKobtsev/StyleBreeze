use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use stylebreeze_resolver::{FileSystemResolver, Resolver};
use stylebreeze_stylesheet_parser::{Scope, parse_stylesheet};
use stylebreeze_typescript_parser::{AccessKind, parse_typescript};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub path: PathBuf,
    pub span: Span,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub location: Location,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub unnecessary: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextEdit {
    pub location: Location,
    pub new_text: String,
}
#[derive(Clone, Debug)]
struct Export {
    name: String,
    occurrences: Vec<Span>,
    independent: bool,
}
#[derive(Clone, Debug)]
struct Reference {
    module: PathBuf,
    name: String,
    span: Span,
    composition: Option<Span>,
    composition_certain: bool,
}
#[derive(Clone, Debug)]
struct ModifierRule {
    modifier: String,
    required_all: Vec<String>,
    modifier_span: Span,
    base_spans: Vec<Span>,
    selector: Span,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModifierDecoration {
    pub modifier: String,
    pub required_all: Vec<String>,
    pub range: Span,
    pub selector: Span,
    pub base_locations: Vec<Location>,
    pub standalone: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HoverInfo {
    pub range: Span,
    pub markdown: String,
}
#[derive(Clone, Debug)]
struct ModuleImport {
    binding: String,
    module: PathBuf,
}
#[derive(Clone, Debug)]
struct FileRecord {
    source: String,
    version: Option<i32>,
    exports: Vec<Export>,
    references: Vec<Reference>,
    imports: Vec<ModuleImport>,
    uncertain_modules: HashSet<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    modifier_rules: Vec<ModifierRule>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RenameError {
    #[error("no renameable CSS Module symbol at this position")]
    NoSymbol,
    #[error("the new class name is not a valid exact CSS Module identifier")]
    InvalidName,
    #[error("an export named '{0}' already exists")]
    Collision(String),
    #[error("the symbol cannot be resolved unambiguously")]
    Ambiguous,
}

pub struct Project {
    files: HashMap<PathBuf, FileRecord>,
    roots: Vec<PathBuf>,
    resolver: Box<dyn Resolver>,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            files: HashMap::new(),
            roots: Vec::new(),
            resolver: Box::new(FileSystemResolver::default()),
        }
    }
}

impl Project {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            ..Self::default()
        }
    }
    pub fn index_workspace(&mut self) {
        let roots = self.roots.clone();
        for root in roots {
            for e in WalkDir::new(root)
                .into_iter()
                .filter_entry(|e| {
                    !e.file_type().is_dir()
                        || !matches!(
                            e.file_name().to_str(),
                            Some(".git" | "target" | "node_modules" | ".idea")
                        )
                })
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
            {
                let p = e.path();
                if relevant(p)
                    && let Ok(s) = fs::read_to_string(p)
                {
                    self.open_or_update_file(p.to_path_buf(), s, None);
                }
            }
        }
    }
    pub fn open_or_update_file(&mut self, path: PathBuf, source: String, version: Option<i32>) {
        let path = canonical_or(path);
        let mut exports = Vec::new();
        let mut references = Vec::new();
        let mut imports = Vec::new();
        let mut uncertain_modules = HashSet::new();
        let mut diagnostics = Vec::new();
        let mut modifier_rules = Vec::new();
        if stylesheet(&path) {
            let facts = parse_stylesheet(&source);
            let independent: HashSet<_> = facts.independent_classes.iter().cloned().collect();
            let mut by_name: HashMap<String, Vec<Span>> = HashMap::new();
            for c in facts
                .classes
                .into_iter()
                .filter(|c| c.scope == Scope::Local)
            {
                by_name.entry(c.name).or_default().push(Span {
                    start: c.span.start,
                    end: c.span.end,
                });
            }
            exports = by_name
                .into_iter()
                .map(|(name, occurrences)| Export {
                    independent: independent.contains(&name),
                    name,
                    occurrences,
                })
                .collect();
            modifier_rules = facts
                .modifier_rules
                .into_iter()
                .map(|rule| ModifierRule {
                    modifier: rule.modifier,
                    required_all: rule.required_all,
                    modifier_span: Span {
                        start: rule.modifier_span.start,
                        end: rule.modifier_span.end,
                    },
                    base_spans: rule
                        .base_spans
                        .into_iter()
                        .map(|s| Span {
                            start: s.start,
                            end: s.end,
                        })
                        .collect(),
                    selector: Span {
                        start: rule.selector.start,
                        end: rule.selector.end,
                    },
                })
                .collect();
            for d in facts.diagnostics {
                diagnostics.push(Diagnostic {
                    location: Location {
                        path: path.clone(),
                        span: Span {
                            start: d.span.start,
                            end: d.span.end,
                        },
                    },
                    severity: Severity::Information,
                    code: "unresolved-sass-interpolation",
                    message: d.message,
                    unnecessary: false,
                });
            }
        } else {
            let facts = parse_typescript(&path, &source);
            let resolved_imports: HashMap<_, _> = facts
                .imports
                .iter()
                .filter_map(|i| {
                    self.resolver
                        .resolve_stylesheet(&path, &i.specifier)
                        .ok()
                        .map(|p| (i.binding.clone(), p))
                })
                .collect();
            imports = resolved_imports
                .iter()
                .map(|(binding, module)| ModuleImport {
                    binding: binding.clone(),
                    module: module.clone(),
                })
                .collect();
            let mut dynamic = HashSet::new();
            for a in facts.accesses {
                if let Some(module) = resolved_imports.get(&a.binding) {
                    if let Some(name) = a.class_name {
                        references.push(Reference {
                            module: module.clone(),
                            name,
                            span: Span {
                                start: a.span.start,
                                end: a.span.end,
                            },
                            composition: a.composition.map(|s| Span {
                                start: s.start,
                                end: s.end,
                            }),
                            composition_certain: a.composition_certain,
                        });
                    } else if a.kind == AccessKind::Dynamic {
                        dynamic.insert(a.binding);
                        diagnostics.push(Diagnostic {
                            location: Location {
                                path: path.clone(),
                                span: Span {
                                    start: a.span.start,
                                    end: a.span.end,
                                },
                            },
                            severity: Severity::Information,
                            code: "dynamic-module-access",
                            message:
                                "Dynamic CSS Module access cannot be resolved to a specific export"
                                    .into(),
                            unnecessary: false,
                        });
                    }
                }
            }
            uncertain_modules.extend(
                dynamic
                    .into_iter()
                    .filter_map(|binding| resolved_imports.get(&binding).cloned()),
            );
        }
        self.files.insert(
            path,
            FileRecord {
                source,
                version,
                exports,
                references,
                imports,
                uncertain_modules,
                diagnostics,
                modifier_rules,
            },
        );
    }
    pub fn close_file(&mut self, path: &Path) {
        let p = canonical_or(path.to_path_buf());
        if let Ok(s) = fs::read_to_string(&p) {
            self.open_or_update_file(p, s, None);
        } else {
            self.files.remove(&p);
        }
    }
    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(&canonical_or(path.to_path_buf()));
    }
    pub fn definition_at(&self, path: &Path, offset: usize) -> Option<Location> {
        self.definitions_at(path, offset).into_iter().next()
    }
    pub fn definitions_at(&self, path: &Path, offset: usize) -> Vec<Location> {
        let Some((module, name)) = self.symbol_at(path, offset) else {
            return vec![];
        };
        let Some(f) = self.files.get(&module) else {
            return vec![];
        };
        let Some(e) = f.exports.iter().find(|e| e.name == name) else {
            return vec![];
        };
        let source_path = canonical_or(path.to_path_buf());
        if source_path != module {
            let Some(source_file) = self.files.get(&source_path) else {
                return vec![];
            };
            let Some(reference) = source_file
                .references
                .iter()
                .find(|r| inside(r.span, offset))
            else {
                return vec![];
            };
            let modifier_rules: Vec<_> = f
                .modifier_rules
                .iter()
                .filter(|rule| rule.modifier == name)
                .collect();
            if modifier_rules.is_empty() {
                return e
                    .occurrences
                    .first()
                    .map(|span| {
                        vec![Location {
                            path: module,
                            span: *span,
                        }]
                    })
                    .unwrap_or_default();
            }
            let matched: Vec<_> = f
                .modifier_rules
                .iter()
                .filter(|rule| {
                    rule.modifier == name
                        && self.reference_satisfies(source_file, reference, &rule.required_all)
                })
                .map(|rule| Location {
                    path: module.clone(),
                    span: rule.modifier_span,
                })
                .collect();
            if !matched.is_empty() {
                return matched;
            }
            if !e.independent {
                return vec![];
            }
            let modifier_spans: HashSet<_> = f
                .modifier_rules
                .iter()
                .filter(|r| r.modifier == name)
                .map(|r| r.modifier_span)
                .collect();
            return e
                .occurrences
                .iter()
                .filter(|s| !modifier_spans.contains(s))
                .map(|s| Location {
                    path: module.clone(),
                    span: *s,
                })
                .collect();
        }
        vec![Location {
            path: module,
            span: *e.occurrences.first().unwrap(),
        }]
    }
    pub fn references_at(
        &self,
        path: &Path,
        offset: usize,
        include_declaration: bool,
    ) -> Vec<Location> {
        let Some((module, name)) = self.symbol_at(path, offset) else {
            return vec![];
        };
        let mut out = Vec::new();
        let source_path = canonical_or(path.to_path_buf());
        let selected_rules: Vec<_> = if source_path == module {
            self.files
                .get(&module)
                .into_iter()
                .flat_map(|file| file.modifier_rules.iter())
                .filter(|rule| inside(rule.modifier_span, offset))
                .collect()
        } else {
            Vec::new()
        };
        if include_declaration
            && let Some(f) = self.files.get(&module)
            && let Some(e) = f.exports.iter().find(|e| e.name == name)
        {
            out.extend(e.occurrences.iter().map(|s| Location {
                path: module.clone(),
                span: *s,
            }));
        }
        for (p, f) in &self.files {
            out.extend(
                f.references
                    .iter()
                    .filter(|r| {
                        r.module == module
                            && r.name == name
                            && (selected_rules.is_empty()
                                || selected_rules
                                    .iter()
                                    .any(|rule| self.reference_satisfies(f, r, &rule.required_all)))
                    })
                    .map(|r| Location {
                        path: p.clone(),
                        span: r.span,
                    }),
            );
        }
        out
    }
    pub fn prepare_rename(&self, path: &Path, offset: usize) -> Result<Span, RenameError> {
        let (_, name) = self.symbol_at(path, offset).ok_or(RenameError::NoSymbol)?;
        self.span_at(path, offset, &name)
            .ok_or(RenameError::NoSymbol)
    }
    pub fn rename(
        &self,
        path: &Path,
        offset: usize,
        new_name: &str,
    ) -> Result<Vec<TextEdit>, RenameError> {
        if !valid_name(new_name) {
            return Err(RenameError::InvalidName);
        }
        let (module, name) = self.symbol_at(path, offset).ok_or(RenameError::NoSymbol)?;
        let mf = self.files.get(&module).ok_or(RenameError::NoSymbol)?;
        if mf
            .exports
            .iter()
            .any(|e| e.name == new_name && e.name != name)
        {
            return Err(RenameError::Collision(new_name.into()));
        }
        let mut locs = self.references_at(path, offset, true);
        locs.sort_by(|a, b| a.path.cmp(&b.path).then(a.span.start.cmp(&b.span.start)));
        locs.dedup();
        if locs.is_empty() {
            return Err(RenameError::Ambiguous);
        }
        Ok(locs
            .into_iter()
            .map(|location| TextEdit {
                location,
                new_text: new_name.into(),
            })
            .collect())
    }
    pub fn diagnostics_for(&self, path: &Path) -> Vec<Diagnostic> {
        let p = canonical_or(path.to_path_buf());
        let mut out = self
            .files
            .get(&p)
            .map(|f| f.diagnostics.clone())
            .unwrap_or_default();
        if let Some(f) = self.files.get(&p) {
            for r in &f.references {
                if let Some(module) = self.files.get(&r.module)
                    && !module.exports.iter().any(|e| e.name == r.name)
                {
                    out.push(Diagnostic {
                        location: Location {
                            path: p.clone(),
                            span: r.span,
                        },
                        severity: Severity::Warning,
                        code: "unknown-export",
                        message: format!("CSS Module has no export named '{}'", r.name),
                        unnecessary: false,
                    });
                } else if let Some(module) = self.files.get(&r.module)
                    && let Some(export) = module.exports.iter().find(|e| e.name == r.name)
                    && !export.independent
                {
                    let rules: Vec<_> = module
                        .modifier_rules
                        .iter()
                        .filter(|rule| rule.modifier == r.name)
                        .collect();
                    if !rules.is_empty()
                        && r.composition_certain
                        && r.composition.is_some()
                        && !rules
                            .iter()
                            .any(|rule| self.reference_satisfies(f, r, &rule.required_all))
                        && !rules
                            .iter()
                            .any(|rule| self.reference_may_share_root(f, r, &rule.required_all))
                    {
                        let alternatives = rules
                            .iter()
                            .map(|rule| rule.required_all.join(" + "))
                            .collect::<Vec<_>>()
                            .join(" or ");
                        out.push(Diagnostic {
                            location: Location {
                                path: p.clone(),
                                span: r.span,
                            },
                            severity: Severity::Warning,
                            code: "dependent-modifier-without-base",
                            message: format!(
                                "Modifier '{}' requires {} in the same className composition",
                                r.name, alternatives
                            ),
                            unnecessary: false,
                        });
                    }
                }
            }
        }
        if let Some(f) = self.files.get(&p)
            && stylesheet(&p)
        {
            let usage_is_uncertain = self
                .files
                .values()
                .any(|candidate| candidate.uncertain_modules.contains(&p));
            if !usage_is_uncertain {
                for export in &f.exports {
                    let used = self.files.values().any(|candidate| {
                        candidate
                            .references
                            .iter()
                            .any(|reference| reference.module == p && reference.name == export.name)
                    });
                    if !used {
                        out.extend(export.occurrences.iter().map(|span| Diagnostic {
                            location: Location {
                                path: p.clone(),
                                span: *span,
                            },
                            severity: Severity::Hint,
                            code: "unused-export",
                            message: format!("CSS Module export '{}' is unused", export.name),
                            unnecessary: true,
                        }));
                    }
                }
            }
        }
        out
    }
    pub fn workspace_diagnostics(&self) -> Vec<Diagnostic> {
        self.files
            .keys()
            .flat_map(|p| self.diagnostics_for(p))
            .collect()
    }
    pub fn completions_at(&self, path: &Path, offset: usize) -> Vec<String> {
        let path = canonical_or(path.to_path_buf());
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        let Some((binding, prefix)) = member_context(&file.source, offset) else {
            return Vec::new();
        };
        let Some(import) = file.imports.iter().find(|i| i.binding == binding) else {
            return Vec::new();
        };
        let Some(module) = self.files.get(&import.module) else {
            return Vec::new();
        };
        let mut names: Vec<_> = module
            .exports
            .iter()
            .filter(|e| e.name.starts_with(prefix) && valid_dot_name(&e.name))
            .map(|e| e.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }
    pub fn modifier_decorations(&self, path: &Path) -> Vec<ModifierDecoration> {
        let path = canonical_or(path.to_path_buf());
        let Some(file) = self.files.get(&path) else {
            return vec![];
        };
        let mut grouped: HashMap<(String, Span, Span), ModifierDecoration> = HashMap::new();
        for rule in &file.modifier_rules {
            let standalone = file
                .exports
                .iter()
                .find(|e| e.name == rule.modifier)
                .is_some_and(|e| e.independent);
            let entry = grouped
                .entry((rule.modifier.clone(), rule.modifier_span, rule.selector))
                .or_insert_with(|| ModifierDecoration {
                    modifier: rule.modifier.clone(),
                    required_all: Vec::new(),
                    range: rule.modifier_span,
                    selector: rule.selector,
                    base_locations: Vec::new(),
                    standalone,
                });
            for required in &rule.required_all {
                if !entry.required_all.contains(required) {
                    entry.required_all.push(required.clone());
                }
            }
            entry
                .base_locations
                .extend(rule.base_spans.iter().map(|span| Location {
                    path: path.clone(),
                    span: *span,
                }));
        }
        let mut out: Vec<_> = grouped.into_values().collect();
        for item in &mut out {
            item.required_all.sort();
            item.base_locations.sort_by_key(|l| l.span.start);
            item.base_locations.dedup();
        }
        out.sort_by_key(|d| d.range.start);
        out
    }
    pub fn hover_at(&self, path: &Path, offset: usize) -> Option<HoverInfo> {
        let source_path = canonical_or(path.to_path_buf());
        let (module, name) = self.symbol_at(&source_path, offset)?;
        let module_file = self.files.get(&module)?;
        let mut rules: Vec<_> = module_file
            .modifier_rules
            .iter()
            .filter(|r| r.modifier == name)
            .collect();
        if source_path == module {
            let occurrence_rules: Vec<_> = rules
                .iter()
                .copied()
                .filter(|r| inside(r.modifier_span, offset))
                .collect();
            if occurrence_rules.is_empty() {
                return None;
            }
            rules = occurrence_rules;
        } else if let Some(source_file) = self.files.get(&source_path)
            && let Some(reference) = source_file
                .references
                .iter()
                .find(|r| inside(r.span, offset))
        {
            let matched: Vec<_> = rules
                .iter()
                .copied()
                .filter(|r| self.reference_satisfies(source_file, reference, &r.required_all))
                .collect();
            if !matched.is_empty() {
                rules = matched;
            }
        }
        if rules.is_empty() {
            return None;
        }
        let mut bases: Vec<_> = rules
            .iter()
            .flat_map(|r| r.required_all.iter().cloned())
            .collect();
        bases.sort();
        bases.dedup();
        let standalone = module_file
            .exports
            .iter()
            .find(|e| e.name == name)
            .is_some_and(|e| e.independent);
        let requirement = bases
            .iter()
            .map(|b| format!("`.{b}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        let suffix = if standalone {
            "\n\nAlso has independently applicable styles."
        } else {
            "\n\nHas no independently applicable selector in this module."
        };
        Some(HoverInfo {
            range: self.span_at(&source_path, offset, &name)?,
            markdown: format!("**Dependent modifier**\n\nRequires {requirement}.{suffix}"),
        })
    }
    pub fn file_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }
    pub fn source(&self, path: &Path) -> Option<&str> {
        self.files
            .get(&canonical_or(path.to_path_buf()))
            .map(|f| f.source.as_str())
    }
    pub fn version(&self, path: &Path) -> Option<i32> {
        self.files
            .get(&canonical_or(path.to_path_buf()))
            .and_then(|f| f.version)
    }
    fn reference_satisfies(
        &self,
        file: &FileRecord,
        reference: &Reference,
        required: &[String],
    ) -> bool {
        let Some(composition) = reference.composition else {
            return false;
        };
        required.iter().all(|name| {
            file.references.iter().any(|candidate| {
                candidate.module == reference.module
                    && candidate.name == *name
                    && candidate.composition.is_some_and(|candidate_composition| {
                        span_contains(candidate_composition, composition)
                    })
            })
        })
    }
    fn reference_may_share_root(
        &self,
        file: &FileRecord,
        reference: &Reference,
        required: &[String],
    ) -> bool {
        let Some(composition) = reference.composition else {
            return false;
        };
        required.iter().all(|name| {
            file.references.iter().any(|candidate| {
                candidate.module == reference.module
                    && candidate.name == *name
                    && candidate.composition.is_some_and(|candidate_composition| {
                        span_contains(composition, candidate_composition)
                    })
            })
        })
    }
    fn symbol_at(&self, path: &Path, offset: usize) -> Option<(PathBuf, String)> {
        let p = canonical_or(path.to_path_buf());
        let f = self.files.get(&p)?;
        if stylesheet(&p) {
            for e in &f.exports {
                if e.occurrences.iter().any(|s| inside(*s, offset)) {
                    return Some((p.clone(), e.name.clone()));
                }
            }
        } else {
            for r in &f.references {
                if inside(r.span, offset) {
                    return Some((r.module.clone(), r.name.clone()));
                }
            }
        }
        None
    }
    fn span_at(&self, path: &Path, offset: usize, name: &str) -> Option<Span> {
        let p = canonical_or(path.to_path_buf());
        let f = self.files.get(&p)?;
        if stylesheet(&p) {
            f.exports
                .iter()
                .find(|e| e.name == name)?
                .occurrences
                .iter()
                .copied()
                .find(|s| inside(*s, offset))
        } else {
            f.references
                .iter()
                .find(|r| r.name == name && inside(r.span, offset))
                .map(|r| r.span)
        }
    }
}
fn inside(s: Span, o: usize) -> bool {
    o >= s.start && o <= s.end
}
fn span_contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && outer.end >= inner.end
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !s.as_bytes()[0].is_ascii_digit()
}
fn valid_dot_name(s: &str) -> bool {
    !s.is_empty()
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
}
fn member_context(source: &str, offset: usize) -> Option<(&str, &str)> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }
    let bytes = source.as_bytes();
    let mut prefix_start = offset;
    while prefix_start > 0 && ident_member(bytes[prefix_start - 1]) {
        prefix_start -= 1;
    }
    if prefix_start == 0 || bytes[prefix_start - 1] != b'.' {
        return None;
    }
    let binding_end = prefix_start - 1;
    let mut binding_start = binding_end;
    while binding_start > 0 && ident_member(bytes[binding_start - 1]) {
        binding_start -= 1;
    }
    (binding_start < binding_end).then(|| {
        (
            &source[binding_start..binding_end],
            &source[prefix_start..offset],
        )
    })
}
fn ident_member(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$')
}
fn canonical_or(p: PathBuf) -> PathBuf {
    p.canonicalize().unwrap_or(p)
}
fn stylesheet(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.ends_with(".module.css") || s.ends_with(".module.scss")
}
fn relevant(p: &Path) -> bool {
    stylesheet(p)
        || matches!(
            p.extension().and_then(|x| x.to_str()),
            Some("js" | "jsx" | "ts" | "tsx")
        ) && !p.to_string_lossy().ends_with(".d.ts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn navigation_diagnostics_and_rename() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        fs::write(&css, ":where(.myClass) {}").unwrap();
        fs::write(
            &ts,
            "import styles from './x.module.scss'; styles.myClass; styles.missing",
        )
        .unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let src = p.source(&ts).unwrap();
        let at = src.find("myClass").unwrap();
        assert_eq!(
            p.definition_at(&ts, at).unwrap().path,
            css.canonicalize().unwrap()
        );
        assert_eq!(p.diagnostics_for(&ts).len(), 1);
        assert_eq!(p.diagnostics_for(&ts)[0].severity, Severity::Warning);
        assert_eq!(p.rename(&ts, at, "renamed").unwrap().len(), 2);
        let completion_offset =
            p.source(&ts).unwrap().find("styles.myClass").unwrap() + "styles.".len();
        assert_eq!(p.completions_at(&ts, completion_offset), ["myClass"]);
    }

    #[test]
    fn reports_unused_exports_but_not_for_dynamic_module_access() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        fs::write(&css, ".used {} .unused {}").unwrap();
        fs::write(&ts, "import styles from './x.module.scss'; styles.used;").unwrap();

        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let diagnostics = p.diagnostics_for(&css);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-export");
        assert_eq!(diagnostics[0].severity, Severity::Hint);
        assert!(diagnostics[0].unnecessary);

        p.open_or_update_file(
            ts,
            "import styles from './x.module.scss'; styles[name];".into(),
            Some(2),
        );
        assert!(p.diagnostics_for(&css).is_empty());
    }

    #[test]
    fn pair_aware_modifier_navigation_and_diagnostics() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        fs::write(&css, ".first { &.active {} } .second { &.active {} }").unwrap();
        fs::write(&ts, "import s from './x.module.scss'; <><i className={clsx(s.first, s.active)} /><i className={clsx(s.second, s.active)} /><i className={clsx(s.active)} /></>").unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let ts_source = p.source(&ts).unwrap();
        let first_active = ts_source.find("s.active").unwrap() + 2;
        let second_active =
            ts_source[first_active + 1..].find("s.active").unwrap() + first_active + 3;
        let first_definition = p.definitions_at(&ts, first_active);
        let second_definition = p.definitions_at(&ts, second_active);
        assert_eq!(first_definition.len(), 1);
        assert_eq!(second_definition.len(), 1);
        assert_ne!(first_definition[0].span, second_definition[0].span);
        let diagnostics = p.diagnostics_for(&ts);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == "dependent-modifier-without-base")
                .count(),
            1
        );

        let css_source = p.source(&css).unwrap();
        let first_css_active = css_source.find("active").unwrap();
        let usages = p.references_at(&css, first_css_active, false);
        assert_eq!(usages.len(), 1);
        assert_eq!(
            &ts_source[usages[0].span.start..usages[0].span.end],
            "active"
        );
    }

    #[test]
    fn standalone_modifier_is_valid_fallback() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        fs::write(&css, ".base { &.active {} } .active {}").unwrap();
        fs::write(
            &ts,
            "import s from './x.module.scss'; <i className={s.active} />",
        )
        .unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let source = p.source(&ts).unwrap();
        let at = source.rfind("active").unwrap();
        assert!(
            p.diagnostics_for(&ts)
                .iter()
                .all(|d| d.code != "dependent-modifier-without-base")
        );
        let definitions = p.definitions_at(&ts, at);
        assert_eq!(definitions.len(), 1);
        let css_source = p.source(&css).unwrap();
        assert_eq!(
            &css_source[definitions[0].span.start..definitions[0].span.end],
            "active"
        );
        assert!(definitions[0].span.start > css_source.find("&.active").unwrap());
    }
}
