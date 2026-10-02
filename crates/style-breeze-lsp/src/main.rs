use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use lsp_server::{Connection, Message, Notification};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DiagnosticTag, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, InitializeParams, NumberOrString, Position,
    PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, Url,
};
use serde::Serialize;
use style_contract::{
    WorkspaceIndex,
    diagnostic::{Diagnostic, Severity, UnusedSymbol, UnusedSymbolKind},
    workspace::RefreshOutcome,
};

fn main() -> Result<()> {
    let (config, stdio) = arguments()?;
    if !stdio {
        bail!("StyleBreeze currently supports only --stdio transport");
    }
    run_stdio(config)
}

fn arguments() -> Result<(PathBuf, bool)> {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut stdio = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "lsp" => {}
            "--stdio" => stdio = true,
            "--config" => config = args.next().map(PathBuf::from),
            "--version" | "-V" => {
                println!("style-breeze {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    Ok((config.context("--config <PATH> is required")?, stdio))
}

fn run_stdio(config_path: PathBuf) -> Result<()> {
    let (connection, threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::SaveOptions(
                    lsp_types::SaveOptions {
                        include_text: Some(true),
                    },
                )),
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    let params: InitializeParams =
        serde_json::from_value(connection.initialize(serde_json::to_value(capabilities)?)?)?;
    let root = workspace_root(&params, &config_path);
    let mut server = Server::load(root, config_path, &connection);
    server.event_loop(&connection)?;
    threads.join()?;
    Ok(())
}

struct OpenDocument {
    version: i32,
    text: String,
}

struct Server {
    root: PathBuf,
    config_path: PathBuf,
    index: Option<WorkspaceIndex>,
    open: HashMap<Url, OpenDocument>,
    dirty_documents: HashSet<Url>,
    invalid_documents: HashSet<Url>,
    published: HashSet<Url>,
}

impl Server {
    fn load(root: PathBuf, config_path: PathBuf, connection: &Connection) -> Self {
        let mut server = Self {
            root,
            config_path,
            index: None,
            open: HashMap::new(),
            dirty_documents: HashSet::new(),
            invalid_documents: HashSet::new(),
            published: HashSet::new(),
        };
        server.reload(connection);
        server
    }

    fn reload(&mut self, connection: &Connection) {
        match WorkspaceIndex::load(self.root.clone(), self.config_path.clone()) {
            Ok(mut index) => {
                let mut invalid_documents = HashSet::new();
                for (uri, document) in &self.open {
                    if let Ok(path) = uri.to_file_path()
                        && index.update_document(path, document.text.clone()).is_err()
                    {
                        invalid_documents.insert(uri.clone());
                    }
                }
                self.index = Some(index);
                self.dirty_documents.clear();
                self.invalid_documents = invalid_documents;
                self.issue(connection, false, "clear", "", None);
                self.publish_open(connection);
            }
            Err(error) => {
                self.index = None;
                self.clear_all(connection);
                let code = if self.config_path.exists() {
                    "config_invalid"
                } else {
                    "config_missing"
                };
                self.issue(
                    connection,
                    true,
                    code,
                    &format!("{error:#}"),
                    Some(&self.config_path),
                );
                if self.config_path.exists() {
                    self.publish_config_error(connection, &format!("{error:#}"));
                }
            }
        }
    }

    fn event_loop(&mut self, connection: &Connection) -> Result<()> {
        let mut pending = VecDeque::new();
        loop {
            let message = match pending.pop_front() {
                Some(message) => message,
                None => match connection.receiver.recv() {
                    Ok(message) => message,
                    Err(_) => return Ok(()),
                },
            };
            match message {
                Message::Request(request) if connection.handle_shutdown(&request)? => {
                    self.clear_all(connection);
                    return Ok(());
                }
                Message::Request(request) => {
                    connection
                        .sender
                        .send(Message::Response(lsp_server::Response::new_err(
                            request.id,
                            lsp_server::ErrorCode::MethodNotFound as i32,
                            "StyleBreeze exposes diagnostics only".into(),
                        )))?
                }
                Message::Notification(notification) => {
                    let mut publish = self.notification(connection, notification);
                    if publish {
                        while let Ok(message) = connection.receiver.try_recv() {
                            match message {
                                Message::Notification(notification) => {
                                    publish |= self.notification(connection, notification);
                                }
                                other => {
                                    pending.push_back(other);
                                    break;
                                }
                            }
                        }
                        if publish {
                            self.publish_open(connection);
                        }
                    }
                }
                Message::Response(_) => {}
            }
        }
    }

    fn notification(&mut self, connection: &Connection, notification: Notification) -> bool {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                if let Ok(params) =
                    serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
                {
                    let uri = params.text_document.uri;
                    let document = OpenDocument {
                        version: params.text_document.version,
                        text: params.text_document.text,
                    };
                    self.open.insert(uri.clone(), document);
                    self.dirty_documents.insert(uri);
                    return true;
                }
            }
            "textDocument/didChange" => {
                if let Ok(params) =
                    serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
                    && let Some(document) = self.open.get_mut(&params.text_document.uri)
                {
                    for change in params.content_changes {
                        apply_change(&mut document.text, change.range, &change.text);
                    }
                    document.version = params.text_document.version;
                    self.dirty_documents.insert(params.text_document.uri);
                    return true;
                }
            }
            "textDocument/didSave" => {
                let _ = serde_json::from_value::<DidSaveTextDocumentParams>(notification.params);
            }
            "textDocument/didClose" => {
                if let Ok(params) =
                    serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
                {
                    let close_result = self.index.as_mut().and_then(|index| {
                        params
                            .text_document
                            .uri
                            .to_file_path()
                            .ok()
                            .map(|path| index.close_document(&path))
                    });
                    if let Some(Err(error)) = close_result {
                        self.issue(
                            connection,
                            true,
                            "analysis_failed",
                            &format!("{error:#}"),
                            None,
                        );
                    }
                    self.dirty_documents.remove(&params.text_document.uri);
                    self.invalid_documents.remove(&params.text_document.uri);
                    self.open.remove(&params.text_document.uri);
                    self.publish(connection, params.text_document.uri, None, vec![]);
                    return true;
                }
            }
            "workspace/didChangeConfiguration" => self.reload(connection),
            "workspace/didChangeWatchedFiles" => {
                if let Ok(params) =
                    serde_json::from_value::<DidChangeWatchedFilesParams>(notification.params)
                {
                    let mut updated = false;
                    let mut reload = false;
                    let mut failure = None;
                    if let Some(index) = &mut self.index {
                        for change in params.changes {
                            let Ok(path) = change.uri.to_file_path() else {
                                continue;
                            };
                            match index.refresh_path(&path) {
                                Ok(RefreshOutcome::Updated) => updated = true,
                                Ok(RefreshOutcome::FullReloadRequired) => reload = true,
                                Ok(RefreshOutcome::Unchanged) => {}
                                Err(error) => failure = Some(error),
                            }
                        }
                    }
                    if reload {
                        self.reload(connection);
                        return false;
                    }
                    if let Some(error) = failure {
                        self.issue(
                            connection,
                            true,
                            "analysis_failed",
                            &format!("{error:#}"),
                            None,
                        );
                    }
                    return updated;
                }
            }
            _ => {}
        }
        false
    }

    fn publish_open(&mut self, connection: &Connection) {
        let dirty: Vec<_> = self.dirty_documents.drain().collect();
        if let Some(index) = &mut self.index {
            for uri in dirty {
                let (Ok(path), Some(document)) = (uri.to_file_path(), self.open.get(&uri)) else {
                    continue;
                };
                if index.update_document(path, document.text.clone()).is_err() {
                    self.invalid_documents.insert(uri);
                } else {
                    self.invalid_documents.remove(&uri);
                }
            }
        }
        let Some(index) = &mut self.index else {
            return;
        };
        let result = match index.diagnostics() {
            Ok(result) => result,
            Err(error) => {
                self.issue(
                    connection,
                    true,
                    "analysis_failed",
                    &format!("{error:#}"),
                    None,
                );
                return;
            }
        };
        self.issue(connection, false, "clear", "", None);
        let uris: Vec<_> = self.open.keys().cloned().collect();
        for uri in uris {
            let Ok(path) = uri.to_file_path() else {
                continue;
            };
            let Some(document) = self.open.get(&uri) else {
                continue;
            };
            if self.invalid_documents.contains(&uri) {
                self.publish(connection, uri, Some(document.version), vec![]);
                continue;
            }
            let mut diagnostics: Vec<_> = result
                .diagnostics
                .iter()
                .filter(|diagnostic| same_path(&diagnostic.location.path, &path))
                .map(|diagnostic| to_lsp(diagnostic, &document.text))
                .collect();
            diagnostics.extend(
                result
                    .unused_symbols
                    .iter()
                    .filter(|symbol| same_path(&symbol.location.path, &path))
                    .filter_map(|symbol| unused_symbol_diagnostic(symbol, &document.text)),
            );
            self.publish(connection, uri, Some(document.version), diagnostics);
        }
    }

    fn publish_config_error(&mut self, connection: &Connection, message: &str) {
        if let Ok(uri) = Url::from_file_path(&self.config_path) {
            self.publish(
                connection,
                uri,
                None,
                vec![LspDiagnostic {
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("StyleContract".into()),
                    code: Some(NumberOrString::String("configuration".into())),
                    message: message.into(),
                    ..Default::default()
                }],
            );
        }
    }

    fn publish(
        &mut self,
        connection: &Connection,
        uri: Url,
        version: Option<i32>,
        diagnostics: Vec<LspDiagnostic>,
    ) {
        let params = PublishDiagnosticsParams::new(uri.clone(), diagnostics, version);
        let _ = connection
            .sender
            .send(Message::Notification(Notification::new(
                "textDocument/publishDiagnostics".into(),
                params,
            )));
        self.published.insert(uri);
    }

    fn clear_all(&mut self, connection: &Connection) {
        let uris: Vec<_> = self.published.drain().collect();
        for uri in uris {
            let params = PublishDiagnosticsParams::new(uri, vec![], None);
            let _ = connection
                .sender
                .send(Message::Notification(Notification::new(
                    "textDocument/publishDiagnostics".into(),
                    params,
                )));
        }
    }

    fn issue(
        &self,
        connection: &Connection,
        active: bool,
        code: &str,
        message: &str,
        path: Option<&Path>,
    ) {
        let issue = WorkspaceIssue {
            version: 1,
            active,
            code,
            message,
            path: path.map(|value| value.to_string_lossy().as_ref().to_owned()),
        };
        let _ = connection
            .sender
            .send(Message::Notification(Notification::new(
                "styleBreeze/workspaceIssue".into(),
                issue,
            )));
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceIssue<'a> {
    version: u8,
    active: bool,
    code: &'a str,
    message: &'a str,
    path: Option<String>,
}

fn unused_symbol_range(symbol: &UnusedSymbol, text: &str) -> Option<Range> {
    let line_number = symbol.location.line.checked_sub(1)?;
    let line_start = text
        .split_inclusive('\n')
        .take(line_number)
        .map(str::len)
        .sum::<usize>();
    let line = text[line_start..]
        .split_once('\n')
        .map_or(&text[line_start..], |(line, _)| {
            line.strip_suffix('\r').unwrap_or(line)
        });
    let name_column = symbol.location.column.checked_sub(1)?.min(line.len());
    let name_start = line_start + name_column;
    if !text.is_char_boundary(name_start) {
        return None;
    }
    let start = if symbol.kind == UnusedSymbolKind::Class
        && name_start > 0
        && text.as_bytes().get(name_start - 1) == Some(&b'.')
    {
        name_start - 1
    } else {
        name_start
    };
    let mut end = (name_start + symbol.name.len()).min(text.len());
    if symbol.kind == UnusedSymbolKind::Class {
        let mut last_selector_byte = end;
        for character in text[end..].chars() {
            if matches!(character, ',' | ')' | '{') {
                break;
            }
            end += character.len_utf8();
            if !character.is_whitespace() {
                last_selector_byte = end;
            }
        }
        end = last_selector_byte;
    }
    Some(Range::new(
        position_at_byte(text, start),
        position_at_byte(text, end),
    ))
}

fn position_at_byte(text: &str, byte: usize) -> Position {
    let byte = byte.min(text.len());
    let prefix = &text[..byte];
    let line = prefix.bytes().filter(|value| *value == b'\n').count() as u32;
    let column_text = prefix.rsplit_once('\n').map_or(prefix, |(_, tail)| tail);
    Position::new(line, column_text.encode_utf16().count() as u32)
}

fn unused_symbol_diagnostic(symbol: &UnusedSymbol, text: &str) -> Option<LspDiagnostic> {
    let (rule, label) = match symbol.kind {
        UnusedSymbolKind::Class => ("unused-class", "class"),
        UnusedSymbolKind::Export => ("unused-export", ":export key"),
    };
    Some(LspDiagnostic {
        range: unused_symbol_range(symbol, text)?,
        severity: Some(DiagnosticSeverity::HINT),
        code: Some(NumberOrString::String(rule.into())),
        source: Some("StyleContract".into()),
        message: format!("Unused {label} '{}'", symbol.name),
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        ..Default::default()
    })
}

fn to_lsp(diagnostic: &Diagnostic, text: &str) -> LspDiagnostic {
    let line = diagnostic.location.line.saturating_sub(1) as u32;
    let byte_column = diagnostic.location.column.saturating_sub(1);
    let (start_character, end_character) = text
        .lines()
        .nth(line as usize)
        .map(|line| diagnostic_columns_for_rule(line, byte_column, diagnostic.rule))
        .unwrap_or((0, 0));
    let special_range = match diagnostic.rule {
        "empty-rule" => empty_rule_range(diagnostic, text),
        "module-to-module-import" => stylesheet_import_range(diagnostic, text),
        _ => None,
    };
    let range = special_range.unwrap_or_else(|| {
        Range::new(
            Position::new(line, start_character),
            Position::new(line, end_character),
        )
    });
    LspDiagnostic {
        range,
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Off => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(diagnostic.rule.into())),
        source: Some("StyleContract".into()),
        message: diagnostic.message.clone(),
        tags: diagnostic_tags(diagnostic.rule),
        ..Default::default()
    }
}

