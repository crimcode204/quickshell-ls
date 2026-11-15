use std::collections::HashMap;

use tower_lsp::lsp_types::{Position, PrepareRenameResponse, TextEdit, Url, WorkspaceEdit};

use crate::{
    doc_state::DocState,
    handlers::resolver::{ExpressionMember, find_all_references_in_doc, resolve_expression},
    qml::node_to_lsp_range,
    registry::ComponentRegistry,
};

pub fn prepare_rename(
    position: Position,
    doc_state: &DocState,
    registry: &ComponentRegistry,
) -> Option<PrepareRenameResponse> {
    let node = doc_state.node_at(position)?;
    let member = resolve_expression(node, doc_state, registry)?;
    let text = doc_state.node_text(&node);

    if !is_renameable(&text, &member, doc_state) {
        return None;
    }

    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: node_to_lsp_range(node),
        placeholder: text,
    })
}

pub fn rename(
    uri: &Url,
    position: Position,
    new_name: String,
    doc_state: &DocState,
    registry: &ComponentRegistry,
) -> Option<WorkspaceEdit> {
    let node = doc_state.node_at(position)?;
    let text = doc_state.node_text(&node);
    let (member, refs) = find_all_references_in_doc(node, doc_state, registry)?;

    if !is_renameable(&text, &member, doc_state) {
        return None;
    }

    let edits: Vec<TextEdit> = refs
        .into_iter()
        .map(|n| TextEdit {
            range: node_to_lsp_range(n),
            new_text: new_name.clone(),
        })
        .collect();

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn is_renameable(node_text: &str, member: &ExpressionMember, doc_state: &DocState) -> bool {
    match member {
        ExpressionMember::LocalProperty { .. } => true,
        ExpressionMember::GlobalProperty { component, .. } => {
            let name = component.name();
            name == doc_state.name || name.starts_with(&format!("{}.", doc_state.name))
        }
        ExpressionMember::Component(component) => component
            .name()
            .starts_with(&format!("{}.", doc_state.name)),
        ExpressionMember::IdReference(_) => node_text != "parent",
    }
}
