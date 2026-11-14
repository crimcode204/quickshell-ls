use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolKind};

use crate::doc_state::{DocState, LocalInstance};

pub fn document_symbol(doc_state: &DocState) -> Option<DocumentSymbolResponse> {
    let symbols = doc_state
        .instances
        .iter()
        .map(|instance| instance_to_symbol(instance, doc_state))
        .collect();

    Some(DocumentSymbolResponse::Nested(symbols))
}

#[allow(deprecated)]
fn instance_to_symbol(instance: &LocalInstance, doc_state: &DocState) -> DocumentSymbol {
    let range = doc_state.byte_range_to_lsp_range(&instance.byte_range);

    let prop_symbols = instance.properties.iter().filter_map(|prop| {
        let prop_range = doc_state.byte_range_to_lsp_range(prop.byte_range.as_ref()?);
        Some(DocumentSymbol {
            name: prop.name.clone(),
            detail: Some(format!("{}{}", prop.modifier, prop.p_type)),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range: prop_range,
            selection_range: prop_range,
            children: None,
        })
    });

    let child_symbols = instance
        .children
        .iter()
        .map(|child| instance_to_symbol(child, doc_state));

    let children: Vec<DocumentSymbol> = prop_symbols.chain(child_symbols).collect();

    let (name, detail) = match &instance.id {
        Some(id) => (
            format!("{} ({})", instance.type_name, id),
            Some(format!("id: {}", id)),
        ),
        None => (instance.type_name.clone(), None),
    };

    DocumentSymbol {
        name,
        detail,
        kind: SymbolKind::OBJECT,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}