fn stylesheet_import_range(diagnostic: &Diagnostic, text: &str) -> Option<Range> {
    let line_number = diagnostic.location.line.checked_sub(1)?;
    let line_start = text
        .split_inclusive('\n')
        .take(line_number)
        .map(str::len)
        .sum::<usize>();
    let start = line_start + diagnostic.location.column.checked_sub(1)?;
    if start >= text.len() || !text.is_char_boundary(start) {
        return None;
    }
    let end = text[start..]
        .char_indices()
        .find(|(_, character)| matches!(character, '\'' | '"' | '\r' | '\n'))
        .map_or(text.len(), |(offset, _)| start + offset);
    (end > start).then(|| Range::new(position_at_byte(text, start), position_at_byte(text, end)))
}

fn empty_rule_range(diagnostic: &Diagnostic, text: &str) -> Option<Range> {
    let line_number = diagnostic.location.line.checked_sub(1)?;
    let line_start = text
        .split_inclusive('\n')
        .take(line_number)
        .map(str::len)
        .sum::<usize>();
    let start = line_start + diagnostic.location.column.checked_sub(1)?;
    if start >= text.len() || !text.is_char_boundary(start) {
        return None;
    }
    let open = find_rule_open(text, start)?;
    let selector = &text[start..open];
    let end = open - (selector.len() - selector.trim_end().len());
    (end > start).then(|| Range::new(position_at_byte(text, start), position_at_byte(text, end)))
}

