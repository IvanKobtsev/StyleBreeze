use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{notification as notif, request as req, *};
use lsp_types::{notification::Notification as _, request::Request as _};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use stylebreeze_analysis::Project;
use stylebreeze_protocol::{diagnostic_to_lsp, location_to_lsp, position_to_offset, span_to_range};

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--version") {
        println!("stylebreeze {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let (connection, io_threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into()]),
            ..Default::default()
        }),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
            identifier: Some("stylebreeze".into()),
            inter_file_dependencies: true,
            workspace_diagnostics: true,
            work_done_progress_options: Default::default(),
        })),
        ..Default::default()
    };
    let init = connection.initialize(serde_json::to_value(capabilities)?)?;
    let params: InitializeParams = serde_json::from_value(init)?;
    let roots = workspace_roots(&params);
    let mut project = Project::new(roots);
    project.index_workspace();
    for msg in &connection.receiver {
        match msg {
            Message::Request(r) => {
                if connection.handle_shutdown(&r)? {
                    break;
                }
                handle_request(&connection, &project, r)?;
            }
            Message::Notification(n) => handle_notification(&connection, &mut project, n)?,
            Message::Response(_) => {}
        }
    }
    io_threads.join().context("LSP I/O threads failed")?;
    Ok(())
}
#[allow(deprecated)]
fn workspace_roots(p: &InitializeParams) -> Vec<PathBuf> {
    if let Some(f) = &p.workspace_folders {
        f.iter().filter_map(|x| x.uri.to_file_path().ok()).collect()
    } else {
        p.root_uri
            .as_ref()
            .and_then(|u| u.to_file_path().ok())
            .into_iter()
            .collect()
    }
}
fn handle_notification(c: &Connection, p: &mut Project, n: Notification) -> Result<()> {
    match n.method.as_str() {
        notif::DidOpenTextDocument::METHOD => {
            let q: DidOpenTextDocumentParams = serde_json::from_value(n.params)?;
            if let Ok(path) = q.text_document.uri.to_file_path() {
                p.open_or_update_file(
                    path.clone(),
                    q.text_document.text,
                    Some(q.text_document.version),
                );
                publish_all(c, p)?;
            }
        }
        notif::DidChangeTextDocument::METHOD => {
            let q: DidChangeTextDocumentParams = serde_json::from_value(n.params)?;
            if let (Ok(path), Some(change)) = (
                q.text_document.uri.to_file_path(),
                q.content_changes.into_iter().last(),
            ) {
                p.open_or_update_file(path.clone(), change.text, Some(q.text_document.version));
                publish_all(c, p)?;
            }
        }
        notif::DidCloseTextDocument::METHOD => {
            let q: DidCloseTextDocumentParams = serde_json::from_value(n.params)?;
            if let Ok(path) = q.text_document.uri.to_file_path() {
                p.close_file(&path);
                publish_all(c, p)?;
            }
        }
        notif::DidChangeWatchedFiles::METHOD => {
            let q: DidChangeWatchedFilesParams = serde_json::from_value(n.params)?;
            for e in q.changes {
                if let Ok(path) = e.uri.to_file_path() {
                    if e.typ == FileChangeType::DELETED {
                        p.remove_file(&path);
                    } else if let Ok(s) = std::fs::read_to_string(&path) {
                        p.open_or_update_file(path.clone(), s, None);
                    }
                    publish_all(c, p)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn handle_request(c: &Connection, p: &Project, r: Request) -> Result<()> {
    let id = r.id.clone();
    let result: Result<serde_json::Value> = match r.method.as_str() {
        req::GotoDefinition::METHOD => {
            let q: GotoDefinitionParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document_position_params
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let at =
                position_to_offset(source, q.text_document_position_params.position).unwrap_or(0);
            let locations: Vec<_> = p
                .definitions_at(&path, at)
                .into_iter()
                .filter_map(|l| p.source(&l.path).and_then(|s| location_to_lsp(&l, s)))
                .collect();
            Ok(serde_json::to_value(GotoDefinitionResponse::Array(
                locations,
            ))?)
        }
        req::References::METHOD => {
            let q: ReferenceParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document_position
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let at = position_to_offset(
                p.source(&path).unwrap_or(""),
                q.text_document_position.position,
            )
            .unwrap_or(0);
            let locs: Vec<_> = p
                .references_at(&path, at, q.context.include_declaration)
                .into_iter()
                .filter_map(|l| p.source(&l.path).and_then(|s| location_to_lsp(&l, s)))
                .collect();
            Ok(serde_json::to_value(locs)?)
        }
        req::Completion::METHOD => {
            let q: CompletionParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document_position
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let at = position_to_offset(
                p.source(&path).unwrap_or(""),
                q.text_document_position.position,
            )
            .unwrap_or(0);
            let items: Vec<_> = p
                .completions_at(&path, at)
                .into_iter()
                .map(|label| CompletionItem {
                    sort_text: Some(format!("0000-{label}")),
                    preselect: Some(true),
                    label,
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some("CSS Module export".into()),
                    ..Default::default()
                })
                .collect();
            Ok(serde_json::to_value(CompletionResponse::Array(items))?)
        }
        req::HoverRequest::METHOD => {
            let q: HoverParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document_position_params
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let at =
                position_to_offset(source, q.text_document_position_params.position).unwrap_or(0);
            let hover = p.hover_at(&path, at).map(|info| Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: info.markdown,
                }),
                range: Some(span_to_range(source, info.range)),
            });
            Ok(serde_json::to_value(hover)?)
        }
        "stylebreeze/modifierDecorations" => {
            let q: TextDocumentIdentifier = serde_json::from_value(r.params)?;
            let path = q
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let items: Vec<_> = p.modifier_decorations(&path).into_iter().map(|item| {
                serde_json::json!({
                    "modifier": item.modifier,
                    "alternatives": item.alternatives.into_iter().map(|alternative| {
                        serde_json::json!({
                            "requiredAll": alternative.required_all,
                            "baseLocations": alternative.base_locations.into_iter().filter_map(|location| {
                                p.source(&location.path).and_then(|base_source| location_to_lsp(&location, base_source))
                            }).collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                    "range": span_to_range(source, item.range),
                    "selectorRange": span_to_range(source, item.selector),
                    "standalone": item.standalone,
                })
            }).collect();
            Ok(serde_json::to_value(items)?)
        }
        req::PrepareRenameRequest::METHOD => {
            let q: TextDocumentPositionParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let at = position_to_offset(source, q.position).unwrap_or(0);
            Ok(serde_json::to_value(
                p.prepare_rename(&path, at)
                    .ok()
                    .map(|s| span_to_range(source, s)),
            )?)
        }
        req::Rename::METHOD => {
            let q: RenameParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document_position
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let at = position_to_offset(
                p.source(&path).unwrap_or(""),
                q.text_document_position.position,
            )
            .unwrap_or(0);
            let edits = p
                .rename(&path, at, &q.new_name)
                .map_err(anyhow::Error::msg)?;
            let mut changes: HashMap<Url, Vec<lsp_types::TextEdit>> = HashMap::new();
            for e in edits {
                if let (Ok(uri), Some(source)) = (
                    Url::from_file_path(&e.location.path),
                    p.source(&e.location.path),
                ) {
                    changes.entry(uri).or_default().push(lsp_types::TextEdit {
                        range: span_to_range(source, e.location.span),
                        new_text: e.new_text,
                    })
                }
            }
            Ok(serde_json::to_value(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })?)
        }
        req::DocumentDiagnosticRequest::METHOD => {
            let q: DocumentDiagnosticParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let items = p
                .diagnostics_for(&path)
                .iter()
                .map(|d| diagnostic_to_lsp(d, source))
                .collect();
            Ok(serde_json::to_value(DocumentDiagnosticReport::Full(
                RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items,
                    },
                },
            ))?)
        }
        _ => Ok(serde_json::Value::Null),
    };
    match result {
        Ok(v) => c.sender.send(Message::Response(Response::new_ok(id, v)))?,
        Err(e) => c.sender.send(Message::Response(Response::new_err(
            id,
            lsp_server::ErrorCode::InvalidParams as i32,
            e.to_string(),
        )))?,
    }
    Ok(())
}
fn publish(c: &Connection, p: &Project, path: &Path) -> Result<()> {
    let Some(source) = p.source(path) else {
        return Ok(());
    };
    let Ok(uri) = Url::from_file_path(path) else {
        return Ok(());
    };
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics: p
            .diagnostics_for(path)
            .iter()
            .map(|d| diagnostic_to_lsp(d, source))
            .collect(),
        version: p.version(path),
    };
    c.sender.send(Message::Notification(Notification::new(
        notif::PublishDiagnostics::METHOD.to_string(),
        params,
    )))?;
    Ok(())
}
fn publish_all(c: &Connection, p: &Project) -> Result<()> {
    for path in p.file_paths() {
        publish(c, p, path)?
    }
    Ok(())
}
