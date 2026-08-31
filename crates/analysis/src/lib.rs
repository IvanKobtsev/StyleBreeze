use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use stylebreeze_resolver::{FileSystemResolver, Resolver};
use stylebreeze_stylesheet_parser::{
    SassDirectiveKind, SassSymbolKind, Scope, SelectorPreview, SelectorRule, UnsupportedReason,
    parse_stylesheet, preview_selector,
};
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub label: String,
    pub kind: Option<SassSymbolKind>,
    pub detail: String,
    pub replace_span: Span,
    pub additional_edits: Vec<TextEdit>,
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
pub struct ModifierRequirement {
    pub required_all: Vec<String>,
    pub base_locations: Vec<Location>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModifierDecoration {
    pub modifier: String,
    pub alternatives: Vec<ModifierRequirement>,
    pub range: Span,
    pub selector: Span,
    pub standalone: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HoverInfo {
    pub range: Span,
    pub markdown: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorPreviewInfo {
    pub range: Span,
    pub preview: Option<SelectorPreview>,
    pub unsupported: Option<UnsupportedReason>,
}
#[derive(Clone, Debug)]
struct ModuleImport {
    binding: String,
    module: PathBuf,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPropertyOccurrence {
    pub name: String,
    pub span: Span,
    pub declaration: bool,
    pub roles: Vec<String>,
}
#[derive(Clone, Debug)]
struct PropertyDeclaration {
    name: String,
    span: Span,
    global: bool,
    registered: bool,
    syntax: Option<String>,
    inherits: Option<bool>,
    initial_value: Option<String>,
    selector: Option<String>,
}
#[derive(Clone, Debug)]
struct PropertyReference {
    name: String,
    span: Span,
    line: usize,
}
#[derive(Clone, Debug)]
struct PropertyImport {
    source: PathBuf,
    path_span: Span,
    names: Vec<(String, Span)>,
}
#[derive(Clone, Debug)]
struct SassDeclaration {
    name: String,
    span: Span,
    kind: SassSymbolKind,
    private: bool,
}
#[derive(Clone, Debug)]
struct SassReference {
    name: String,
    span: Span,
    kind: SassSymbolKind,
    namespace: Option<String>,
}
#[derive(Clone, Debug)]
struct SassDependency {
    kind: SassDirectiveKind,
    source: Option<PathBuf>,
    path: String,
    path_span: Span,
    namespace: Option<String>,
    star: bool,
    prefix: Option<String>,
    show: Vec<String>,
    hide: Vec<String>,
    member_spans: Vec<(String, Span)>,
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
    selectors: Vec<SelectorRule>,
    property_declarations: Vec<PropertyDeclaration>,
    property_references: Vec<PropertyReference>,
    property_imports: Vec<PropertyImport>,
    property_exports: Vec<(String, Span)>,
    suppressed_lines: Vec<usize>,
    sass_declarations: Vec<SassDeclaration>,
    sass_references: Vec<SassReference>,
    sass_dependencies: Vec<SassDependency>,
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
    global_selectors: Vec<String>,
    property_presentation: HashMap<String, String>,
    sass_load_roots: Vec<PathBuf>,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            files: HashMap::new(),
            roots: Vec::new(),
            resolver: Box::new(FileSystemResolver::default()),
            global_selectors: vec![":root".into()],
            property_presentation: [
                "global",
                "local",
                "registered",
                "imported",
                "exported",
                "unresolved",
            ]
            .into_iter()
            .map(|r| (r.into(), "semantic".into()))
            .collect(),
            sass_load_roots: Vec::new(),
        }
    }
}

impl Project {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            sass_load_roots: roots.clone(),
            roots,
            ..Self::default()
        }
    }
    pub fn set_sass_load_roots(&mut self, roots: Vec<PathBuf>) {
        let roots = if roots.is_empty() {
            self.roots.clone()
        } else {
            roots
        };
        if roots == self.sass_load_roots {
            return;
        }
        self.sass_load_roots = roots;
        let files: Vec<_> = self
            .files
            .iter()
            .map(|(p, f)| (p.clone(), f.source.clone(), f.version))
            .collect();
        for (path, source, version) in files {
            self.open_or_update_file(path, source, version);
        }
    }
    pub fn set_sass_load_root_strings(&mut self, roots: Vec<String>) {
        let base = self.roots.first().cloned().unwrap_or_default();
        self.set_sass_load_roots(
            roots
                .into_iter()
                .map(|root| {
                    let path = PathBuf::from(root);
                    if path.is_absolute() {
                        path
                    } else {
                        base.join(path)
                    }
                })
                .collect(),
        );
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
        let mut selectors = Vec::new();
        let mut property_declarations = Vec::new();
        let mut property_references = Vec::new();
        let mut property_imports = Vec::new();
        let mut property_exports = Vec::new();
        let mut suppressed_lines = Vec::new();
        let mut sass_declarations = Vec::new();
        let mut sass_references = Vec::new();
        let mut sass_dependencies = Vec::new();
        if stylesheet(&path) {
            let facts = parse_stylesheet(&source);
            sass_declarations = facts
                .sass_declarations
                .iter()
                .map(|d| SassDeclaration {
                    name: d.name.clone(),
                    span: Span {
                        start: d.span.start,
                        end: d.span.end,
                    },
                    kind: d.kind,
                    private: d.private,
                })
                .collect();
            sass_references = facts
                .sass_references
                .iter()
                .map(|r| SassReference {
                    name: r.name.clone(),
                    span: Span {
                        start: r.span.start,
                        end: r.span.end,
                    },
                    kind: r.kind,
                    namespace: r.namespace.clone(),
                })
                .collect();
            sass_dependencies = facts
                .sass_directives
                .iter()
                .map(|d| SassDependency {
                    kind: d.kind,
                    source: self
                        .resolver
                        .resolve_sass(&path, &d.path, &self.sass_load_roots)
                        .ok(),
                    path: d.path.clone(),
                    path_span: Span {
                        start: d.path_span.start,
                        end: d.path_span.end,
                    },
                    namespace: d.namespace.clone(),
                    star: d.star,
                    prefix: d.prefix.clone(),
                    show: d.show.clone(),
                    hide: d.hide.clone(),
                    member_spans: d
                        .member_spans
                        .iter()
                        .map(|(name, span)| {
                            (
                                name.clone(),
                                Span {
                                    start: span.start,
                                    end: span.end,
                                },
                            )
                        })
                        .collect(),
                })
                .collect();
            selectors = facts.selectors.clone();
            property_declarations = facts
                .custom_property_declarations
                .iter()
                .map(|d| PropertyDeclaration {
                    name: d.name.clone(),
                    span: Span {
                        start: d.span.start,
                        end: d.span.end,
                    },
                    global: d.selector.as_deref().is_some_and(selector_is_global),
                    registered: d.registered,
                    syntax: d.syntax.clone(),
                    inherits: d.inherits,
                    initial_value: d.initial_value.clone(),
                    selector: d.selector.clone(),
                })
                .collect();
            property_references = facts
                .custom_property_references
                .iter()
                .map(|r| PropertyReference {
                    name: r.name.clone(),
                    span: Span {
                        start: r.span.start,
                        end: r.span.end,
                    },
                    line: r.line,
                })
                .collect();
            property_exports = facts
                .property_annotations
                .exports
                .iter()
                .map(|(n, s)| {
                    (
                        n.clone(),
                        Span {
                            start: s.start,
                            end: s.end,
                        },
                    )
                })
                .collect();
            suppressed_lines = facts.property_annotations.suppress_next_lines.clone();
            property_imports = facts
                .property_annotations
                .imports
                .iter()
                .map(|import| {
                    let source_path = path.parent().unwrap_or(Path::new("")).join(&import.path);
                    PropertyImport {
                        source: canonical_or(source_path),
                        path_span: Span {
                            start: import.path_span.start,
                            end: import.path_span.end,
                        },
                        names: import
                            .names
                            .iter()
                            .map(|(n, s)| {
                                (
                                    n.clone(),
                                    Span {
                                        start: s.start,
                                        end: s.end,
                                    },
                                )
                            })
                            .collect(),
                    }
                })
                .collect();
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
            if !module_stylesheet(&path) {
                exports.clear();
                modifier_rules.clear();
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
                selectors,
                property_declarations,
                property_references,
                property_imports,
                property_exports,
                suppressed_lines,
                sass_declarations,
                sass_references,
                sass_dependencies,
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
    pub fn set_global_selectors(&mut self, selectors: Vec<String>) {
        self.global_selectors = if selectors.is_empty() {
            vec![":root".into()]
        } else {
            selectors
        };
        for file in self.files.values_mut() {
            for declaration in &mut file.property_declarations {
                declaration.global = declaration
                    .selector
                    .as_deref()
                    .is_some_and(|s| selector_matches_globals(s, &self.global_selectors));
            }
        }
    }
    pub fn set_property_presentation(&mut self, presentation: HashMap<String, String>) {
        for (role, mode) in presentation {
            if matches!(mode.as_str(), "semantic" | "inlayHint" | "off") {
                self.property_presentation.insert(role, mode);
            }
        }
    }
    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(&canonical_or(path.to_path_buf()));
    }
    pub fn definition_at(&self, path: &Path, offset: usize) -> Option<Location> {
        self.definitions_at(path, offset).into_iter().next()
    }
    pub fn definitions_at(&self, path: &Path, offset: usize) -> Vec<Location> {
        let p = canonical_or(path.to_path_buf());
        if let Some((origin, kind, name)) = self.sass_symbol_at(&p, offset) {
            return self
                .files
                .get(&origin)
                .into_iter()
                .flat_map(|f| f.sass_declarations.iter())
                .filter(|d| d.kind == kind && canonical_sass_name(&d.name) == name)
                .map(|d| Location {
                    path: origin.clone(),
                    span: d.span,
                })
                .collect();
        }
        if let Some((name, _)) = self.property_symbol_at(&p, offset) {
            if let Some(f) = self.files.get(&p) {
                for import in &f.property_imports {
                    let selected = import
                        .names
                        .iter()
                        .any(|(n, s)| n == &name && inside(*s, offset))
                        || f.property_references
                            .iter()
                            .any(|r| r.name == name && inside(r.span, offset));
                    if selected && let Some(source) = self.files.get(&import.source) {
                        let found: Vec<_> = source
                            .property_declarations
                            .iter()
                            .filter(|d| d.name == name)
                            .map(|d| Location {
                                path: import.source.clone(),
                                span: d.span,
                            })
                            .chain(
                                source
                                    .property_exports
                                    .iter()
                                    .filter(|(n, _)| n == &name)
                                    .map(|(_, s)| Location {
                                        path: import.source.clone(),
                                        span: *s,
                                    }),
                            )
                            .collect();
                        if !found.is_empty() {
                            return found;
                        }
                    }
                }
            }
            let mut found = Vec::new();
            if let Some(f) = self.files.get(&p) {
                found.extend(
                    f.property_declarations
                        .iter()
                        .filter(|d| d.name == name)
                        .map(|d| Location {
                            path: p.clone(),
                            span: d.span,
                        }),
                );
                found.extend(f.property_exports.iter().filter(|(n, _)| n == &name).map(
                    |(_, s)| Location {
                        path: p.clone(),
                        span: *s,
                    },
                ));
            }
            if found.is_empty() {
                for (q, f) in &self.files {
                    found.extend(
                        f.property_declarations
                            .iter()
                            .filter(|d| d.name == name && (d.global || d.registered))
                            .map(|d| Location {
                                path: q.clone(),
                                span: d.span,
                            }),
                    );
                }
            }
            return found;
        }
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
        let p = canonical_or(path.to_path_buf());
        if let Some((origin, kind, name)) = self.sass_symbol_at(&p, offset) {
            let mut out = Vec::new();
            for (file_path, file) in &self.files {
                for reference in &file.sass_references {
                    if reference.kind == kind
                        && self
                            .resolve_sass_reference(file_path, reference)
                            .is_some_and(|(o, n)| o == origin && n == name)
                    {
                        out.push(Location {
                            path: file_path.clone(),
                            span: reference.span,
                        });
                    }
                }
                for dependency in &file.sass_dependencies {
                    let Some(source) = &dependency.source else {
                        continue;
                    };
                    for (member, span) in &dependency.member_spans {
                        let member_name = canonical_sass_name(member);
                        let kind_matches = dependency.kind == SassDirectiveKind::Forward
                            || kind == SassSymbolKind::Variable;
                        if kind_matches
                            && self
                                .sass_export_origin(source, kind, &member_name, &mut HashSet::new())
                                .as_ref()
                                == Some(&origin)
                        {
                            out.push(Location {
                                path: file_path.clone(),
                                span: *span,
                            });
                        }
                    }
                }
            }
            if include_declaration && let Some(file) = self.files.get(&origin) {
                out.extend(
                    file.sass_declarations
                        .iter()
                        .filter(|d| d.kind == kind && canonical_sass_name(&d.name) == name)
                        .map(|d| Location {
                            path: origin.clone(),
                            span: d.span,
                        }),
                );
            }
            out.sort_by(|a, b| a.path.cmp(&b.path).then(a.span.start.cmp(&b.span.start)));
            out.dedup();
            return out;
        }
        if let Some((name, _)) = self.property_symbol_at(&p, offset) {
            let roles = self.property_roles(&p, &name);
            let project_wide = roles.iter().any(|r| r == "global" || r == "registered");
            let origin = self.property_origin(&p, &name);
            let mut out = Vec::new();
            for (q, f) in &self.files {
                let participates = project_wide
                    || origin.as_ref().is_some_and(|source| {
                        q == source
                            || f.property_imports.iter().any(|i| {
                                &i.source == source && i.names.iter().any(|(n, _)| n == &name)
                            })
                    });
                if !participates {
                    continue;
                }
                out.extend(
                    f.property_references
                        .iter()
                        .filter(|r| r.name == name)
                        .map(|r| Location {
                            path: q.clone(),
                            span: r.span,
                        }),
                );
                for i in &f.property_imports {
                    out.extend(
                        i.names
                            .iter()
                            .filter(|(n, _)| n == &name)
                            .map(|(_, s)| Location {
                                path: q.clone(),
                                span: *s,
                            }),
                    );
                }
                out.extend(
                    f.property_exports
                        .iter()
                        .filter(|(n, _)| n == &name)
                        .map(|(_, s)| Location {
                            path: q.clone(),
                            span: *s,
                        }),
                );
                if include_declaration {
                    out.extend(
                        f.property_declarations
                            .iter()
                            .filter(|d| d.name == name)
                            .map(|d| Location {
                                path: q.clone(),
                                span: d.span,
                            }),
                    );
                }
            }
            out.sort_by(|a, b| a.path.cmp(&b.path).then(a.span.start.cmp(&b.span.start)));
            out.dedup();
            return out;
        }
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
        let p = canonical_or(path.to_path_buf());
        if self.sass_symbol_at(&p, offset).is_some() {
            return self.sass_span_at(&p, offset).ok_or(RenameError::NoSymbol);
        }
        if let Some((_, span)) = self.property_symbol_at(&p, offset) {
            return Ok(span);
        }
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
        let p = canonical_or(path.to_path_buf());
        if let Some((origin, kind, old_name)) = self.sass_symbol_at(&p, offset) {
            let clean = new_name.trim_start_matches('$');
            if !valid_sass_name(clean) {
                return Err(RenameError::InvalidName);
            }
            if self.files.get(&origin).is_some_and(|f| {
                f.sass_declarations.iter().any(|d| {
                    d.kind == kind
                        && canonical_sass_name(&d.name) == canonical_sass_name(clean)
                        && canonical_sass_name(&d.name) != old_name
                })
            }) {
                return Err(RenameError::Collision(clean.into()));
            }
            return Ok(self
                .references_at(&p, offset, true)
                .into_iter()
                .map(|location| TextEdit {
                    location,
                    new_text: clean.into(),
                })
                .collect());
        }
        if let Some((name, _)) = self.property_symbol_at(&p, offset) {
            if !valid_custom_property_name(new_name) {
                return Err(RenameError::InvalidName);
            }
            if self.files.values().any(|f| {
                f.property_declarations
                    .iter()
                    .any(|d| d.name == new_name && d.name != name)
            }) {
                return Err(RenameError::Collision(new_name.into()));
            }
            return Ok(self
                .references_at(&p, offset, true)
                .into_iter()
                .map(|location| TextEdit {
                    location,
                    new_text: new_name.into(),
                })
                .collect());
        }
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
            for reference in &f.property_references {
                if !f.suppressed_lines.contains(&reference.line)
                    && !self.property_resolved(&p, &reference.name)
                {
                    out.push(Diagnostic { location: Location { path: p.clone(), span: reference.span }, severity: Severity::Warning,
                        code: "unresolved-custom-property", message: format!("Custom property '{}' is not declared in this module, globally, or as an explicit dependency", reference.name), unnecessary: false });
                }
            }
            let mut seen = HashSet::new();
            for import in &f.property_imports {
                if !self.files.contains_key(&import.source) {
                    out.push(Diagnostic {
                        location: Location {
                            path: p.clone(),
                            span: import.path_span,
                        },
                        severity: Severity::Warning,
                        code: "missing-property-import-source",
                        message: "Imported property stylesheet cannot be resolved".into(),
                        unnecessary: false,
                    });
                }
                for (name, span) in &import.names {
                    if !seen.insert((import.source.clone(), name.clone())) {
                        out.push(Diagnostic {
                            location: Location {
                                path: p.clone(),
                                span: *span,
                            },
                            severity: Severity::Warning,
                            code: "duplicate-property-import",
                            message: format!("Property '{}' is imported more than once", name),
                            unnecessary: false,
                        });
                    } else if let Some(source) = self.files.get(&import.source) {
                        if !source.property_declarations.iter().any(|d| d.name == *name)
                            && !source.property_exports.iter().any(|(n, _)| n == name)
                        {
                            out.push(Diagnostic {
                                location: Location {
                                    path: p.clone(),
                                    span: *span,
                                },
                                severity: Severity::Warning,
                                code: "missing-imported-property",
                                message: format!(
                                    "Property '{}' is not declared or exported by this stylesheet",
                                    name
                                ),
                                unnecessary: false,
                            });
                        } else if !f.property_references.iter().any(|r| r.name == *name) {
                            out.push(Diagnostic {
                                location: Location {
                                    path: p.clone(),
                                    span: *span,
                                },
                                severity: Severity::Hint,
                                code: "unused-property-import",
                                message: format!("Imported property '{}' is unused", name),
                                unnecessary: true,
                            });
                        }
                    }
                }
            }
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
                    let mut references = Vec::new();
                    for candidate in self.files.values() {
                        for reference in &candidate.references {
                            if reference.module == p && reference.name == export.name {
                                references.push((candidate, reference));
                            }
                        }
                    }
                    let rules: Vec<_> = f
                        .modifier_rules
                        .iter()
                        .filter(|rule| rule.modifier == export.name)
                        .collect();
                    if rules.is_empty() {
                        if references.is_empty() {
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
                        continue;
                    }
                    let modifier_spans: HashSet<_> =
                        rules.iter().map(|rule| rule.modifier_span).collect();
                    if references.is_empty() {
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
                        continue;
                    }
                    let relationship_usage_unknown = references
                        .iter()
                        .any(|(_, reference)| !reference.composition_certain);
                    let mut used_relationships = HashSet::new();
                    if !relationship_usage_unknown {
                        for rule in &rules {
                            if references.iter().any(|(candidate, reference)| {
                                self.reference_satisfies(candidate, reference, &rule.required_all)
                            }) {
                                used_relationships.insert(modifier_relationship_key(rule));
                            }
                        }
                    }
                    if !relationship_usage_unknown {
                        for span in &modifier_spans {
                            let occurrence_used = rules.iter().any(|rule| {
                                rule.modifier_span == *span
                                    && used_relationships.contains(&modifier_relationship_key(rule))
                            });
                            if !occurrence_used {
                                out.push(Diagnostic {
                                    location: Location {
                                        path: p.clone(),
                                        span: *span,
                                    },
                                    severity: Severity::Hint,
                                    code: "unused-export",
                                    message: format!(
                                        "CSS Module modifier relationship '{}' is unused",
                                        export.name
                                    ),
                                    unnecessary: true,
                                });
                            }
                        }
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
    pub fn completion_items_at(&self, path: &Path, offset: usize) -> Vec<Completion> {
        let path = canonical_or(path.to_path_buf());
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        if let Some((kind, namespace, prefix, replace_span)) =
            sass_completion_context(&file.source, offset)
        {
            let mut out = Vec::new();
            let mut seen = HashSet::new();
            for (source_path, source_file) in &self.files {
                if source_path.extension().and_then(|e| e.to_str()) != Some("scss") {
                    continue;
                }
                for declaration in &source_file.sass_declarations {
                    if declaration.private
                        || declaration.kind != kind
                        || !canonical_sass_name(&declaration.name)
                            .starts_with(&canonical_sass_name(&prefix))
                    {
                        continue;
                    }
                    if let Some(ns) = &namespace {
                        let dependency_matches = file.sass_dependencies.iter().any(|d| {
                            d.kind == SassDirectiveKind::Use
                                && d.namespace.as_deref() == Some(ns)
                                && d.source.as_ref() == Some(source_path)
                        });
                        if !dependency_matches {
                            continue;
                        }
                    }
                    let fake = SassReference {
                        name: declaration.name.clone(),
                        span: replace_span,
                        kind,
                        namespace: namespace.clone(),
                    };
                    let visible = self
                        .resolve_sass_reference(&path, &fake)
                        .is_some_and(|(origin, _)| origin == *source_path);
                    let mut additional_edits = Vec::new();
                    if !visible
                        && namespace.is_none()
                        && source_path != &path
                        && !file
                            .sass_dependencies
                            .iter()
                            .any(|d| d.source.as_ref() == Some(source_path))
                        && let Some(specifier) = self
                            .resolver
                            .sass_specifier(source_path, &self.sass_load_roots)
                    {
                        additional_edits.push(TextEdit {
                            location: Location {
                                path: path.clone(),
                                span: Span {
                                    start: sass_import_insertion_offset(&file.source),
                                    end: sass_import_insertion_offset(&file.source),
                                },
                            },
                            new_text: format!(
                                "@use \"{specifier}\" as *;{}",
                                newline(&file.source)
                            ),
                        });
                    }
                    if visible
                        || !additional_edits.is_empty()
                        || namespace.is_some()
                        || source_path == &path
                    {
                        let identity = (declaration.name.clone(), source_path.clone());
                        if seen.insert(identity) {
                            out.push(Completion {
                                label: declaration.name.clone(),
                                kind: Some(kind),
                                detail: format!(
                                    "{} — {}",
                                    sass_kind_name(kind),
                                    self.resolver
                                        .sass_specifier(source_path, &self.sass_load_roots)
                                        .unwrap_or_else(|| source_path.display().to_string())
                                ),
                                replace_span,
                                additional_edits,
                            });
                        }
                    }
                }
            }
            out.sort_by(|a, b| a.label.cmp(&b.label).then(a.detail.cmp(&b.detail)));
            return out;
        }
        self.completions_at(path.as_path(), offset)
            .into_iter()
            .map(|label| Completion {
                label,
                kind: None,
                detail: "CSS Module export".into(),
                replace_span: Span {
                    start: offset,
                    end: offset,
                },
                additional_edits: Vec::new(),
            })
            .collect()
    }
    pub fn fix_sass_imports(&self, path: &Path) -> Vec<TextEdit> {
        let path = canonical_or(path.to_path_buf());
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        file.sass_dependencies
            .iter()
            .filter(|d| d.path.starts_with('.'))
            .filter_map(|dependency| {
                let target = dependency.source.as_ref()?;
                let mut specifier = self
                    .resolver
                    .sass_specifier(target, &self.sass_load_roots)?;
                if Path::new(&dependency.path).extension().is_none() {
                    specifier = specifier.trim_end_matches(".scss").to_string();
                    if let Some((parent, name)) = specifier.rsplit_once('/')
                        && let Some(name) = name.strip_prefix('_')
                    {
                        specifier = format!("{parent}/{name}");
                    } else if let Some(name) = specifier.strip_prefix('_') {
                        specifier = name.to_string();
                    }
                }
                (specifier != dependency.path).then(|| TextEdit {
                    location: Location {
                        path: path.clone(),
                        span: dependency.path_span,
                    },
                    new_text: specifier,
                })
            })
            .collect()
    }
    pub fn completions_at(&self, path: &Path, offset: usize) -> Vec<String> {
        let path = canonical_or(path.to_path_buf());
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        if let Some(import) = file.property_imports.iter().find(|i| {
            offset >= i.path_span.end
                && file.source[i.path_span.end..]
                    .find("*/")
                    .is_some_and(|relative_end| offset <= i.path_span.end + relative_end)
        }) {
            let prefix = property_prefix(&file.source, offset);
            let mut names: Vec<String> = self
                .files
                .get(&import.source)
                .map(|f| {
                    f.property_declarations
                        .iter()
                        .map(|d| d.name.clone())
                        .chain(f.property_exports.iter().map(|(n, _)| n.clone()))
                        .filter(|n| n.starts_with(prefix))
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names.dedup();
            return names;
        }
        if let Some(prefix) = var_context(&file.source, offset) {
            let mut names = self.available_properties(&path);
            names.retain(|name| name.starts_with(prefix));
            names.sort();
            names.dedup();
            return names;
        }
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
                    alternatives: Vec::new(),
                    range: rule.modifier_span,
                    selector: rule.selector,
                    standalone,
                });
            let mut required_all = rule.required_all.clone();
            required_all.sort();
            required_all.dedup();
            let mut base_locations: Vec<_> = rule
                .base_spans
                .iter()
                .map(|span| Location {
                    path: path.clone(),
                    span: *span,
                })
                .collect();
            base_locations.sort_by_key(|location| location.span.start);
            base_locations.dedup();
            if !entry
                .alternatives
                .iter()
                .any(|alternative| alternative.required_all == required_all)
            {
                entry.alternatives.push(ModifierRequirement {
                    required_all,
                    base_locations,
                });
            }
        }
        let mut out: Vec<_> = grouped.into_values().collect();
        for item in &mut out {
            item.alternatives
                .sort_by(|a, b| a.required_all.cmp(&b.required_all));
        }
        out.sort_by_key(|d| d.range.start);
        out
    }
    pub fn hover_at(&self, path: &Path, offset: usize) -> Option<HoverInfo> {
        let source_path = canonical_or(path.to_path_buf());
        if let Some((name, span)) = self.property_symbol_at(&source_path, offset) {
            let roles = self.property_roles(&source_path, &name);
            let registration = self
                .files
                .values()
                .flat_map(|f| &f.property_declarations)
                .find(|d| d.name == name && d.registered);
            let mut markdown = format!(
                "**Custom property** `{name}`\n\nRoles: {}",
                roles.join(", ")
            );
            if let Some(d) = registration {
                markdown.push_str(&format!(
                    "\n\nSyntax: `{}`  \nInherits: `{}`  \nInitial value: `{}`",
                    d.syntax.as_deref().unwrap_or("unknown"),
                    d.inherits
                        .map(|v| v.to_string())
                        .as_deref()
                        .unwrap_or("unknown"),
                    d.initial_value.as_deref().unwrap_or("none")
                ));
            }
            return Some(HoverInfo {
                range: span,
                markdown,
            });
        }
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
        let mut alternatives: Vec<_> = rules
            .iter()
            .map(|rule| {
                let mut required = rule.required_all.clone();
                required.sort();
                required.dedup();
                required
            })
            .collect();
        alternatives.sort();
        alternatives.dedup();
        let standalone = module_file
            .exports
            .iter()
            .find(|e| e.name == name)
            .is_some_and(|e| e.independent);
        let requirement = alternatives
            .iter()
            .map(|required| {
                required
                    .iter()
                    .map(|base| format!("`.{base}`"))
                    .collect::<Vec<_>>()
                    .join(" + ")
            })
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
    pub fn selector_preview_at(&self, path: &Path, offset: usize) -> Option<SelectorPreviewInfo> {
        let file = self.files.get(&canonical_or(path.to_path_buf()))?;
        let rule = file
            .selectors
            .iter()
            .find(|rule| rule.source_span.start <= offset && offset <= rule.source_span.end)?;
        let range = Span {
            start: rule.source_span.start,
            end: rule.source_span.end,
        };
        match preview_selector(&rule.resolved) {
            Ok(preview) => Some(SelectorPreviewInfo {
                range,
                preview: Some(preview),
                unsupported: None,
            }),
            Err(reason) => Some(SelectorPreviewInfo {
                range,
                preview: None,
                unsupported: Some(reason),
            }),
        }
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
    pub fn sass_debug_summary(&self, path: &Path) -> Option<String> {
        let file = self.files.get(&canonical_or(path.to_path_buf()))?;
        let resolved = file
            .sass_dependencies
            .iter()
            .filter(|dependency| dependency.source.is_some())
            .count();
        Some(format!(
            "declarations={} references={} dependencies={} resolved_dependencies={resolved}",
            file.sass_declarations.len(),
            file.sass_references.len(),
            file.sass_dependencies.len(),
        ))
    }
    pub fn custom_property_occurrences(&self, path: &Path) -> Vec<CustomPropertyOccurrence> {
        let p = canonical_or(path.to_path_buf());
        let Some(f) = self.files.get(&p) else {
            return vec![];
        };
        let mut out = Vec::new();
        for d in &f.property_declarations {
            out.push(CustomPropertyOccurrence {
                name: d.name.clone(),
                span: d.span,
                declaration: true,
                roles: self.property_roles(&p, &d.name),
            });
        }
        for (name, span) in &f.property_exports {
            out.push(CustomPropertyOccurrence {
                name: name.clone(),
                span: *span,
                declaration: true,
                roles: vec!["exported".into()],
            });
        }
        for import in &f.property_imports {
            for (name, span) in &import.names {
                out.push(CustomPropertyOccurrence {
                    name: name.clone(),
                    span: *span,
                    declaration: true,
                    roles: vec!["imported".into()],
                });
            }
        }
        for r in &f.property_references {
            out.push(CustomPropertyOccurrence {
                name: r.name.clone(),
                span: r.span,
                declaration: false,
                roles: self.property_roles(&p, &r.name),
            });
        }
        out.sort_by_key(|o| o.span.start);
        out
    }
    pub fn custom_property_occurrences_for(
        &self,
        path: &Path,
        mode: &str,
    ) -> Vec<CustomPropertyOccurrence> {
        self.custom_property_occurrences(path)
            .into_iter()
            .filter(|o| {
                o.roles
                    .iter()
                    .any(|r| self.property_presentation.get(r).is_some_and(|m| m == mode))
            })
            .collect()
    }
    pub fn property_declaration_sources(&self, name: &str) -> Vec<PathBuf> {
        let mut out: Vec<_> = self
            .files
            .iter()
            .filter(|(_, f)| {
                f.property_declarations.iter().any(|d| d.name == name)
                    || f.property_exports.iter().any(|(n, _)| n == name)
            })
            .map(|(path, _)| path.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }
    pub fn known_property_names(&self) -> Vec<String> {
        let mut out: Vec<_> = self
            .files
            .values()
            .flat_map(|f| {
                f.property_declarations
                    .iter()
                    .map(|d| d.name.clone())
                    .chain(f.property_exports.iter().map(|(n, _)| n.clone()))
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
    fn sass_span_at(&self, path: &Path, offset: usize) -> Option<Span> {
        let file = self.files.get(path)?;
        file.sass_declarations
            .iter()
            .find(|d| inside(d.span, offset))
            .map(|d| d.span)
            .or_else(|| {
                file.sass_references
                    .iter()
                    .find(|r| inside(r.span, offset))
                    .map(|r| r.span)
            })
            .or_else(|| {
                file.sass_dependencies
                    .iter()
                    .flat_map(|d| d.member_spans.iter())
                    .find(|(_, span)| inside(*span, offset))
                    .map(|(_, span)| *span)
            })
    }
    fn sass_symbol_at(
        &self,
        path: &Path,
        offset: usize,
    ) -> Option<(PathBuf, SassSymbolKind, String)> {
        let file = self.files.get(path)?;
        if let Some(d) = file
            .sass_declarations
            .iter()
            .find(|d| inside(d.span, offset))
        {
            return Some((path.to_path_buf(), d.kind, canonical_sass_name(&d.name)));
        }
        let reference = file.sass_references.iter().find(|r| inside(r.span, offset));
        if let Some(reference) = reference
            && let Some((origin, name)) = self.resolve_sass_reference(path, reference)
        {
            return Some((origin, reference.kind, name));
        }
        for dependency in &file.sass_dependencies {
            let Some(source) = &dependency.source else {
                continue;
            };
            if let Some((member, _)) = dependency
                .member_spans
                .iter()
                .find(|(_, span)| inside(*span, offset))
            {
                let name = canonical_sass_name(member);
                let kinds: &[SassSymbolKind] = if dependency.kind == SassDirectiveKind::Use {
                    &[SassSymbolKind::Variable]
                } else {
                    &[
                        SassSymbolKind::Variable,
                        SassSymbolKind::Mixin,
                        SassSymbolKind::Function,
                    ]
                };
                let mut matches = kinds
                    .iter()
                    .copied()
                    .filter_map(|kind| {
                        self.sass_export_origin(source, kind, &name, &mut HashSet::new())
                            .map(|origin| (origin, kind, name.clone()))
                    })
                    .collect::<Vec<_>>();
                if matches.len() == 1 {
                    return matches.pop();
                }
            }
        }
        None
    }
    fn resolve_sass_reference(
        &self,
        path: &Path,
        reference: &SassReference,
    ) -> Option<(PathBuf, String)> {
        let name = canonical_sass_name(&reference.name);
        let file = self.files.get(path)?;
        if reference.namespace.is_none()
            && file
                .sass_declarations
                .iter()
                .any(|d| d.kind == reference.kind && canonical_sass_name(&d.name) == name)
        {
            return Some((path.to_path_buf(), name));
        }
        let mut matches = Vec::new();
        for dependency in file
            .sass_dependencies
            .iter()
            .filter(|d| d.kind == SassDirectiveKind::Use)
        {
            let visible = match &reference.namespace {
                Some(ns) => dependency.namespace.as_deref() == Some(ns),
                None => dependency.star,
            };
            if !visible {
                continue;
            }
            if let Some(source) = &dependency.source
                && let Some(origin) =
                    self.sass_export_origin(source, reference.kind, &name, &mut HashSet::new())
            {
                matches.push(origin);
            }
        }
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| (matches.remove(0), name))
    }
    fn sass_export_origin(
        &self,
        path: &Path,
        kind: SassSymbolKind,
        requested: &str,
        seen: &mut HashSet<PathBuf>,
    ) -> Option<PathBuf> {
        let path = canonical_or(path.to_path_buf());
        if !seen.insert(path.clone()) {
            return None;
        }
        let file = self.files.get(&path)?;
        if file
            .sass_declarations
            .iter()
            .any(|d| !d.private && d.kind == kind && canonical_sass_name(&d.name) == requested)
        {
            return Some(path);
        }
        let mut origins = Vec::new();
        for dep in file
            .sass_dependencies
            .iter()
            .filter(|d| d.kind == SassDirectiveKind::Forward)
        {
            let mut inner = requested.to_string();
            if let Some(prefix) = &dep.prefix {
                let prefix = canonical_sass_name(prefix);
                if !inner.starts_with(&prefix) {
                    continue;
                }
                inner = inner[prefix.len()..].to_string();
            }
            if !dep.show.is_empty() && !dep.show.iter().any(|n| canonical_sass_name(n) == inner) {
                continue;
            }
            if dep.hide.iter().any(|n| canonical_sass_name(n) == inner) {
                continue;
            }
            if let Some(source) = &dep.source
                && let Some(origin) = self.sass_export_origin(source, kind, &inner, seen)
            {
                origins.push(origin);
            }
        }
        origins.sort();
        origins.dedup();
        (origins.len() == 1).then(|| origins.remove(0))
    }
    fn property_symbol_at(&self, path: &Path, offset: usize) -> Option<(String, Span)> {
        let f = self.files.get(path)?;
        for d in &f.property_declarations {
            if inside(d.span, offset) {
                return Some((d.name.clone(), d.span));
            }
        }
        for r in &f.property_references {
            if inside(r.span, offset) {
                return Some((r.name.clone(), r.span));
            }
        }
        for (n, s) in &f.property_exports {
            if inside(*s, offset) {
                return Some((n.clone(), *s));
            }
        }
        for i in &f.property_imports {
            for (n, s) in &i.names {
                if inside(*s, offset) {
                    return Some((n.clone(), *s));
                }
            }
        }
        None
    }
    fn property_resolved(&self, path: &Path, name: &str) -> bool {
        !self
            .property_roles(path, name)
            .contains(&"unresolved".into())
    }
    fn property_origin(&self, path: &Path, name: &str) -> Option<PathBuf> {
        let f = self.files.get(path)?;
        if f.property_declarations.iter().any(|d| d.name == name)
            || f.property_exports.iter().any(|(n, _)| n == name)
        {
            return Some(path.to_path_buf());
        }
        f.property_imports
            .iter()
            .find(|i| {
                i.names.iter().any(|(n, _)| n == name)
                    && self.files.get(&i.source).is_some_and(|source| {
                        source.property_declarations.iter().any(|d| d.name == name)
                            || source.property_exports.iter().any(|(n, _)| n == name)
                    })
            })
            .map(|i| i.source.clone())
    }
    fn property_roles(&self, path: &Path, name: &str) -> Vec<String> {
        let mut roles = Vec::new();
        let Some(f) = self.files.get(path) else {
            return vec!["unresolved".into()];
        };
        if f.property_declarations
            .iter()
            .any(|d| d.name == name && !d.registered)
        {
            roles.push("local".into());
        }
        if self.files.values().any(|x| {
            x.property_declarations
                .iter()
                .any(|d| d.name == name && d.global)
        }) {
            roles.push("global".into());
        }
        if self.files.values().any(|x| {
            x.property_declarations
                .iter()
                .any(|d| d.name == name && d.registered)
        }) {
            roles.push("registered".into());
        }
        if f.property_imports.iter().any(|i| {
            i.names.iter().any(|(n, _)| n == name)
                && self.files.get(&i.source).is_some_and(|source| {
                    source.property_declarations.iter().any(|d| d.name == name)
                        || source.property_exports.iter().any(|(n, _)| n == name)
                })
        }) {
            roles.push("imported".into());
        }
        if f.property_exports.iter().any(|(n, _)| n == name) {
            roles.push("exported".into());
        }
        if roles.is_empty() {
            roles.push("unresolved".into());
        }
        roles
    }
    fn available_properties(&self, path: &Path) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(f) = self.files.get(path) {
            out.extend(f.property_declarations.iter().map(|d| d.name.clone()));
            out.extend(f.property_exports.iter().map(|(n, _)| n.clone()));
            for i in &f.property_imports {
                out.extend(i.names.iter().map(|(n, _)| n.clone()));
            }
        }
        for f in self.files.values() {
            out.extend(
                f.property_declarations
                    .iter()
                    .filter(|d| d.global || d.registered)
                    .map(|d| d.name.clone()),
            );
        }
        out
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
fn modifier_relationship_key(rule: &ModifierRule) -> (String, Vec<String>) {
    let mut required = rule.required_all.clone();
    required.sort();
    required.dedup();
    (rule.modifier.clone(), required)
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !s.as_bytes()[0].is_ascii_digit()
}
fn canonical_sass_name(s: &str) -> String {
    s.trim_start_matches('$').replace('_', "-")
}
fn valid_sass_name(s: &str) -> bool {
    !s.is_empty()
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || b >= 0x80)
}
fn valid_custom_property_name(s: &str) -> bool {
    s.starts_with("--") && s.len() > 2 && s[2..].bytes().all(ident_member_or_dash)
}
fn ident_member_or_dash(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || b >= 0x80
}
fn selector_is_global(selector: &str) -> bool {
    selector.split(',').any(|branch| branch.trim() == ":root")
}
fn selector_matches_globals(selector: &str, patterns: &[String]) -> bool {
    selector.split(',').any(|branch| {
        let branch = branch.trim();
        patterns.iter().any(|pattern| {
            let pattern = pattern.trim();
            if pattern.starts_with('[') && pattern.ends_with(']') && !pattern.contains('=') {
                let attr = &pattern[1..pattern.len() - 1];
                branch.contains(&format!("[{attr}]"))
                    || branch.contains(&format!("[{attr}="))
                    || branch.contains(&format!("[{attr}~="))
                    || branch.contains(&format!("[{attr}|="))
                    || branch.contains(&format!("[{attr}^="))
                    || branch.contains(&format!("[{attr}$="))
                    || branch.contains(&format!("[{attr}*="))
            } else {
                branch == pattern
            }
        })
    })
}
fn var_context(source: &str, offset: usize) -> Option<&str> {
    if offset > source.len() {
        return None;
    }
    let head = &source[..offset];
    let open = head.rfind("var(")?;
    if head[open + 4..].contains(')') {
        return None;
    }
    let value = head[open + 4..].trim_start();
    if value.contains(',') {
        return None;
    }
    if value.is_empty() {
        return Some("");
    }
    if value.starts_with("--") && value.bytes().all(ident_member_or_dash) {
        Some(value)
    } else {
        None
    }
}
fn sass_completion_context(
    source: &str,
    offset: usize,
) -> Option<(SassSymbolKind, Option<String>, String, Span)> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }
    let bytes = source.as_bytes();
    let mut start = offset;
    while start > 0 && sass_name_byte(bytes[start - 1]) {
        start -= 1;
    }
    let prefix = source[start..offset].to_string();
    if start > 0 && bytes[start - 1] == b'$' {
        let namespace = if start > 1 && bytes[start - 2] == b'.' {
            let mut ns_start = start - 2;
            while ns_start > 0 && sass_name_byte(bytes[ns_start - 1]) {
                ns_start -= 1;
            }
            Some(source[ns_start..start - 2].into())
        } else {
            None
        };
        return Some((
            SassSymbolKind::Variable,
            namespace,
            prefix,
            Span { start, end: offset },
        ));
    }
    let line_start = source[..start].rfind(['\n', '\r']).map_or(0, |p| p + 1);
    let head = source[line_start..start].trim_start();
    if let Some(rest) = head.strip_prefix("@include") {
        let namespace = rest
            .trim()
            .strip_suffix('.')
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        return Some((
            SassSymbolKind::Mixin,
            namespace,
            prefix,
            Span { start, end: offset },
        ));
    }
    let namespace = if start > 0 && bytes[start - 1] == b'.' {
        let mut ns_start = start - 1;
        while ns_start > 0 && sass_name_byte(bytes[ns_start - 1]) {
            ns_start -= 1;
        }
        Some(source[ns_start..start - 1].into())
    } else {
        None
    };
    let before = source[..start].trim_end();
    if namespace.is_some()
        || before.ends_with([':', '(', ',', '=', '+', '-', '*', '/'])
        || before.rsplit_once(':').is_some()
    {
        return Some((
            SassSymbolKind::Function,
            namespace,
            prefix,
            Span { start, end: offset },
        ));
    }
    None
}
fn sass_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || b >= 0x80
}
fn sass_kind_name(kind: SassSymbolKind) -> &'static str {
    match kind {
        SassSymbolKind::Variable => "Sass variable",
        SassSymbolKind::Mixin => "Sass mixin",
        SassSymbolKind::Function => "Sass function",
    }
}
fn newline(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}
fn sass_import_insertion_offset(source: &str) -> usize {
    let facts = parse_stylesheet(source);
    let mut end = facts
        .sass_directives
        .iter()
        .map(|d| d.statement_span.end)
        .max()
        .unwrap_or(0);
    if end == 0 && source.trim_start().starts_with("@charset") {
        let leading = source.len() - source.trim_start().len();
        end = source[leading..].find(';').map_or(0, |p| leading + p + 1);
    }
    while end < source.len() && matches!(source.as_bytes()[end], b'\r' | b'\n') {
        end += 1;
    }
    end
}
fn property_prefix(source: &str, offset: usize) -> &str {
    if offset > source.len() {
        return "";
    }
    let bytes = source.as_bytes();
    let mut start = offset;
    while start > 0 && ident_member_or_dash(bytes[start - 1]) {
        start -= 1;
    }
    &source[start..offset]
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
    s.ends_with(".css") || s.ends_with(".scss")
}
fn module_stylesheet(p: &Path) -> bool {
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
    fn custom_property_ownership_imports_and_suppression() {
        let d = tempdir().unwrap();
        let theme = d.path().join("theme.scss");
        let card = d.path().join("card.scss");
        fs::write(
            &theme,
            ":root { --global: red; } .theme { --local: blue; } /* @export-props: --runtime */",
        )
        .unwrap();
        fs::write(&card,"/* @import-props \"./theme.scss\": --local, --runtime, --unused */\n.card { --owned: 1; color: var(--owned); background: var(--global); border: var(--local); x: var(--runtime); y: var(--missing, red); }\n/* @suppress-unresolved-prop */\nz: var(--suppressed);").unwrap();
        let mut p = Project::new(vec![d.path().to_path_buf()]);
        p.index_workspace();
        let diagnostics = p.diagnostics_for(&card);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "unresolved-custom-property" && d.message.contains("--missing"))
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("--suppressed"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "missing-imported-property" && d.message.contains("--unused"))
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == "unresolved-custom-property" && d.message.contains("--local"))
        );
    }
    #[test]
    fn local_property_rename_does_not_touch_unrelated_modules() {
        let d = tempdir().unwrap();
        let one = d.path().join("one.scss");
        let two = d.path().join("two.scss");
        fs::write(&one, ".one { --shared: red; color: var(--shared); }").unwrap();
        fs::write(&two, ".two { --shared: blue; color: var(--shared); }").unwrap();
        let mut p = Project::new(vec![d.path().to_path_buf()]);
        p.index_workspace();
        let offset = p.source(&one).unwrap().find("--shared").unwrap();
        let edits = p.rename(&one, offset, "--renamed").unwrap();
        assert_eq!(edits.len(), 2);
        assert!(
            edits
                .iter()
                .all(|edit| edit.location.path == canonical_or(one.clone()))
        );
    }
    #[test]
    fn configured_attribute_presence_selector_is_global() {
        let d = tempdir().unwrap();
        let theme = d.path().join("theme.css");
        let use_file = d.path().join("use.css");
        fs::write(&theme, "[data-theme=\"dark\"] { --surface: black; }").unwrap();
        fs::write(&use_file, ".card { color: var(--surface); }").unwrap();
        let mut p = Project::new(vec![d.path().to_path_buf()]);
        p.index_workspace();
        assert!(
            p.diagnostics_for(&use_file)
                .iter()
                .any(|d| d.code == "unresolved-custom-property")
        );
        p.set_global_selectors(vec!["[data-theme]".into()]);
        assert!(
            !p.diagnostics_for(&use_file)
                .iter()
                .any(|d| d.code == "unresolved-custom-property")
        );
    }
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

    #[test]
    fn nested_modifier_uses_full_chain_for_navigation_and_hover() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        fs::write(&css, ".gradientWrapper { &.offset { &.narrow {} } }").unwrap();
        fs::write(
            &ts,
            "import s from './x.module.scss'; <i className={clsx(s.gradientWrapper, s.offset, s.narrow)} />",
        )
        .unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let source = p.source(&ts).unwrap();
        let at = source.rfind("narrow").unwrap();
        let definitions = p.definitions_at(&ts, at);
        assert_eq!(definitions.len(), 1);
        assert!(p.diagnostics_for(&ts).is_empty());
        let hover = p.hover_at(&ts, at).unwrap();
        assert!(hover.markdown.contains("`.gradientWrapper` + `.offset`"));
        let decorations = p.modifier_decorations(&css);
        let narrow = decorations
            .iter()
            .find(|decoration| decoration.modifier == "narrow")
            .unwrap();
        assert_eq!(narrow.alternatives.len(), 1);
        assert_eq!(
            narrow.alternatives[0].required_all,
            ["gradientWrapper", "offset"]
        );

        p.open_or_update_file(
            ts.clone(),
            "import s from './x.module.scss'; <i className={clsx(s.gradientWrapper, s.narrow)} />"
                .into(),
            Some(2),
        );
        assert!(p.diagnostics_for(&ts).iter().any(|diagnostic| {
            diagnostic.code == "dependent-modifier-without-base"
                && diagnostic.message.contains("gradientWrapper + offset")
        }));
    }

    #[test]
    fn selector_preview_is_available_across_nested_selector_span() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let source = ".searchWrapper { &:has(+ .popup:popover-open) .arrow {} }";
        fs::write(&css, source).unwrap();
        let mut project = Project::new(vec![d.path().into()]);
        project.index_workspace();
        let at = source.find("popover-open").unwrap();
        let info = project.selector_preview_at(&css, at).unwrap();
        let preview = info.preview.unwrap();
        assert_eq!(
            preview.resolved_selector,
            ".searchWrapper:has(+ .popup:popover-open) .arrow"
        );
        assert!(
            preview.nodes[preview.subject]
                .classes
                .contains(&"arrow".into())
        );
    }

