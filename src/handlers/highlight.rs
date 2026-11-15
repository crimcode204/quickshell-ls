use tower_lsp::lsp_types::{DocumentHighlight, DocumentHighlightKind, Position};
use tree_sitter::Node;

use crate::{
    doc_state::DocState, handlers::resolver::find_all_references_in_doc, qml::node_to_lsp_range,
    registry::ComponentRegistry, ts_kinds,
};

pub fn document_highlight(
    position: Position,
    doc_state: &DocState,
    registry: &ComponentRegistry,
) -> Option<Vec<DocumentHighlight>> {
    let node = doc_state.node_at(position)?;
    let (_, nodes) = find_all_references_in_doc(node, doc_state, registry)?;

    let highlights = nodes
        .into_iter()
        .map(|n| DocumentHighlight {
            range: node_to_lsp_range(n),
            kind: Some(classify_highlight_kind(n)),
        })
        .collect();

    Some(highlights)
}

fn classify_highlight_kind(node: Node) -> DocumentHighlightKind {
    let Some(parent) = node.parent() else {
        return DocumentHighlightKind::TEXT;
    };

    match parent.kind() {
        ts_kinds::UI_PROPERTY => {
            if parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id()) {
                return DocumentHighlightKind::WRITE;
            }
        }
        ts_kinds::UI_BINDING => {
            if parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id()) {
                return DocumentHighlightKind::WRITE;
            }
            if parent.child_by_field_name("value").map(|n| n.id()) == Some(node.id()) {
                return DocumentHighlightKind::WRITE;
            }
        }
        ts_kinds::NESTED_IDENTIFIER => {
            if let Some(grandparent) = parent.parent()
                && grandparent.kind() == ts_kinds::UI_BINDING
                && grandparent.child_by_field_name("name").map(|n| n.id()) == Some(parent.id())
                && parent.child(2).map(|n| n.id()) == Some(node.id())
            {
                return DocumentHighlightKind::WRITE;
            }
        }
        _ => {}
    }
    DocumentHighlightKind::READ
}
