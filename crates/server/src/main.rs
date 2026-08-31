use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{notification as notif, request as req, *};
use lsp_types::{notification::Notification as _, request::Request as _};
use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    sync::OnceLock,
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
            trigger_characters: Some(vec![".".into(), "-".into()]),
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
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: vec![SemanticTokenType::new("styleBreezeCustomProperty")],
                    token_modifiers: vec![
                        "global",
                        "local",
                        "registered",
                        "imported",
                        "exported",
                        "unresolved",
                        "declaration",
                    ]
                    .into_iter()
                    .map(SemanticTokenModifier::new)
                    .collect(),
                },
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                work_done_progress_options: Default::default(),
            },
        )),
        inlay_hint_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..Default::default()
    };
    let init = connection.initialize(serde_json::to_value(capabilities)?)?;
    let params: InitializeParams = serde_json::from_value(init)?;
    let roots = workspace_roots(&params);
    debug_log(format_args!("initialize roots={roots:?}"));
    let mut project = Project::new(roots);
    project.index_workspace();
    debug_log(format_args!(
        "workspace indexed files={}",
        project.file_paths().count()
    ));
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
                log_sass_file("open", p, &path);
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
                log_sass_file("change", p, &path);
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
                        log_sass_file("watched-file", p, &path);
                    }
                    publish_all(c, p)?;
                }
            }
        }
        notif::DidChangeConfiguration::METHOD => {
            let q: DidChangeConfigurationParams = serde_json::from_value(n.params)?;
            let settings = q.settings.get("styleBreeze").unwrap_or(&q.settings);
            let custom = settings
                .get("customProperties")
                .unwrap_or(&serde_json::Value::Null);
            let selectors = custom
                .get("globalSelectors")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec![":root".into()]);
            let presentation = custom
                .get("presentation")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                        .collect()
                })
                .unwrap_or_default();
            p.set_global_selectors(selectors);
            p.set_property_presentation(presentation);
            let sass_roots = settings
                .get("scss")
                .and_then(|v| v.get("loadPaths"))
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            p.set_sass_load_root_strings(sass_roots);
            debug_log(format_args!("configuration updated"));
            publish_all(c, p)?;
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
            debug_log(format_args!(
                "definition path={} offset={at} targets={}",
                path.display(),
                locations.len()
            ));
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
            debug_log(format_args!(
                "references path={} offset={at} include_declaration={} targets={}",
                path.display(),
                q.context.include_declaration,
                locs.len()
            ));
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
            let source = p.source(&path).unwrap_or("");
            let items: Vec<_> = p
                .completion_items_at(&path, at)
                .into_iter()
                .map(|item| CompletionItem {
                    sort_text: Some(format!("0000-{}-{}", item.label, item.detail)),
                    preselect: Some(true),
                    label: item.label.clone(),
                    kind: Some(match item.kind {
                        Some(stylebreeze_stylesheet_parser::SassSymbolKind::Variable) => {
                            CompletionItemKind::VARIABLE
                        }
                        Some(stylebreeze_stylesheet_parser::SassSymbolKind::Mixin) => {
                            CompletionItemKind::METHOD
                        }
                        Some(stylebreeze_stylesheet_parser::SassSymbolKind::Function) => {
                            CompletionItemKind::FUNCTION
                        }
                        None => CompletionItemKind::FIELD,
                    }),
                    detail: Some(item.detail),
                    text_edit: item.kind.map(|_| {
                        CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range: span_to_range(source, item.replace_span),
                            new_text: item.label.clone(),
                        })
                    }),
                    additional_text_edits: (!item.additional_edits.is_empty()).then(|| {
                        item.additional_edits
                            .into_iter()
                            .filter_map(|edit| {
                                p.source(&edit.location.path).map(|edit_source| {
                                    lsp_types::TextEdit {
                                        range: span_to_range(edit_source, edit.location.span),
                                        new_text: edit.new_text,
                                    }
                                })
                            })
                            .collect()
                    }),
                    ..Default::default()
                })
                .collect();
            debug_log(format_args!(
                "completion path={} offset={at} items={}",
                path.display(),
                items.len()
            ));
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
        "stylebreeze/selectorPreview" => {
            let q: TextDocumentPositionParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let at = position_to_offset(source, q.position).unwrap_or(0);
            let response = p.selector_preview_at(&path, at).map(|info| {
                serde_json::json!({
                    "range": span_to_range(source, info.range),
                    "preview": info.preview,
                    "unsupported": info.unsupported,
                })
            });
            Ok(serde_json::to_value(response)?)
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
        "stylebreeze/fixSassImports" => {
            let q: TextDocumentIdentifier = serde_json::from_value(r.params)?;
            let path = q
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let edits: Vec<_> = p
                .fix_sass_imports(&path)
                .into_iter()
                .map(|edit| lsp_types::TextEdit {
                    range: span_to_range(source, edit.location.span),
                    new_text: edit.new_text,
                })
                .collect();
            debug_log(format_args!(
                "fix-sass-imports path={} edits={}",
                path.display(),
                edits.len()
            ));
            Ok(serde_json::json!({ "version": p.version(&path), "edits": edits }))
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
        req::SemanticTokensFullRequest::METHOD => {
            let q: SemanticTokensParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let modifiers = [
                "global",
                "local",
                "registered",
                "imported",
                "exported",
                "unresolved",
                "declaration",
            ];
            let mut absolute = Vec::new();
            for item in p.custom_property_occurrences_for(&path, "semantic") {
                let pos = stylebreeze_protocol::offset_to_position(source, item.span.start);
                let end = stylebreeze_protocol::offset_to_position(source, item.span.end);
                let mut bits = 0;
                for role in &item.roles {
                    if let Some(i) = modifiers.iter().position(|m| m == role) {
                        bits |= 1 << i;
                    }
                }
                if item.declaration {
                    bits |= 1 << 6;
                }
                absolute.push((pos.line, pos.character, end.character - pos.character, bits));
            }
            absolute.sort();
            let mut last_line = 0;
            let mut last_char = 0;
            let data = absolute
                .into_iter()
                .map(|(line, ch, len, bits)| {
                    let dl = line - last_line;
                    let ds = if dl == 0 { ch - last_char } else { ch };
                    last_line = line;
                    last_char = ch;
                    SemanticToken {
                        delta_line: dl,
                        delta_start: ds,
                        length: len,
                        token_type: 0,
                        token_modifiers_bitset: bits,
                    }
                })
                .collect();
            Ok(serde_json::to_value(SemanticTokensResult::Tokens(
                SemanticTokens {
                    result_id: None,
                    data,
                },
            ))?)
        }
        req::InlayHintRequest::METHOD => {
            let q: InlayHintParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let hints: Vec<_> = p
                .custom_property_occurrences_for(&path, "inlayHint")
                .into_iter()
                .filter(|o| o.declaration)
                .map(|o| InlayHint {
                    position: stylebreeze_protocol::offset_to_position(source, o.span.end),
                    label: InlayHintLabel::String(format!(" {}", o.roles.join(" + "))),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                })
                .collect();
            Ok(serde_json::to_value(hints)?)
        }
        req::CodeActionRequest::METHOD => {
            let q: CodeActionParams = serde_json::from_value(r.params)?;
            let path = q
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
            let source = p.source(&path).unwrap_or("");
            let mut actions = Vec::new();
            let context_diagnostics = q.context.diagnostics;
            for diagnostic in context_diagnostics.iter().filter(|d|matches!(&d.code,Some(NumberOrString::String(code)) if code=="unresolved-custom-property")).cloned() {
                let start=stylebreeze_protocol::position_to_offset(source,diagnostic.range.start).unwrap_or(0);let end=stylebreeze_protocol::position_to_offset(source,diagnostic.range.end).unwrap_or(start);let name=&source[start..end];let uri=q.text_document.uri.clone();
                let line_start=Position::new(diagnostic.range.start.line,0);
                for(title,range,new_text,preferred)in[
                    ("Create local declaration",Range::new(line_start,line_start),format!("  {name}: initial;\n"),true),
                    ("Mark as exported",Range::new(Position::new(0,0),Position::new(0,0)),format!("/* @export-props: {name} */\n"),false),
                    ("Suppress unresolved property",Range::new(line_start,line_start),"/* @suppress-unresolved-prop */\n".into(),false),
                ]{let edit=WorkspaceEdit{changes:Some(HashMap::from([(uri.clone(),vec![lsp_types::TextEdit{range,new_text}])])),document_changes:None,change_annotations:None};actions.push(CodeActionOrCommand::CodeAction(CodeAction{title:title.into(),kind:Some(CodeActionKind::QUICKFIX),diagnostics:Some(vec![diagnostic.clone()]),edit:Some(edit),is_preferred:Some(preferred),..Default::default()}));}
                for candidate in p.property_declaration_sources(name).into_iter().filter(|candidate|candidate!=&path){if let Some(parent)=path.parent() && let Ok(relative)=candidate.strip_prefix(parent){let specifier=format!("./{}",relative.to_string_lossy().replace('\\',"/"));let new_text=format!("/* @import-props \"{specifier}\": {name} */\n");let edit=WorkspaceEdit{changes:Some(HashMap::from([(uri.clone(),vec![lsp_types::TextEdit{range:Range::new(Position::new(0,0),Position::new(0,0)),new_text}])])),document_changes:None,change_annotations:None};actions.push(CodeActionOrCommand::CodeAction(CodeAction{title:format!("Import from {specifier}"),kind:Some(CodeActionKind::QUICKFIX),diagnostics:Some(vec![diagnostic.clone()]),edit:Some(edit),..Default::default()}));}}
                if let Some(similar)=p.known_property_names().into_iter().filter(|candidate|candidate!=name).min_by_key(|candidate|edit_distance(name,candidate)).filter(|candidate|edit_distance(name,candidate)<=3){let edit=WorkspaceEdit{changes:Some(HashMap::from([(uri.clone(),vec![lsp_types::TextEdit{range:diagnostic.range,new_text:similar.clone()}])])),document_changes:None,change_annotations:None};actions.push(CodeActionOrCommand::CodeAction(CodeAction{title:format!("Replace with {similar}"),kind:Some(CodeActionKind::QUICKFIX),diagnostics:Some(vec![diagnostic.clone()]),edit:Some(edit),..Default::default()}));}
            }
            for diagnostic in context_diagnostics.iter().filter(|d|matches!(&d.code,Some(NumberOrString::String(code)) if matches!(code.as_str(),"missing-imported-property"|"duplicate-property-import"|"unused-property-import"))){let range=import_removal_range(source,diagnostic.range);let edit=WorkspaceEdit{changes:Some(HashMap::from([(q.text_document.uri.clone(),vec![lsp_types::TextEdit{range,new_text:String::new()}])])),document_changes:None,change_annotations:None};actions.push(CodeActionOrCommand::CodeAction(CodeAction{title:"Remove property from import".into(),kind:Some(CodeActionKind::QUICKFIX),diagnostics:Some(vec![diagnostic.clone()]),edit:Some(edit),is_preferred:Some(true),..Default::default()}));}
            Ok(serde_json::to_value(actions)?)
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
fn debug_log(arguments: fmt::Arguments<'_>) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| {
        std::env::var("STYLEBREEZE_LOG").is_ok_and(|value| !value.is_empty() && value != "off")
    }) {
        eprintln!("[stylebreeze] {arguments}");
    }
}
fn log_sass_file(event: &str, project: &Project, path: &Path) {
    if let Some(summary) = project.sass_debug_summary(path) {
        debug_log(format_args!(
            "scss-{event} path={} {summary}",
            path.display()
        ));
    }
}
fn edit_distance(a: &str, b: &str) -> usize {
    let mut previous: Vec<_> = (0..=b.len()).collect();
    for (i, ac) in a.bytes().enumerate() {
        let mut current = vec![i + 1];
        for (j, bc) in b.bytes().enumerate() {
            current.push(
                (previous[j + 1] + 1)
                    .min(current[j] + 1)
                    .min(previous[j] + usize::from(ac != bc)),
            );
        }
        previous = current;
    }
    previous[b.len()]
}
fn import_removal_range(source: &str, range: Range) -> Range {
    let mut start = stylebreeze_protocol::position_to_offset(source, range.start).unwrap_or(0);
    let mut end = stylebreeze_protocol::position_to_offset(source, range.end).unwrap_or(start);
    let bytes = source.as_bytes();
    let mut cursor = end;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor < bytes.len() && bytes[cursor] == b',' {
        end = cursor + 1;
    } else {
        cursor = start;
        while cursor > 0 && bytes[cursor - 1].is_ascii_whitespace() {
            cursor -= 1;
        }
        if cursor > 0 && bytes[cursor - 1] == b',' {
            start = cursor - 1;
        }
    }
    Range::new(
        stylebreeze_protocol::offset_to_position(source, start),
        stylebreeze_protocol::offset_to_position(source, end),
    )
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