    #[test]
    fn unused_modifier_diagnostics_are_grouped_by_exact_relationship() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        let css_source =
            ".first { &.active {} } .first { &.active {} } .second { &.active {} } .active {}";
        fs::write(&css, css_source).unwrap();
        fs::write(
            &ts,
            "import s from './x.module.scss'; <i className={clsx(s.first, s.active)} />",
        )
        .unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let unused_active: Vec<_> = p
            .diagnostics_for(&css)
            .into_iter()
            .filter(|diagnostic| {
                diagnostic.code == "unused-export"
                    && &css_source[diagnostic.location.span.start..diagnostic.location.span.end]
                        == "active"
            })
            .collect();
        assert_eq!(unused_active.len(), 1);
        assert!(unused_active[0].location.span.start > css_source.find(".second").unwrap());
    }

    #[test]
    fn uncertain_modifier_composition_suppresses_relationship_unused_hints() {
        let d = tempdir().unwrap();
        let css = d.path().join("x.module.scss");
        let ts = d.path().join("x.tsx");
        fs::write(&css, ".first { &.active {} } .second { &.active {} }").unwrap();
        fs::write(
            &ts,
            "import s from './x.module.scss'; <i className={unknownHelper(s.active)} />",
        )
        .unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let css_source = p.source(&css).unwrap();
        assert!(p.diagnostics_for(&css).iter().all(|diagnostic| {
            diagnostic.code != "unused-export"
                || &css_source[diagnostic.location.span.start..diagnostic.location.span.end]
                    != "active"
        }));
    }
    #[test]
    fn sass_graph_navigation_rename_completion_and_import_fixing() {
        let d = tempdir().unwrap();
        let tokens = d.path().join("src/styles/_tokens.scss");
        let barrel = d.path().join("src/styles/index.scss");
        let card = d.path().join("src/card.scss");
        fs::create_dir_all(tokens.parent().unwrap()).unwrap();
        fs::write(
            &tokens,
            "$space_value: 1rem;\n@mixin paint {}\n@function scale($v) { @return $v; }",
        )
        .unwrap();
        fs::write(&barrel, "@forward \"./tokens\";").unwrap();
        let source = "@use \"./styles/index.scss\" as *;\n.x { gap: $space-value; @include paint; width: scale(2); }";
        fs::write(&card, source).unwrap();
        let mut p = Project::new(vec![d.path().into()]);
        p.index_workspace();
        let use_offset = source.find("space-value").unwrap();
        assert_eq!(
            p.definition_at(&card, use_offset).unwrap().path,
            tokens.canonicalize().unwrap()
        );
        let declaration_offset = p.source(&tokens).unwrap().find("space_value").unwrap();
        assert_eq!(p.references_at(&tokens, declaration_offset, false).len(), 1);
        assert_eq!(
            p.rename(&tokens, declaration_offset, "spacing")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            p.fix_sass_imports(&card)[0].new_text,
            "src/styles/index.scss"
        );

        let fresh = d.path().join("src/fresh.scss");
        fs::write(&fresh, ".x { gap: $spa }").unwrap();
        p.open_or_update_file(fresh.clone(), fs::read_to_string(&fresh).unwrap(), Some(1));
        let completion_offset = p.source(&fresh).unwrap().find("spa }").unwrap() + 3;
        let item = p
            .completion_items_at(&fresh, completion_offset)
            .into_iter()
            .find(|i| i.label == "space_value")
            .unwrap();
        assert_eq!(item.additional_edits.len(), 1);
        assert!(
            item.additional_edits[0]
                .new_text
                .contains("@use \"src/styles/tokens.scss\" as *;")
        );
    }
}
