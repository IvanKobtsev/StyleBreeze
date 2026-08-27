use super::Span;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorRule {
    pub source_span: Span,
    pub resolved: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeRole {
    Selected,
    RequiredSupport,
    RelationalWitness,
    IllustrativeSpacer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RelationshipKind {
    Descendant,
    Child,
    AdjacentSibling,
    GeneralSibling,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    pub from: usize,
    pub to: usize,
    pub kind: RelationshipKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAttribute {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateRequirement {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewNode {
    pub id: usize,
    pub tag: String,
    pub element_id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<PreviewAttribute>,
    pub states: Vec<StateRequirement>,
    pub role: NodeRole,
    pub parent: Option<usize>,
    pub order: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorPreview {
    pub resolved_selector: String,
    pub nodes: Vec<PreviewNode>,
    pub relationships: Vec<Relationship>,
    pub subject: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedReason {
    pub message: String,
}

#[derive(Default, Clone, Debug)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attributes: Vec<PreviewAttribute>,
    states: Vec<StateRequirement>,
    has: Vec<String>,
}

pub fn collect_selector_rules(source: &str) -> Vec<SelectorRule> {
    let bytes = source.as_bytes();
    let mut rules = Vec::new();
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut statement_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if super::skip_string_or_comment(bytes, &mut i) {
            continue;
        }
        match bytes[i] {
            b'{' => {
                let span = super::selector_span(source, statement_start, i);
                let text = source[span.start..span.end].trim();
                let parent = stack.iter().rev().find_map(|x| x.as_deref());
                let resolved = if text.is_empty()
                    || text.starts_with('@')
                    || text.contains(':')
                        && text
                            .split_whitespace()
                            .next()
                            .is_some_and(|x| x.ends_with(':'))
                {
                    None
                } else if let Some(parent) = parent {
                    Some(if text.contains('&') {
                        text.replace('&', parent)
                    } else {
                        format!("{parent} {text}")
                    })
                } else {
                    Some(text.to_string())
                };
                if let Some(resolved) = &resolved {
                    rules.push(SelectorRule {
                        source_span: span,
                        resolved: resolved.clone(),
                    });
                }
                stack.push(resolved);
                statement_start = i + 1;
            }
            b'}' => {
                stack.pop();
                statement_start = i + 1;
            }
            b';' => statement_start = i + 1,
            _ => {}
        }
        i += 1;
    }
    rules
}

pub fn preview_selector(selector: &str) -> Result<SelectorPreview, UnsupportedReason> {
    let (compounds, combinators) = parse_chain(selector, false)?;
    if compounds.is_empty() {
        return Err(unsupported("empty selector"));
    }
    let mut preview = SelectorPreview {
        resolved_selector: selector.into(),
        nodes: Vec::new(),
        relationships: Vec::new(),
        subject: 0,
    };
    let mut ids = Vec::new();
    for (index, compound) in compounds.iter().enumerate() {
        let relation = index.checked_sub(1).map(|i| combinators[i]);
        let previous = ids.last().copied();
        let (parent, order) = placement(&mut preview, previous, relation);
        let id = push_node(
            &mut preview,
            compound,
            NodeRole::RequiredSupport,
            parent,
            order,
        );
        if let (Some(from), Some(kind)) = (previous, relation) {
            preview
                .relationships
                .push(Relationship { from, to: id, kind });
        }
        ids.push(id);
    }
    preview.subject = *ids.last().unwrap();
    preview.nodes[preview.subject].role = NodeRole::Selected;
    for (compound, anchor) in compounds.iter().zip(ids) {
        for relative in &compound.has {
            add_has_witness(&mut preview, anchor, relative)?;
        }
    }
    Ok(preview)
}

fn placement(
    preview: &mut SelectorPreview,
    previous: Option<usize>,
    relation: Option<RelationshipKind>,
) -> (Option<usize>, i32) {
    let Some(previous) = previous else {
        return (None, 0);
    };
    match relation.unwrap() {
        RelationshipKind::Descendant | RelationshipKind::Child => (Some(previous), 0),
        RelationshipKind::AdjacentSibling => (
            preview.nodes[previous].parent,
            preview.nodes[previous].order + 1,
        ),
        RelationshipKind::GeneralSibling => {
            let parent = preview.nodes[previous].parent;
            let order = preview.nodes[previous].order + 1;
            let spacer = PreviewNode {
                id: preview.nodes.len(),
                tag: "div".into(),
                element_id: None,
                classes: vec![],
                attributes: vec![],
                states: vec![],
                role: NodeRole::IllustrativeSpacer,
                parent,
                order,
            };
            preview.nodes.push(spacer);
            (parent, order + 1)
        }
    }
}

fn push_node(
    preview: &mut SelectorPreview,
    c: &Compound,
    role: NodeRole,
    parent: Option<usize>,
    order: i32,
) -> usize {
    let id = preview.nodes.len();
    preview.nodes.push(PreviewNode {
        id,
        tag: c.tag.clone().unwrap_or_else(|| "div".into()),
        element_id: c.id.clone(),
        classes: c.classes.clone(),
        attributes: c.attributes.clone(),
        states: c.states.clone(),
        role,
        parent,
        order,
    });
    id
}

fn add_has_witness(
    preview: &mut SelectorPreview,
    anchor: usize,
    relative: &str,
) -> Result<(), UnsupportedReason> {
    let (compounds, combinators) = parse_chain(relative, true)?;
    let mut previous = anchor;
    for (i, compound) in compounds.iter().enumerate() {
        let relation = combinators[i];
        let (parent, order) = placement(preview, Some(previous), Some(relation));
        let id = push_node(
            preview,
            compound,
            NodeRole::RelationalWitness,
            parent,
            order,
        );
        preview.relationships.push(Relationship {
            from: previous,
            to: id,
            kind: relation,
        });
        previous = id;
    }
    Ok(())
}

fn parse_chain(
    input: &str,
    relative: bool,
) -> Result<(Vec<Compound>, Vec<RelationshipKind>), UnsupportedReason> {
    if input.contains(',') {
        return Err(unsupported("selector lists are not supported yet"));
    }
    if [":is(", ":where(", ":not(", "::", "#{"]
        .iter()
        .any(|x| input.contains(x))
    {
        return Err(unsupported(
            "selector contains unsupported functional or generated syntax",
        ));
    }
    let bytes = input.as_bytes();
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut depth = 0usize;
    let mut pending_relative = None;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        if !at_end {
            match bytes[i] {
                b'[' | b'(' => depth += 1,
                b']' | b')' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        let explicit = !at_end && depth == 0 && matches!(bytes[i], b'>' | b'+' | b'~');
        let whitespace = !at_end && depth == 0 && bytes[i].is_ascii_whitespace();
        if at_end || explicit || whitespace {
            let part = input[start..i].trim();
            if !part.is_empty() {
                compounds.push(parse_compound(part)?);
            }
            if at_end {
                break;
            }
            if explicit {
                let kind = match bytes[i] {
                    b'>' => RelationshipKind::Child,
                    b'+' => RelationshipKind::AdjacentSibling,
                    _ => RelationshipKind::GeneralSibling,
                };
                if compounds.is_empty() && relative {
                    pending_relative = Some(kind);
                } else {
                    combinators.push(kind);
                }
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                start = i;
                continue;
            }
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && matches!(bytes[j], b'>' | b'+' | b'~') {
                i = j;
                start = j;
                continue;
            }
            if !compounds.is_empty() && combinators.len() < compounds.len() {
                combinators.push(RelationshipKind::Descendant);
            }
            i = j;
            start = i;
            continue;
        }
        i += 1;
    }
    if relative {
        let first = pending_relative.unwrap_or(RelationshipKind::Descendant);
        combinators.insert(0, first);
        if combinators.len() != compounds.len() {
            return Err(unsupported("invalid relative selector"));
        }
    } else if combinators.len() + 1 != compounds.len() {
        return Err(unsupported("invalid selector relationship"));
    }
    Ok((compounds, combinators))
}

fn parse_compound(text: &str) -> Result<Compound, UnsupportedReason> {
    let bytes = text.as_bytes();
    let mut c = Compound::default();
    let mut i = 0;
    if bytes.first() == Some(&b'&') {
        return Err(unsupported("unresolved nesting selector"));
    }
    while i < bytes.len() {
        match bytes[i] {
            b'*' => i += 1,
            b'.' | b'#' => {
                let marker = bytes[i];
                i += 1;
                let start = i;
                while i < bytes.len() && ident(bytes[i]) {
                    i += 1;
                }
                if start == i {
                    return Err(unsupported("invalid identifier"));
                }
                let value = text[start..i].to_string();
                if marker == b'.' {
                    c.classes.push(value)
                } else if c.id.replace(value).is_some() {
                    return Err(unsupported("compound requires multiple IDs"));
                }
            }
            b'[' => {
                let end = find_close(bytes, i, b'[', b']')?;
                c.attributes.push(parse_attribute(&text[i + 1..end])?);
                i = end + 1;
            }
            b':' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && ident(bytes[end]) {
                    end += 1;
                }
                let name = &text[start..end];
                if end < bytes.len() && bytes[end] == b'(' {
                    let close = find_close(bytes, end, b'(', b')')?;
                    if name != "has" {
                        return Err(unsupported("unsupported functional pseudo-class"));
                    }
                    c.has.push(text[end + 1..close].trim().into());
                    i = close + 1;
                } else {
                    if !supported_state(name) {
                        return Err(unsupported(&format!("unsupported pseudo-class :{name}")));
                    }
                    c.states.push(StateRequirement { name: name.into() });
                    i = end;
                }
            }
            b if ident(b) => {
                let start = i;
                while i < bytes.len() && ident(bytes[i]) {
                    i += 1;
                }
                if c.tag.replace(text[start..i].into()).is_some() {
                    return Err(unsupported("multiple element types in compound"));
                }
            }
            _ => return Err(unsupported("unsupported selector token")),
        }
    }
    Ok(c)
}

fn parse_attribute(text: &str) -> Result<PreviewAttribute, UnsupportedReason> {
    let text = text.trim();
    for op in ["~=", "|=", "^=", "$=", "*=", "="] {
        if let Some((name, value)) = text.split_once(op) {
            let name = name.trim();
            if name.is_empty() {
                break;
            }
            let raw = value.trim().trim_matches(['\'', '"']);
            let value = match op {
                "$=" => format!("value{raw}"),
                "*=" => format!("value{raw}value"),
                "~=" => raw.into(),
                "|=" => raw.into(),
                _ => raw.into(),
            };
            return Ok(PreviewAttribute {
                name: name.into(),
                value: Some(value),
            });
        }
    }
    if !text.is_empty() && text.bytes().all(|b| ident(b) || b == b':') {
        Ok(PreviewAttribute {
            name: text.into(),
            value: None,
        })
    } else {
        Err(unsupported("unsupported attribute selector"))
    }
}

fn find_close(bytes: &[u8], start: usize, open: u8, close: u8) -> Result<usize, UnsupportedReason> {
    let mut depth = 0;
    for (i, b) in bytes.iter().enumerate().skip(start) {
        if *b == open {
            depth += 1
        } else if *b == close {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
    }
    Err(unsupported("unclosed selector function or attribute"))
}
fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-') || b >= 0x80
}
fn supported_state(s: &str) -> bool {
    matches!(
        s,
        "hover"
            | "focus"
            | "focus-visible"
            | "focus-within"
            | "active"
            | "checked"
            | "disabled"
            | "enabled"
            | "required"
            | "optional"
            | "read-only"
            | "read-write"
            | "placeholder-shown"
            | "open"
            | "popover-open"
            | "target"
            | "visited"
            | "link"
    )
}
fn unsupported(message: &str) -> UnsupportedReason {
    UnsupportedReason {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_has_no_spacer_but_general_does() {
        let adjacent = preview_selector(".a + .b").unwrap();
        assert!(
            !adjacent
                .nodes
                .iter()
                .any(|n| n.role == NodeRole::IllustrativeSpacer)
        );
        let general = preview_selector(".a ~ .b").unwrap();
        assert!(
            general
                .nodes
                .iter()
                .any(|n| n.role == NodeRole::IllustrativeSpacer)
        );
    }
    #[test]
    fn relational_subject_and_state() {
        let p = preview_selector(".searchWrapper:has(+ .popup:popover-open) .arrow").unwrap();
        assert!(p.nodes[p.subject].classes.contains(&"arrow".into()));
        let popup = p
            .nodes
            .iter()
            .find(|n| n.classes.contains(&"popup".into()))
            .unwrap();
        assert_eq!(popup.role, NodeRole::RelationalWitness);
        assert_eq!(popup.states[0].name, "popover-open");
    }
    #[test]
    fn general_has_uses_a_spacer_and_preserves_attributes() {
        let preview =
            preview_selector("section[data-kind^='news']:has(~ .popup) > .title").unwrap();
        assert!(
            preview
                .nodes
                .iter()
                .any(|node| node.role == NodeRole::IllustrativeSpacer)
        );
        assert_eq!(preview.nodes[0].tag, "section");
        assert_eq!(preview.nodes[0].attributes[0].name, "data-kind");
        assert_eq!(
            preview.nodes[0].attributes[0].value.as_deref(),
            Some("news")
        );
        assert_eq!(preview.nodes[preview.subject].parent, Some(0));
    }
    #[test]
    fn nested_rules_are_resolved() {
        let rules = collect_selector_rules(".a { &:has(+ .b) .c {} }");
        assert_eq!(rules[1].resolved, ".a:has(+ .b) .c");
    }
}
