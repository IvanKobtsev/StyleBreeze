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
    Information,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub location: Location,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
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
}
#[derive(Clone, Debug)]
struct Reference {
    module: PathBuf,
    name: String,
    span: Span,
}
#[derive(Clone, Debug)]
struct FileRecord {
    source: String,
    version: Option<i32>,
    exports: Vec<Export>,
    references: Vec<Reference>,
    diagnostics: Vec<Diagnostic>,
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
            resolver: Box::new(FileSystemResolver),
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
        let mut diagnostics = Vec::new();
        if stylesheet(&path) {
            let facts = parse_stylesheet(&source);
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
                .map(|(name, occurrences)| Export { name, occurrences })
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
                });
            }
        } else {
            let facts = parse_typescript(&path, &source);
            let imports: HashMap<_, _> = facts
                .imports
                .iter()
                .filter_map(|i| {
                    self.resolver
                        .resolve_stylesheet(&path, &i.specifier)
                        .ok()
                        .map(|p| (i.binding.clone(), p))
                })
                .collect();
            let mut dynamic = HashSet::new();
            for a in facts.accesses {
                if let Some(module) = imports.get(&a.binding) {
                    if let Some(name) = a.class_name {
                        references.push(Reference {
                            module: module.clone(),
                            name,
                            span: Span {
                                start: a.span.start,
                                end: a.span.end,
                            },
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
                        });
                    }
                }
            }
        }
        self.files.insert(
            path,
            FileRecord {
                source,
                version,
                exports,
                references,
                diagnostics,
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
        let (module, name) = self.symbol_at(path, offset)?;
        let f = self.files.get(&module)?;
        let e = f.exports.iter().find(|e| e.name == name)?;
        Some(Location {
            path: module,
            span: *e.occurrences.first()?,
        })
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
                    .filter(|r| r.module == module && r.name == name)
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
                        severity: Severity::Error,
                        code: "unknown-export",
                        message: format!("CSS Module has no export named '{}'", r.name),
                    });
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
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !s.as_bytes()[0].is_ascii_digit()
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
        assert_eq!(p.rename(&ts, at, "renamed").unwrap().len(), 2);
    }
}