fn find_rule_open(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = start;
    let mut quote = None;
    let mut interpolation_depth = 0usize;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if bytes[index] == delimiter {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"') {
            quote = Some(bytes[index]);
            index += 1;
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'/' {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'#' && bytes[index + 1] == b'{' {
            interpolation_depth += 1;
            index += 2;
            continue;
        }
        if interpolation_depth > 0 {
            match bytes[index] {
                b'{' => interpolation_depth += 1,
                b'}' => interpolation_depth -= 1,
                _ => {}
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'{' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn diagnostic_tags(rule: &str) -> Option<Vec<DiagnosticTag>> {
    matches!(rule, "unused-class" | "unused-export").then(|| vec![DiagnosticTag::UNNECESSARY])
}

fn utf16_column(line: &str, byte_column: usize) -> u32 {
    let boundary = byte_column.min(line.len());
    line[..boundary].encode_utf16().count() as u32
}

fn diagnostic_columns(line: &str, byte_column: usize) -> (u32, u32) {
    let boundary = byte_column.min(line.len());
    let start = utf16_column(line, boundary);
    let length = line[boundary..]
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '$')
        })
        .map(char::len_utf8)
        .sum::<usize>();
    let length = if length == 0 {
        line[boundary..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0)
    } else {
        length
    };
    (start, utf16_column(line, boundary + length))
}

fn diagnostic_columns_for_rule(line: &str, byte_column: usize, rule: &str) -> (u32, u32) {
    let (start, end) = diagnostic_columns(line, byte_column);
    if rule == "unused-class"
        && byte_column > 0
        && line.as_bytes().get(byte_column - 1) == Some(&b'.')
    {
        (utf16_column(line, byte_column - 1), end)
    } else {
        (start, end)
    }
}

fn apply_change(text: &mut String, range: Option<Range>, replacement: &str) {
    let Some(range) = range else {
        replacement.clone_into(text);
        return;
    };
    if let (Some(start), Some(end)) = (offset_at(text, range.start), offset_at(text, range.end)) {
        text.replace_range(start..end, replacement);
    }
}

fn offset_at(text: &str, position: Position) -> Option<usize> {
    let mut offset = 0;
    let line = text.split_inclusive('\n').nth(position.line as usize)?;
    let line_without_newline = line
        .strip_suffix('\n')
        .unwrap_or(line)
        .strip_suffix('\r')
        .unwrap_or(line.strip_suffix('\n').unwrap_or(line));
    let mut utf16 = 0u32;
    let mut bytes = 0usize;
    for character in line_without_newline.chars() {
        if utf16 >= position.character {
            break;
        }
        utf16 += character.len_utf16() as u32;
        bytes += character.len_utf8();
    }
    if utf16 != position.character {
        return None;
    }
    for previous in text.split_inclusive('\n').take(position.line as usize) {
        offset += previous.len();
    }
    Some(offset + bytes)
}

fn workspace_root(params: &InitializeParams, config: &Path) -> PathBuf {
    #[allow(deprecated)]
    params
        .workspace_folders
        .as_ref()
        .and_then(|folders| folders.first())
        .and_then(|folder| folder.uri.to_file_path().ok())
        .or_else(|| {
            params
                .root_uri
                .as_ref()
                .and_then(|uri| uri.to_file_path().ok())
        })
        .or_else(|| config.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_utf8_byte_columns_to_utf16() {
        assert_eq!(utf16_column("a😀b", 5), 3);
    }

    #[test]
    fn diagnostic_range_covers_a_typescript_symbol() {
        assert_eq!(diagnostic_columns("styles.missingSymbol", 7), (7, 20));
    }

    #[test]
    fn diagnostic_range_covers_a_scss_class_name() {
        assert_eq!(diagnostic_columns(".border-top {}", 1), (1, 11));
    }

    #[test]
    fn unused_class_range_includes_the_selector_dot() {
        assert_eq!(
            diagnostic_columns_for_rule(".border-top {}", 1, "unused-class"),
            (0, 11)
        );
        assert_eq!(
            diagnostic_columns_for_rule(".border-top {}", 1, "missing-symbol"),
            (1, 11)
        );
    }

    #[test]
    fn unused_occurrence_range_includes_attached_pseudos() {
        let symbol = UnusedSymbol {
            location: style_contract::diagnostic::Location {
                path: PathBuf::from("example.module.scss"),
                line: 1,
                column: 2,
            },
            name: "title".into(),
            kind: UnusedSymbolKind::Class,
        };
        assert_eq!(
            unused_symbol_range(&symbol, ".title::placeholder,"),
            Some(Range::new(Position::new(0, 0), Position::new(0, 19)))
        );
        assert_eq!(
            unused_symbol_range(&symbol, ".title:focus::placeholder {"),
            Some(Range::new(Position::new(0, 0), Position::new(0, 25)))
        );
        assert_eq!(
            unused_symbol_range(&symbol, ".title:hover path {}"),
            Some(Range::new(Position::new(0, 0), Position::new(0, 17)))
        );
        assert_eq!(
            unused_symbol_range(&symbol, ".title:hover\n  path {}"),
            Some(Range::new(Position::new(0, 0), Position::new(1, 6)))
        );
        let diagnostic = unused_symbol_diagnostic(&symbol, ".title:focus::placeholder {").unwrap();
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(diagnostic.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
        assert_eq!(
            diagnostic.range,
            Range::new(Position::new(0, 0), Position::new(0, 25))
        );
    }

    #[test]
    fn tags_only_active_unused_diagnostics_as_unnecessary() {
        assert_eq!(
            diagnostic_tags("unused-class"),
            Some(vec![DiagnosticTag::UNNECESSARY])
        );
        assert_eq!(
            diagnostic_tags("unused-export"),
            Some(vec![DiagnosticTag::UNNECESSARY])
        );
        assert_eq!(diagnostic_tags("missing-symbol"), None);
    }

    #[test]
    fn diagnostic_range_preserves_utf16_columns() {
        assert_eq!(diagnostic_columns("😀 styles.missing", 12), (10, 17));
    }

    #[test]
    fn empty_rule_range_covers_a_multiline_selector_list() {
        let text = "😀 .first,\n.second:hover { }";
        let diagnostic = Diagnostic {
            location: style_contract::diagnostic::Location {
                path: PathBuf::from("example.module.scss"),
                line: 1,
                column: 6,
            },
            severity: Severity::Warning,
            rule: "empty-rule",
            message: "Empty style rule".into(),
        };
        assert_eq!(
            empty_rule_range(&diagnostic, text),
            Some(Range::new(Position::new(0, 3), Position::new(1, 13)))
        );
        assert_eq!(to_lsp(&diagnostic, text).tags, None);

        let interpolated = ".item-#{$state}[data-value=\"{\"] {}";
        let mut interpolated_diagnostic = diagnostic.clone();
        interpolated_diagnostic.location.column = 1;
        assert_eq!(
            empty_rule_range(&interpolated_diagnostic, interpolated),
            Some(Range::new(Position::new(0, 0), Position::new(0, 31)))
        );
    }

    #[test]
    fn module_import_range_covers_the_complete_specifier() {
        let text = "@use \"src/styles/table.module.scss\" as *;";
        let diagnostic = Diagnostic {
            location: style_contract::diagnostic::Location {
                path: PathBuf::from("example.module.scss"),
                line: 1,
                column: 7,
            },
            severity: Severity::Warning,
            rule: "module-to-module-import",
            message: "Example".into(),
        };
        assert_eq!(
            to_lsp(&diagnostic, text).range,
            Range::new(Position::new(0, 6), Position::new(0, 34))
        );
        assert_eq!(to_lsp(&diagnostic, text).tags, None);
    }

    #[test]
    fn applies_incremental_utf16_changes() {
        let mut text = "a😀b\nnext".to_owned();
        apply_change(
            &mut text,
            Some(Range::new(Position::new(0, 1), Position::new(0, 3))),
            "x",
        );
        assert_eq!(text, "axb\nnext");
    }

    #[test]
    fn compares_canonical_and_ordinary_forms_of_the_same_path() {
        let ordinary = std::env::current_exe().unwrap();
        let canonical = std::fs::canonicalize(&ordinary).unwrap();
        assert!(same_path(&canonical, &ordinary));
    }

    #[cfg(windows)]
    #[test]
    fn compares_extended_and_unprefixed_windows_paths() {
        let canonical = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let extended = canonical.to_string_lossy();
        let ordinary = PathBuf::from(extended.strip_prefix(r"\\?\").unwrap_or(&extended));
        assert!(same_path(&canonical, &ordinary));
    }
}
