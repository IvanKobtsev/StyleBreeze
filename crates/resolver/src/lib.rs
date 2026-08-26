use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("only relative CSS Module imports are supported in the MVP: {0}")]
    NonRelative(String),
    #[error("CSS Module not found: {0}")]
    NotFound(String),
}

pub trait Resolver: Send + Sync {
    fn resolve_stylesheet(&self, importer: &Path, specifier: &str)
    -> Result<PathBuf, ResolveError>;
}

#[derive(Default)]
pub struct FileSystemResolver;
impl Resolver for FileSystemResolver {
    fn resolve_stylesheet(
        &self,
        importer: &Path,
        specifier: &str,
    ) -> Result<PathBuf, ResolveError> {
        if !specifier.starts_with('.') {
            return Err(ResolveError::NonRelative(specifier.into()));
        }
        let base = importer
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(specifier);
        let mut candidates = vec![base.clone()];
        if base.extension().is_none() {
            candidates.push(base.with_extension("module.css"));
            candidates.push(base.with_extension("module.scss"));
        }
        for candidate in candidates {
            if candidate.is_file() {
                return candidate
                    .canonicalize()
                    .map_err(|_| ResolveError::NotFound(specifier.into()));
            }
        }
        Err(ResolveError::NotFound(specifier.into()))
    }
}
