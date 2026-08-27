use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DiagnosticTag, Location as LspLocation,
    Position, Range, Url,
};
use stylebreeze_analysis::{Diagnostic, Location, Severity, Span};

pub fn position_to_offset(source: &str, position: Position) -> Option<usize> {
    let mut line = 0u32;
    let mut offset = 0usize;
    for part in source.split_inclusive('\n') {
        if line == position.line {
            let text = part.strip_suffix('\n').unwrap_or(part);
            let mut units = 0u32;
            for (i, ch) in text.char_indices() {
                if units >= position.character {
                    return Some(offset + i);
                }
                units += ch.len_utf16() as u32;
                if units > position.character {
                    return None;
                }
            }
            return (units == position.character).then_some(offset + text.len());
        }
        offset += part.len();
        line += 1;
    }
    (line == position.line && position.character == 0).then_some(source.len())
}
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    let safe = offset.min(source.len());
    let head = &source[..safe];
    let line = head.bytes().filter(|b| *b == b'\n').count() as u32;
    let tail = head.rsplit_once('\n').map_or(head, |(_, t)| t);
    Position::new(line, tail.encode_utf16().count() as u32)
}
pub fn span_to_range(source: &str, span: Span) -> Range {
    Range::new(
        offset_to_position(source, span.start),
        offset_to_position(source, span.end),
    )
}
pub fn location_to_lsp(location: &Location, source: &str) -> Option<LspLocation> {
    Some(LspLocation::new(
        Url::from_file_path(&location.path).ok()?,
        span_to_range(source, location.span),
    ))
}
pub fn diagnostic_to_lsp(d: &Diagnostic, source: &str) -> LspDiagnostic {
    LspDiagnostic {
        range: span_to_range(source, d.location.span),
        severity: Some(match d.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(lsp_types::NumberOrString::String(d.code.into())),
        code_description: None,
        source: Some("stylebreeze".into()),
        message: d.message.clone(),
        related_information: None,
        tags: d.unnecessary.then(|| vec![DiagnosticTag::UNNECESSARY]),
        data: None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn utf16_positions() {
        let s = "a😀b\nç";
        assert_eq!(position_to_offset(s, Position::new(0, 3)), Some(5));
        assert_eq!(offset_to_position(s, 5), Position::new(0, 3));
    }

    #[test]
    fn unused_diagnostics_have_the_unnecessary_tag() {
        let diagnostic = Diagnostic {
            location: Location {
                path: PathBuf::from("x.module.scss"),
                span: Span { start: 1, end: 7 },
            },
            severity: Severity::Hint,
            code: "unused-export",
            message: "unused".into(),
            unnecessary: true,
        };
        let lsp = diagnostic_to_lsp(&diagnostic, ".unused {}");
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(lsp.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }
}
