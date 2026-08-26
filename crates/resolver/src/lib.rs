use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("no matching TypeScript path alias was found: {0}")]
    UnmappedAlias(String),
    #[error("CSS Module not found: {0}")]
    NotFound(String),
}

pub trait Resolver: Send + Sync {
    fn resolve_stylesheet(&self, importer: &Path, specifier: &str)
    -> Result<PathBuf, ResolveError>;
}

#[derive(Default)]
pub struct FileSystemResolver {
    configs: RwLock<HashMap<PathBuf, Option<AliasConfig>>>,
}

#[derive(Clone, Debug)]
struct AliasConfig {
    directory: PathBuf,
    base_url: Option<PathBuf>,
    paths: HashMap<String, Vec<String>>,
}

#[derive(Deserialize, Default)]
struct RawConfig {
    #[serde(rename = "compilerOptions", default)]
    compiler_options: RawCompilerOptions,
}

#[derive(Deserialize, Default)]
struct RawCompilerOptions {
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    paths: HashMap<String, Vec<String>>,
}

impl Resolver for FileSystemResolver {
    fn resolve_stylesheet(
        &self,
        importer: &Path,
        specifier: &str,
    ) -> Result<PathBuf, ResolveError> {
        if specifier.starts_with('.') {
            return resolve_candidate(
                &importer
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(specifier),
                specifier,
            );
        }
        let directory = importer.parent().unwrap_or_else(|| Path::new("."));
        let Some(config) = self.config_for(directory) else {
            return Err(ResolveError::UnmappedAlias(specifier.into()));
        };
        let base = config
            .base_url
            .as_deref()
            .unwrap_or(config.directory.as_path());
        let mut mappings: Vec<_> = config.paths.iter().collect();
        mappings.sort_by_key(|(pattern, _)| std::cmp::Reverse(pattern.len()));
        for (pattern, replacements) in mappings {
            let Some(capture) = match_pattern(pattern, specifier) else {
                continue;
            };
            for replacement in replacements {
                let relative = if replacement.contains('*') {
                    replacement.replacen('*', capture, 1)
                } else {
                    replacement.clone()
                };
                if let Ok(path) = resolve_candidate(&base.join(relative), specifier) {
                    return Ok(path);
                }
            }
        }
        Err(ResolveError::UnmappedAlias(specifier.into()))
    }
}

impl FileSystemResolver {
    fn config_for(&self, directory: &Path) -> Option<AliasConfig> {
        let key = directory.to_path_buf();
        if let Some(cached) = self.configs.read().ok()?.get(&key) {
            return cached.clone();
        }
        let found = find_config(directory).and_then(|path| load_config(&path));
        if let Ok(mut cache) = self.configs.write() {
            cache.insert(key, found.clone());
        }
        found
    }
}

fn find_config(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        for name in ["tsconfig.json", "jsconfig.json"] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn load_config(path: &Path) -> Option<AliasConfig> {
    let source = fs::read_to_string(path).ok()?;
    let raw: RawConfig = serde_json::from_str(&sanitize_jsonc(&source)).ok()?;
    let directory = path.parent()?.to_path_buf();
    let base_url = raw.compiler_options.base_url.map(|p| directory.join(p));
    Some(AliasConfig {
        directory,
        base_url,
        paths: raw.compiler_options.paths,
    })
}

fn match_pattern<'a>(pattern: &str, specifier: &'a str) -> Option<&'a str> {
    let Some(star) = pattern.find('*') else {
        return (pattern == specifier).then_some("");
    };
    let (prefix, suffix_with_star) = pattern.split_at(star);
    let suffix = &suffix_with_star[1..];
    if !specifier.starts_with(prefix) || !specifier.ends_with(suffix) {
        return None;
    }
    let end = specifier.len().checked_sub(suffix.len())?;
    (end >= prefix.len()).then(|| &specifier[prefix.len()..end])
}

fn resolve_candidate(base: &Path, display: &str) -> Result<PathBuf, ResolveError> {
    let mut candidates = vec![base.to_path_buf()];
    if base.extension().is_none() {
        candidates.push(base.with_extension("module.css"));
        candidates.push(base.with_extension("module.scss"));
    }
    for candidate in candidates {
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|_| ResolveError::NotFound(display.into()));
        }
    }
    Err(ResolveError::NotFound(display.into()))
}

/// Removes JSONC comments and trailing commas while preserving quoted strings.
fn sanitize_jsonc(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(source.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        if in_string {
            out.push(bytes[i]);
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 1;
                out.push(bytes[i]);
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            out.push(b'"');
            i += 1;
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == b',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && matches!(bytes[j], b'}' | b']') {
                i += 1;
            } else {
                out.push(b',');
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn resolves_tsconfig_wildcard_alias_with_jsonc() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("styles")).unwrap();
        let importer = src.join("component.tsx");
        let stylesheet = src.join("styles/myStyles.module.scss");
        fs::write(&importer, "").unwrap();
        fs::write(&stylesheet, ".root {}").unwrap();
        fs::write(
            dir.path().join("tsconfig.json"),
            r#"{
          // Vite uses this mapping too.
          "compilerOptions": { "paths": { "@/*": ["./src/*"], }, }
        }"#,
        )
        .unwrap();
        let resolved = FileSystemResolver::default()
            .resolve_stylesheet(&importer, "@/styles/myStyles.module.scss")
            .unwrap();
        assert_eq!(resolved, stylesheet.canonicalize().unwrap());
    }
}
